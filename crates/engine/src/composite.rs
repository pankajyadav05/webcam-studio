//! Putting a new background behind a person: box blur, mask-based blending and the
//! [`Compositor`] that blurs or replaces the background without colour fringes.
//!
//! The effects work on a person mask where 255 is the person, 0 is the background and values in
//! between are the soft edge. Two flaws of a plain "blur the whole frame, then blend" are avoided:
//!
//! * Halo: the blurred background is built only from background pixels. Each pixel is weighted
//!   by how much background it shows (`w`, from `(255 - alpha) / 255`, see `WEIGHT_POWER`) and
//!   the blur is `blur(frame * w) / blur(w)`, so the person's colours never bleed outwards.
//! * Fringe: an edge pixel still holds some of the old background. The person's own colour is
//!   recovered with `F = (pixel - (1 - a) * old_background) / a`, using the same weighted blur
//!   (small radius) as the estimate of the old background, and the edge is then composited with
//!   `F * a + new_background * (1 - a)`.

use rayon::prelude::*;
use std::borrow::Cow;
use std::sync::OnceLock;
use studio_core::frame::VideoFrame;
use studio_segmentation::Mask;

/// Smallest and largest blur radius in pixels.
pub const MIN_BLUR_RADIUS: u32 = 1;
/// Largest blur radius in pixels.
pub const MAX_BLUR_RADIUS: u32 = 64;

/// Blurs a frame with a separable box filter. The radius is clamped to 1..=64 and pixels beyond
/// the edge count as copies of the edge pixel. The cost is proportional to the number of pixels,
/// not the radius, because each pass keeps a running sum. The result has no row padding.
///
/// This blurs every pixel the same way. To blur only a background behind a person without
/// colour bleeding use [`Compositor::blur_background`].
pub fn box_blur(frame: &VideoFrame, radius: u32) -> VideoFrame {
    let radius = radius.clamp(MIN_BLUR_RADIUS, MAX_BLUR_RADIUS) as usize;
    let width = frame.width as usize;
    let height = frame.height as usize;
    let mut out = VideoFrame::new_bgra(frame.width, frame.height, frame.timestamp_us);
    if width == 0 || height == 0 || frame.data.len() < frame.stride * (height - 1) + width * 4 {
        return out;
    }
    let row_bytes = width * 4;
    let window = (2 * radius + 1) as u32;
    let half = window / 2;

    // Horizontal pass: frame -> tmp, one row per task.
    let mut tmp = vec![0u8; row_bytes * height];
    tmp.par_chunks_mut(row_bytes)
        .enumerate()
        .for_each(|(y, dst)| {
            let src = &frame.data[y * frame.stride..y * frame.stride + row_bytes];
            let at = |x: isize| -> &[u8] {
                let x = x.clamp(0, width as isize - 1) as usize;
                &src[x * 4..x * 4 + 4]
            };
            let mut sum = [0u32; 4];
            for k in -(radius as isize)..=radius as isize {
                for (s, v) in sum.iter_mut().zip(at(k)) {
                    *s += *v as u32;
                }
            }
            for x in 0..width {
                for c in 0..4 {
                    dst[x * 4 + c] = ((sum[c] + half) / window) as u8;
                }
                let add = at(x as isize + radius as isize + 1);
                let sub = at(x as isize - radius as isize);
                for c in 0..4 {
                    sum[c] = sum[c] + add[c] as u32 - sub[c] as u32;
                }
            }
        });

    // Vertical pass: tmp -> out. One running sum per column, moved down a row at a time, so
    // memory is read row by row. Bands of rows run in parallel; each starts from its own sums.
    let row_at = |y: isize| -> &[u8] {
        let y = y.clamp(0, height as isize - 1) as usize;
        &tmp[y * row_bytes..(y + 1) * row_bytes]
    };
    let band_rows = height.div_ceil(rayon::current_num_threads()).max(16);
    out.data
        .par_chunks_mut(band_rows * row_bytes)
        .enumerate()
        .for_each(|(band, chunk)| {
            let first = band * band_rows;
            let mut sums = vec![0u32; row_bytes];
            for k in -(radius as isize)..=radius as isize {
                for (s, v) in sums.iter_mut().zip(row_at(first as isize + k)) {
                    *s += *v as u32;
                }
            }
            for (i, dst) in chunk.chunks_exact_mut(row_bytes).enumerate() {
                let y = (first + i) as isize;
                for (d, s) in dst.iter_mut().zip(&sums) {
                    *d = ((*s + half) / window) as u8;
                }
                let add = row_at(y + radius as isize + 1);
                let sub = row_at(y - radius as isize);
                for ((s, a), b) in sums.iter_mut().zip(add).zip(sub) {
                    *s = *s + *a as u32 - *b as u32;
                }
            }
        });
    out
}

/// The mask as one byte per pixel of a `width` x `height` frame. A mask of a different size is
/// stretched with nearest-neighbour lookup. A mask that is empty or too short counts as "all
/// person" so a broken segmenter never blanks the picture.
fn fit_alpha(mask: &Mask, width: usize, height: usize) -> Cow<'_, [u8]> {
    let (mw, mh) = (mask.width as usize, mask.height as usize);
    if mw == 0 || mh == 0 || mask.alpha.len() < mw * mh {
        return Cow::Owned(vec![255; width * height]);
    }
    if (mw, mh) == (width, height) {
        return Cow::Borrowed(&mask.alpha[..width * height]);
    }
    let mut alpha = Vec::with_capacity(width * height);
    for y in 0..height {
        let row = &mask.alpha[(y * mh / height).min(mh - 1) * mw..][..mw];
        alpha.extend((0..width).map(|x| row[(x * mw / width).min(mw - 1)]));
    }
    Cow::Owned(alpha)
}

/// True if `frame` holds enough data for its stated size and stride.
fn has_pixels(frame: &VideoFrame) -> bool {
    frame.width > 0
        && frame.height > 0
        && frame.stride >= frame.width as usize * 4
        && frame.data.len() >= frame.stride * (frame.height as usize - 1) + frame.width as usize * 4
}

/// Blends `foreground` over `background` using `mask`: out = fg * a + bg * (1 - a).
///
/// Both frames should be the same size; the colour channels are blended and the foreground's
/// alpha channel is kept. If the background is smaller than the foreground, the foreground is
/// returned unchanged. This is the plain blend; it does not remove the old background from soft
/// edge pixels (see [`Compositor::replace_background`] for that).
pub fn blend(foreground: &VideoFrame, background: &VideoFrame, mask: &Mask) -> VideoFrame {
    let mut out = foreground.clone();
    if background.width != foreground.width
        || background.height != foreground.height
        || !has_pixels(foreground)
        || !has_pixels(background)
    {
        return out;
    }
    let (w, h) = (foreground.width as usize, foreground.height as usize);
    let alpha = fit_alpha(mask, w, h);
    out.data
        .par_chunks_mut(foreground.stride)
        .take(h)
        .zip(background.data.par_chunks(background.stride))
        .zip(alpha.par_chunks(w))
        .for_each(|((fg, bg), alpha)| {
            for ((fg, bg), &a) in fg.chunks_exact_mut(4).zip(bg.chunks_exact(4)).zip(alpha) {
                if a == 255 {
                    continue;
                }
                let (a, b) = (a as u32, 255 - a as u32);
                for c in 0..3 {
                    fg[c] = ((fg[c] as u32 * a + bg[c] as u32 * b + 127) / 255) as u8;
                }
            }
        });
    out
}

/// True when every pixel of the mask is fully person, so no background work is needed.
pub fn is_all_foreground(mask: &Mask) -> bool {
    mask.alpha.iter().all(|&a| a == 255)
}

/// Blur radii below this are computed per pixel; from here on, on 2x2 pixel cells (see
/// [`Compositor`]). A box blur this wide hides the half-resolution grid.
const FULL_RESOLUTION_BELOW: usize = 8;
/// Radius in pixels of the background estimate used to clean soft edge pixels.
const EDGE_RADIUS: usize = 8;
/// Edge pixels with less person than this (of 255) take the new background as it is; the
/// person's colour cannot be recovered reliably from such a faint trace. 10/255 is about 4%.
const MIN_RECOVER_ALPHA: u8 = 10;
/// Weight of the plain, unweighted blur mixed into the background blur. It only matters where
/// the window holds almost no background (deep inside the person) and keeps the division safe.
const PLAIN_BLEND: f32 = 1.0 / 1024.0;

/// How strongly a pixel with mask value `alpha` counts as background in the blurs: 255 for a pure
/// background pixel, falling to 0 for a pure person pixel. The weight is `(1 - a)^POWER`, scaled
/// to 0..=255 (so window sums fit in 32 bits).
///
/// A linear weight (power 1) lets soft edge pixels pull person colour into the estimate: a
/// pixel that is 10% person is 10% off and still counts for 90%. A steep weight trusts only
/// nearly clean pixels and falls back on the rest where nothing cleaner is near. On the
/// synthetic test person the mean error of a recovered edge pixel was 3.2 levels with a linear
/// weight and 1.4 from power 6 up, and the halo test needs power 8 or more at radius 4.
const WEIGHT_POWER: u32 = 12;

/// The background weight of every mask value, see `WEIGHT_POWER`.
fn background_weights() -> &'static [u8; 256] {
    static TABLE: OnceLock<[u8; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u8; 256];
        for (alpha, weight) in table.iter_mut().enumerate() {
            let rest = (255 - alpha) as f64 / 255.0;
            *weight = (255.0 * rest.powi(WEIGHT_POWER as i32)).round() as u8;
        }
        table
    })
}

/// Four sums per cell: red-green-blue-weight, or colour-count for the plain blur.
type Lanes = [u32; 4];

#[inline(always)]
fn add_lanes(a: Lanes, b: Lanes) -> Lanes {
    [
        a[0].wrapping_add(b[0]),
        a[1].wrapping_add(b[1]),
        a[2].wrapping_add(b[2]),
        a[3].wrapping_add(b[3]),
    ]
}

/// Summed-area table over cells of `1 << shift` pixels: any rectangle of cells can be summed
/// with four lookups, whatever its size. The sums wrap around instead of overflowing; a window
/// sum is exact as long as the true window sum fits in 32 bits, which holds for every radius
/// this module allows.
#[derive(Default)]
struct SummedAreas {
    /// Cells per row and number of rows.
    cells_x: usize,
    cells_y: usize,
    /// Cell size is `1 << shift` pixels.
    shift: u32,
    /// `(cells_x + 1) * (cells_y + 1)` entries; row 0 and column 0 are zero.
    table: Vec<Lanes>,
}

impl SummedAreas {
    /// Fills the table from `frame`. With `WEIGHTED` the lanes are `colour * w` and `w`, where
    /// `w` is the background weight of the pixel's mask value; without, they are the colour and
    /// a pixel count.
    ///
    /// Cell rows are built in parallel bands (running sums along each row and down the band),
    /// then each band gets the total of the bands above it added.
    fn build<const WEIGHTED: bool>(&mut self, frame: &VideoFrame, alpha: &[u8], shift: u32) {
        let (w, h) = (frame.width as usize, frame.height as usize);
        let weights = background_weights();
        let cell = 1usize << shift;
        let (cells_x, cells_y) = (w.div_ceil(cell), h.div_ceil(cell));
        let stride = cells_x + 1;
        if (self.cells_x, self.cells_y) != (cells_x, cells_y) {
            self.table.clear();
            self.table.resize(stride * (cells_y + 1), [0; 4]);
        }
        (self.cells_x, self.cells_y, self.shift) = (cells_x, cells_y, shift);

        let band_rows = cells_y.div_ceil(2 * rayon::current_num_threads()).max(4);
        let body = &mut self.table[stride..];
        body.par_chunks_mut(band_rows * stride)
            .enumerate()
            .for_each_init(
                || vec![[0u32; 4]; cells_x],
                |cells, (band, rows)| {
                    for i in 0..rows.len() / stride {
                        let cy = band * band_rows + i;
                        // Sum the pixels of each cell of this cell row.
                        cells.fill([0; 4]);
                        for y in cy * cell..((cy + 1) * cell).min(h) {
                            let line = &frame.data[y * frame.stride..][..w * 4];
                            let line_alpha = &alpha[y * w..][..w];
                            for (x, (px, &a)) in line.chunks_exact(4).zip(line_alpha).enumerate() {
                                let lanes = if WEIGHTED {
                                    let weight = weights[a as usize] as u32;
                                    [
                                        px[0] as u32 * weight,
                                        px[1] as u32 * weight,
                                        px[2] as u32 * weight,
                                        weight,
                                    ]
                                } else {
                                    [px[0] as u32, px[1] as u32, px[2] as u32, 1]
                                };
                                cells[x >> shift] = add_lanes(cells[x >> shift], lanes);
                            }
                        }
                        // Running sum along the row, plus the finished row above.
                        let (above, current) = rows.split_at_mut(i * stride);
                        let row = &mut current[..stride];
                        row[0] = [0; 4];
                        let mut run = [0u32; 4];
                        for (r, &c) in row[1..].iter_mut().zip(cells.iter()) {
                            run = add_lanes(run, c);
                            *r = run;
                        }
                        if i > 0 {
                            for (r, &p) in row.iter_mut().zip(&above[(i - 1) * stride..]) {
                                *r = add_lanes(*r, p);
                            }
                        }
                    }
                },
            );

        // Offsets: what lies above each band is the sum of the last rows of the bands before it.
        let band_len = band_rows * stride;
        let bands = body.len().div_ceil(band_len);
        if bands > 1 {
            let mut offsets: Vec<Vec<Lanes>> = vec![vec![[0; 4]; stride]];
            for b in 0..bands - 1 {
                let last = &body[(b + 1) * band_len - stride..(b + 1) * band_len];
                let previous = &offsets[b];
                let next = last
                    .iter()
                    .zip(previous)
                    .map(|(&l, &p)| add_lanes(l, p))
                    .collect();
                offsets.push(next);
            }
            body.par_chunks_mut(band_len)
                .enumerate()
                .skip(1)
                .for_each(|(band, rows)| {
                    for row in rows.chunks_exact_mut(stride) {
                        for (r, &o) in row.iter_mut().zip(&offsets[band]) {
                            *r = add_lanes(*r, o);
                        }
                    }
                });
        }
    }

    /// Sum over the cells `x0..x1` by `y0..y1`.
    #[inline(always)]
    fn window(&self, x0: usize, x1: usize, y0: usize, y1: usize) -> Lanes {
        let stride = self.cells_x + 1;
        let at = |x: usize, y: usize| self.table[y * stride + x];
        let (a, b, c, d) = (at(x1, y1), at(x0, y1), at(x1, y0), at(x0, y0));
        let mut sum = [0u32; 4];
        for k in 0..4 {
            sum[k] = a[k]
                .wrapping_sub(b[k])
                .wrapping_sub(c[k])
                .wrapping_add(d[k]);
        }
        sum
    }

    /// Cells within `radius` of cell `(cx, cy)`, cut off at the frame edge.
    #[inline(always)]
    fn around(&self, cx: usize, cy: usize, radius: usize) -> Lanes {
        self.window(
            cx.saturating_sub(radius),
            (cx + radius + 1).min(self.cells_x),
            cy.saturating_sub(radius),
            (cy + radius + 1).min(self.cells_y),
        )
    }

    /// Weighted mean colour (blue, green, red) of the background around pixel `(x, y)` within
    /// `radius_px`, or `None` if the window shows no background at all.
    fn background_colour(&self, x: usize, y: usize, radius_px: usize) -> Option<[f32; 3]> {
        let radius = radius_px.div_ceil(1 << self.shift);
        let sums = self.around(x >> self.shift, y >> self.shift, radius);
        (sums[3] != 0).then(|| {
            let scale = 1.0 / sums[3] as f32;
            [
                sums[0] as f32 * scale,
                sums[1] as f32 * scale,
                sums[2] as f32 * scale,
            ]
        })
    }
}

/// For each of `n` pixels: the two neighbouring cells to blend and the weight of the second,
/// so that cell values spread smoothly over the pixels they cover.
fn cell_blend_map(n: usize, cells: usize, shift: u32) -> Vec<(u32, u32, f32)> {
    let cell = (1usize << shift) as f32;
    (0..n)
        .map(|i| {
            let pos = ((i as f32 + 0.5) / cell - 0.5).clamp(0.0, (cells - 1) as f32);
            let first = pos.floor();
            (
                first as u32,
                (first as usize + 1).min(cells - 1) as u32,
                pos - first,
            )
        })
        .collect()
}

/// Composites a person over a blurred or replaced background and removes the old background
/// from soft edge pixels. It keeps its working memory between frames, so keep one around.
///
/// How it works, in short:
///
/// * Summed-area tables of the frame, weighted by how much background each pixel shows, give
///   the sum over any window in four lookups, so the cost does not depend on the blur radius.
/// * For a blur of radius 8 or more the tables use 2x2 pixel cells (a quarter of the work and
///   memory). Each cell holds the exact weighted sums of its four pixels, so the mask weighting
///   stays pixel-accurate; only the window position is rounded to a cell (at most one pixel).
///   The blurred sums are then spread back over the pixels with bilinear interpolation of the
///   sums themselves (not of the colours), which keeps the weighting correct. Smaller radii use
///   one pixel per cell.
/// * Rows are processed in parallel.
#[derive(Default)]
pub struct Compositor {
    /// Sums of `colour * background weight` and of the weight.
    weighted: SummedAreas,
    /// Sums of the plain colour and of the pixel count.
    plain: SummedAreas,
    /// Blurred sums per cell: weighted colour (3) and weight, already mixed with the plain blur.
    blurred: Vec<[f32; 4]>,
}

impl Compositor {
    /// Creates a compositor with no memory reserved yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Blurs the background of `frame` (radius clamped to 1..=64) and keeps the person sharp.
    ///
    /// The blur uses only background pixels, weighted by `(255 - alpha) / 255`, so the person's
    /// colours do not leak into the background. Soft edge pixels are cleaned of their old
    /// background before they are laid over the blur. The frame is edited in place; its size,
    /// stride and alpha channel are kept. A frame without pixel data is returned unchanged.
    pub fn blur_background(
        &mut self,
        mut frame: VideoFrame,
        mask: &Mask,
        radius: u32,
    ) -> VideoFrame {
        if !has_pixels(&frame) {
            return frame;
        }
        let radius = radius.clamp(MIN_BLUR_RADIUS, MAX_BLUR_RADIUS) as usize;
        let (w, h) = (frame.width as usize, frame.height as usize);
        let alpha = fit_alpha(mask, w, h);
        let shift = u32::from(radius >= FULL_RESOLUTION_BELOW);
        self.weighted.build::<true>(&frame, &alpha, shift);
        self.plain.build::<false>(&frame, &alpha, shift);
        self.blur_cells(radius);

        let (cells_x, cells_y) = (self.weighted.cells_x, self.weighted.cells_y);
        let columns = cell_blend_map(w, cells_x, shift);
        let rows = cell_blend_map(h, cells_y, shift);
        let (weighted, blurred) = (&self.weighted, &self.blurred);
        let stride = frame.stride;
        frame
            .data
            .par_chunks_mut(stride)
            .take(h)
            .zip(alpha.par_chunks(w))
            .enumerate()
            .for_each_init(
                || vec![[0f32; 4]; cells_x],
                |line, (y, (row, alpha))| {
                    if alpha.iter().all(|&a| a == 255) {
                        return;
                    }
                    // Blend the two cell rows that cover this pixel row.
                    let (r0, r1, fy) = rows[y];
                    let top = &blurred[r0 as usize * cells_x..][..cells_x];
                    let bottom = &blurred[r1 as usize * cells_x..][..cells_x];
                    for ((l, t), b) in line.iter_mut().zip(top).zip(bottom) {
                        for k in 0..4 {
                            l[k] = t[k] + (b[k] - t[k]) * fy;
                        }
                    }
                    for (x, (px, &a)) in row.chunks_exact_mut(4).zip(alpha).enumerate() {
                        if a == 255 {
                            continue;
                        }
                        let (c0, c1, fx) = columns[x];
                        let (l0, l1) = (line[c0 as usize], line[c1 as usize]);
                        let mut v = [0f32; 4];
                        for k in 0..4 {
                            v[k] = l0[k] + (l1[k] - l0[k]) * fx;
                        }
                        let scale = 1.0 / v[3];
                        let blurred = [v[0] * scale, v[1] * scale, v[2] * scale];
                        write_pixel(px, a, blurred, weighted, x, y);
                    }
                },
            );
        frame
    }

    /// Puts the person of `frame` over `background` (same size as `frame`), removing the old
    /// background from soft edge pixels so no outline of it remains. The frame is edited in
    /// place; its stride and alpha channel are kept. If the sizes differ, or either frame has
    /// no pixel data, the frame is returned unchanged.
    pub fn replace_background(
        &mut self,
        mut frame: VideoFrame,
        background: &VideoFrame,
        mask: &Mask,
    ) -> VideoFrame {
        if (background.width, background.height) != (frame.width, frame.height)
            || !has_pixels(&frame)
            || !has_pixels(background)
        {
            return frame;
        }
        let (w, h) = (frame.width as usize, frame.height as usize);
        let alpha = fit_alpha(mask, w, h);
        self.weighted.build::<true>(&frame, &alpha, 1);

        let weighted = &self.weighted;
        let stride = frame.stride;
        frame
            .data
            .par_chunks_mut(stride)
            .take(h)
            .zip(background.data.par_chunks(background.stride))
            .zip(alpha.par_chunks(w))
            .enumerate()
            .for_each(|(y, ((row, new_row), alpha))| {
                for (x, ((px, new), &a)) in row
                    .chunks_exact_mut(4)
                    .zip(new_row.chunks_exact(4))
                    .zip(alpha)
                    .enumerate()
                {
                    if a == 255 {
                        continue;
                    }
                    let new = [new[0] as f32, new[1] as f32, new[2] as f32];
                    write_pixel(px, a, new, weighted, x, y);
                }
            });
        frame
    }

    /// Computes the blurred sums of every cell: the weighted window sums plus a small share of
    /// the plain ones. `radius` is in pixels.
    fn blur_cells(&mut self, radius: usize) {
        let cell = 1usize << self.weighted.shift;
        let radius_cells = ((radius + cell / 2) / cell).max(1);
        let cells_x = self.weighted.cells_x;
        self.blurred
            .resize(cells_x * self.weighted.cells_y, [0.0; 4]);
        let (weighted, plain) = (&self.weighted, &self.plain);
        self.blurred
            .par_chunks_mut(cells_x)
            .enumerate()
            .for_each(|(cy, out)| {
                for (cx, o) in out.iter_mut().enumerate() {
                    let wsum = weighted.around(cx, cy, radius_cells);
                    let psum = plain.around(cx, cy, radius_cells);
                    for k in 0..3 {
                        o[k] = wsum[k] as f32 * (1.0 / 255.0) + psum[k] as f32 * PLAIN_BLEND;
                    }
                    o[3] = wsum[3] as f32 * (1.0 / 255.0) + psum[3] as f32 * PLAIN_BLEND;
                }
            });
    }
}

/// Writes one non-person pixel `px` (BGRA) with person amount `a` (0..=254): `new_bg` is what
/// should show behind the person. Pure background pixels take it as is. Soft edge pixels first
/// get the old background taken out: with `B` the old background around the pixel and `a` the
/// person amount, the person's own colour is `F = (pixel - (1 - a) * B) / a` and the result is
/// `F * a + new_bg * (1 - a)`.
#[inline(always)]
fn write_pixel(px: &mut [u8], a: u8, new_bg: [f32; 3], weighted: &SummedAreas, x: usize, y: usize) {
    let result = if a < MIN_RECOVER_ALPHA {
        new_bg
    } else {
        let old = weighted.background_colour(x, y, EDGE_RADIUS);
        let person = a as f32 * (1.0 / 255.0);
        let rest = 1.0 - person;
        let mut out = [0f32; 3];
        for c in 0..3 {
            let seen = px[c] as f32;
            let colour = match old {
                Some(old) => ((seen - rest * old[c]) / person).clamp(0.0, 255.0),
                None => seen,
            };
            out[c] = colour * person + new_bg[c] * rest;
        }
        out
    };
    for c in 0..3 {
        px[c] = (result[c] + 0.5) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, px: [u8; 4]) -> VideoFrame {
        let mut f = VideoFrame::new_bgra(w, h, 5);
        for y in 0..h {
            for x in 0..w {
                f.set_pixel(x, y, px);
            }
        }
        f
    }

    #[test]
    fn blur_keeps_flat_colour() {
        let frame = solid(9, 7, [40, 80, 120, 255]);
        for radius in [0, 1, 3, 200] {
            let out = box_blur(&frame, radius);
            assert_eq!(out.data, frame.data, "radius {radius}");
        }
    }

    #[test]
    fn blur_averages_a_neighbourhood() {
        // A single white pixel in a black row spreads out over 2r+1 pixels.
        let mut frame = solid(11, 1, [0, 0, 0, 255]);
        frame.set_pixel(5, 0, [255, 255, 255, 255]);
        let out = box_blur(&frame, 1);
        assert_eq!(out.pixel(4, 0)[0], 85);
        assert_eq!(out.pixel(5, 0)[0], 85);
        assert_eq!(out.pixel(6, 0)[0], 85);
        assert_eq!(out.pixel(3, 0)[0], 0);
        assert_eq!(out.pixel(7, 0)[0], 0);
    }

    #[test]
    fn blur_clamps_at_the_edges() {
        // Edge pixels are repeated, so a bright left edge stays bright near the border.
        let mut frame = solid(6, 1, [0, 0, 0, 255]);
        frame.set_pixel(0, 0, [255, 255, 255, 255]);
        let out = box_blur(&frame, 1);
        assert_eq!(out.pixel(0, 0)[0], 170);
    }

    #[test]
    fn blur_handles_frames_smaller_than_the_radius() {
        let frame = solid(2, 2, [9, 9, 9, 255]);
        assert_eq!(box_blur(&frame, 64).data, frame.data);
        let empty = VideoFrame::new_bgra(0, 0, 0);
        assert!(box_blur(&empty, 5).data.is_empty());
    }

    #[test]
    fn blend_follows_the_mask() {
        let fg = solid(2, 1, [200, 0, 0, 255]);
        let bg = solid(2, 1, [0, 100, 0, 255]);
        let mask = Mask {
            width: 2,
            height: 1,
            alpha: vec![255, 0],
        };
        let out = blend(&fg, &bg, &mask);
        assert_eq!(out.pixel(0, 0), [200, 0, 0, 255]);
        assert_eq!(out.pixel(1, 0), [0, 100, 0, 255]);

        let half = Mask {
            width: 1,
            height: 1,
            alpha: vec![128],
        };
        let out = blend(&fg, &bg, &half);
        assert_eq!(out.pixel(0, 0), [100, 50, 0, 255]);
    }

    #[test]
    fn blend_survives_bad_masks() {
        let fg = solid(4, 4, [1, 2, 3, 255]);
        let bg = solid(4, 4, [9, 9, 9, 255]);
        let short = Mask {
            width: 4,
            height: 4,
            alpha: vec![0; 3],
        };
        assert_eq!(blend(&fg, &bg, &short), fg);
        let empty = Mask {
            width: 0,
            height: 0,
            alpha: Vec::new(),
        };
        assert_eq!(blend(&fg, &bg, &empty), fg);
        // A half-size mask is stretched.
        let small = Mask {
            width: 2,
            height: 2,
            alpha: vec![0; 4],
        };
        assert_eq!(blend(&fg, &bg, &small).pixel(3, 3), [9, 9, 9, 255]);
    }

    #[test]
    fn all_foreground_check() {
        assert!(is_all_foreground(&Mask::full(3, 3)));
        let mut mask = Mask::full(3, 3);
        mask.alpha[4] = 0;
        assert!(!is_all_foreground(&mask));
    }

    /// Mask with a soft-edged disc: 255 inside `radius`, 0 beyond `radius + ramp`.
    fn disc_mask(w: u32, h: u32, (cx, cy): (f32, f32), radius: f32, ramp: f32) -> Mask {
        let mut alpha = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                alpha.push(
                    ((radius + ramp - d) / ramp * 255.0)
                        .clamp(0.0, 255.0)
                        .round() as u8,
                );
            }
        }
        Mask {
            width: w,
            height: h,
            alpha,
        }
    }

    /// Smooth background: a gentle gradient (BGRA).
    fn gradient(w: u32, h: u32, base: [u8; 3], slope: f32) -> VideoFrame {
        let mut f = VideoFrame::new_bgra(w, h, 0);
        for y in 0..h {
            for x in 0..w {
                let t = (x + y) as f32 * slope;
                let c = |v: u8| (v as f32 + t).clamp(0.0, 255.0).round() as u8;
                f.set_pixel(x, y, [c(base[0]), c(base[1]), c(base[2]), 255]);
            }
        }
        f
    }

    /// `person` laid over `background` with the mask, as a camera would see it.
    fn lay_over(background: &VideoFrame, person: [u8; 3], mask: &Mask) -> VideoFrame {
        let mut f = background.clone();
        for y in 0..f.height {
            for x in 0..f.width {
                let a = mask.get(x, y) as f32 / 255.0;
                let mut px = f.pixel(x, y);
                for c in 0..3 {
                    px[c] = (person[c] as f32 * a + px[c] as f32 * (1.0 - a)).round() as u8;
                }
                f.set_pixel(x, y, px);
            }
        }
        f
    }

    /// Chebyshev distance from `(x, y)` to the nearest pixel with a non-zero mask value.
    fn distance_to_person(mask: &Mask, x: i32, y: i32) -> i32 {
        let mut best = i32::MAX;
        for py in 0..mask.height as i32 {
            for px in 0..mask.width as i32 {
                if mask.get(px as u32, py as u32) > 0 {
                    best = best.min((px - x).abs().max((py - y).abs()));
                }
            }
        }
        best
    }

    #[test]
    fn blurred_background_has_no_halo_from_the_person() {
        let (w, h) = (96, 72);
        let scene = gradient(w, h, [90, 110, 130], 0.15);
        let mask = disc_mask(w, h, (48.0, 36.0), 12.0, 6.0);
        // A bright person over the scene.
        let frame = lay_over(&scene, [250, 240, 235], &mask);
        for radius in [4, 8, 16] {
            let ideal = box_blur(&scene, radius);
            let plain = box_blur(&frame, radius);
            let out = Compositor::new().blur_background(frame.clone(), &mask, radius);
            let (mut worst, mut worst_plain, mut checked) = (0u8, 0u8, 0);
            for y in 0..h {
                for x in 0..w {
                    let near = distance_to_person(&mask, x as i32, y as i32);
                    let margin = radius + 2;
                    if mask.get(x, y) != 0
                        || !(1..=8).contains(&near)
                        || x < margin
                        || y < margin
                        || x + margin >= w
                        || y + margin >= h
                    {
                        continue;
                    }
                    checked += 1;
                    for c in 0..3 {
                        worst = worst.max(out.pixel(x, y)[c].abs_diff(ideal.pixel(x, y)[c]));
                        worst_plain =
                            worst_plain.max(plain.pixel(x, y)[c].abs_diff(ideal.pixel(x, y)[c]));
                    }
                }
            }
            assert!(
                checked > 100,
                "radius {radius}: only {checked} pixels checked"
            );
            assert!(worst <= 3, "radius {radius}: halo of {worst} levels");
            // The test has teeth: blurring the whole frame does leave a halo.
            assert!(
                worst_plain > 8,
                "radius {radius}: plain blur only off by {worst_plain}"
            );
        }
    }

    #[test]
    fn replaced_edge_pixels_carry_no_trace_of_the_old_background() {
        let (w, h) = (96, 72);
        let old = gradient(w, h, [200, 120, 60], 0.1);
        let new = gradient(w, h, [60, 150, 100], -0.1);
        let person = [40, 70, 160];
        let mask = disc_mask(w, h, (48.0, 36.0), 16.0, 6.0);
        let frame = lay_over(&old, person, &mask);

        let out = Compositor::new().replace_background(frame.clone(), &new, &mask);
        let plain = blend(&frame, &new, &mask);
        let (mut worst, mut worst_plain, mut edge) = (0.0f32, 0.0f32, 0);
        for y in 0..h {
            for x in 0..w {
                let a = mask.get(x, y);
                if a == 0 || a == 255 {
                    continue;
                }
                edge += 1;
                let a = a as f32 / 255.0;
                for (c, &p) in person.iter().enumerate() {
                    let ideal = p as f32 * a + new.pixel(x, y)[c] as f32 * (1.0 - a);
                    worst = worst.max((out.pixel(x, y)[c] as f32 - ideal).abs());
                    worst_plain = worst_plain.max((plain.pixel(x, y)[c] as f32 - ideal).abs());
                }
            }
        }
        assert!(edge > 200, "only {edge} edge pixels");
        assert!(worst <= 4.0, "edge pixel off by {worst} levels");
        // The test has teeth: a plain blend leaves the old background in the outline.
        assert!(worst_plain > 12.0, "plain blend only off by {worst_plain}");
    }

    #[test]
    fn blur_leaves_person_pixels_and_alpha_alone() {
        let (w, h) = (40, 30);
        let scene = gradient(w, h, [10, 50, 90], 1.0);
        let mut frame = lay_over(&scene, [200, 100, 50], &Mask::full(w, h));
        for px in frame.data.chunks_exact_mut(4) {
            px[3] = 77;
        }
        let mask = disc_mask(w, h, (20.0, 15.0), 6.0, 4.0);
        let out = Compositor::new().blur_background(frame.clone(), &mask, 8);
        for y in 0..h {
            for x in 0..w {
                assert_eq!(out.pixel(x, y)[3], 77);
                if mask.get(x, y) == 255 {
                    assert_eq!(out.pixel(x, y), frame.pixel(x, y));
                }
            }
        }
    }

    #[test]
    fn compositing_a_flat_scene_keeps_it_flat() {
        // Odd sizes, tiny frames, every radius: no panic, and nothing changes colour.
        for (w, h) in [(1, 1), (2, 3), (5, 3), (33, 17)] {
            let flat = solid(w, h, [60, 120, 180, 255]);
            let mut mask = Mask::full(w, h);
            for (i, a) in mask.alpha.iter_mut().enumerate() {
                *a = [0, 77, 255, 200, 0, 12][i % 6];
            }
            for radius in [1, 7, 8, 64] {
                let out = Compositor::new().blur_background(flat.clone(), &mask, radius);
                assert_eq!(out.data, flat.data, "{w}x{h} radius {radius}");
            }
            let out = Compositor::new().replace_background(flat.clone(), &flat, &mask);
            assert_eq!(out.data, flat.data, "{w}x{h} replace");
        }
    }

    #[test]
    fn compositing_copes_with_almost_all_person_and_bad_masks() {
        let frame = gradient(30, 20, [50, 60, 70], 2.0);
        // Nearly everything is person: the background weights are almost zero.
        let faint = Mask {
            width: 30,
            height: 20,
            alpha: vec![254; 600],
        };
        let out = Compositor::new().blur_background(frame.clone(), &faint, 16);
        for (a, b) in out.data.iter().zip(&frame.data) {
            assert!(a.abs_diff(*b) <= 3);
        }
        // A short or empty mask counts as all person; a half-size mask is stretched.
        let short = Mask {
            width: 30,
            height: 20,
            alpha: vec![0; 5],
        };
        assert_eq!(
            Compositor::new().blur_background(frame.clone(), &short, 8),
            frame
        );
        let half = Mask {
            width: 15,
            height: 10,
            alpha: vec![0; 150],
        };
        let out = Compositor::new().blur_background(frame.clone(), &half, 8);
        assert_ne!(out.data, frame.data);
        // A background of another size leaves the frame alone.
        let other = solid(4, 4, [0, 0, 0, 255]);
        assert_eq!(
            Compositor::new().replace_background(frame.clone(), &other, &half),
            frame
        );
    }

    #[test]
    fn compositor_can_be_reused_across_frame_sizes() {
        let mut compositor = Compositor::new();
        for (w, h) in [(40, 30), (20, 10), (40, 30)] {
            let frame = gradient(w, h, [10, 20, 30], 1.0);
            let mask = disc_mask(w, h, (w as f32 / 2.0, h as f32 / 2.0), 4.0, 3.0);
            let fresh = Compositor::new().blur_background(frame.clone(), &mask, 8);
            assert_eq!(compositor.blur_background(frame, &mask, 8), fresh);
        }
    }

    #[test]
    fn blend_and_box_blur_ignore_padding_and_short_data() {
        // A frame with row padding blurs like one without.
        let tight = gradient(6, 4, [0, 50, 100], 5.0);
        let mut padded = VideoFrame::new_bgra(6, 4, 0);
        padded.stride = 32;
        padded.data = vec![9; 32 * 4];
        for y in 0..4 {
            for x in 0..6 {
                padded.set_pixel(x, y, tight.pixel(x, y));
            }
        }
        assert_eq!(box_blur(&padded, 2).data, box_blur(&tight, 2).data);
        let mut broken = tight.clone();
        broken.data.truncate(10);
        assert!(box_blur(&broken, 2).data.iter().all(|&b| b == 0));
        assert_eq!(blend(&broken, &broken, &Mask::full(6, 4)), broken);
    }
}
