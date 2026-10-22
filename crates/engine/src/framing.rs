//! Framing maths: which part of the camera image to show, and how to scale it.

use rayon::prelude::*;
use studio_core::frame::VideoFrame;
use studio_core::settings::Framing;

/// A rectangle inside the input image, in input pixels. Fractions are allowed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CropRect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

/// Works out the part of the input image to show.
///
/// The rectangle has the output's aspect ratio and is as large as fits in the input, divided by
/// the zoom. The pan values slide it inside the leftover margin: -1 puts it against the left or
/// top edge, +1 against the right or bottom edge, 0 centres it. The framing is clamped first (see
/// [`Framing::clamped`]), so out-of-range values are safe. The rectangle never leaves the input
/// (apart from tiny floating point rounding).
///
/// If the output size is zero, the input's own aspect ratio is used instead.
pub fn crop_rect(
    input_w: u32,
    input_h: u32,
    out_w: u32,
    out_h: u32,
    framing: &Framing,
) -> CropRect {
    let iw = input_w as f64;
    let ih = input_h as f64;
    if input_w == 0 || input_h == 0 {
        return CropRect {
            x: 0.0,
            y: 0.0,
            width: iw as f32,
            height: ih as f32,
        };
    }
    let framing = framing.clamped();
    let aspect = if out_w == 0 || out_h == 0 {
        iw / ih
    } else {
        out_w as f64 / out_h as f64
    };

    // Largest rectangle with the output aspect that fits in the input.
    let (max_w, max_h) = if iw / ih > aspect {
        (ih * aspect, ih)
    } else {
        (iw, iw / aspect)
    };
    let zoom = framing.zoom as f64;
    let width = ((max_w / zoom) as f32).min(input_w as f32);
    let height = ((max_h / zoom) as f32).min(input_h as f32);

    let margin_x = (input_w as f32 - width).max(0.0);
    let margin_y = (input_h as f32 - height).max(0.0);
    let x = (margin_x * (framing.pan_x + 1.0) * 0.5).clamp(0.0, margin_x);
    let y = (margin_y * (framing.pan_y + 1.0) * 0.5).clamp(0.0, margin_y);
    CropRect {
        x,
        y,
        width,
        height,
    }
}

/// Scale factors at or below this count as "not shrinking": the bilinear path is used, so
/// floating point noise around 1.0 never turns an exact copy into a blur.
const SHRINK_EPSILON: f64 = 1e-4;

/// How one axis of the output is built from the source: for each output position, a run of
/// consecutive source indexes and one weight per index. The weights of a run add up to 1.
struct AxisPlan {
    /// First source index of each output position's run.
    first: Vec<u32>,
    /// Where each run's weights start in `weights`; one extra entry marks the end.
    begin: Vec<u32>,
    /// All weights, run after run.
    weights: Vec<f32>,
}

impl AxisPlan {
    /// Weights of output position `o`.
    fn taps(&self, o: usize) -> &[f32] {
        &self.weights[self.begin[o] as usize..self.begin[o + 1] as usize]
    }

    /// The range of source indexes used by any output position.
    fn source_span(&self) -> (usize, usize) {
        let mut lo = usize::MAX;
        let mut hi = 0;
        for o in 0..self.first.len() {
            let first = self.first[o] as usize;
            lo = lo.min(first);
            hi = hi.max(first + self.taps(o).len());
        }
        (lo, hi)
    }
}

/// Plans one axis: `length` source pixels starting at `start` become `out_n` output pixels.
///
/// When shrinking (more than one source pixel per output pixel) each output pixel is the
/// average of the source area it covers, with fractional weights at both ends, so thin lines
/// keep their weight instead of falling between sample points. Otherwise it is a two-tap
/// bilinear sample at the middle of the output pixel. `flip` reverses the output order.
fn axis_plan(start: f32, length: f32, out_n: u32, src_n: u32, flip: bool) -> AxisPlan {
    let n = out_n as usize;
    let mut plan = AxisPlan {
        first: Vec::with_capacity(n),
        begin: Vec::with_capacity(n + 1),
        weights: Vec::with_capacity(n * 3),
    };
    let src_len = src_n as f64;
    let last = src_len - 1.0;
    let scale = length as f64 / out_n as f64;
    for o in 0..n {
        let o = if flip { n - 1 - o } else { o };
        plan.begin.push(plan.weights.len() as u32);
        if scale > 1.0 + SHRINK_EPSILON {
            // Footprint of the output pixel in source coordinates (pixel i covers [i, i + 1)).
            let lo = (start as f64 + o as f64 * scale).clamp(0.0, src_len);
            let hi = (start as f64 + (o + 1) as f64 * scale).clamp(0.0, src_len);
            let i0 = (lo.floor() as usize).min(src_n as usize - 1);
            let i1 = (hi.ceil() as usize).clamp(i0 + 1, src_n as usize);
            let mut total = 0.0;
            let at = plan.weights.len();
            for i in i0..i1 {
                let overlap = (hi.min(i as f64 + 1.0) - lo.max(i as f64)).max(0.0);
                plan.weights.push(overlap as f32);
                total += overlap;
            }
            if total <= 1e-9 {
                plan.weights.truncate(at);
                plan.weights.push(1.0);
            } else {
                for w in &mut plan.weights[at..] {
                    *w = (*w as f64 / total) as f32;
                }
            }
            plan.first.push(i0 as u32);
        } else {
            // Sample at the middle of the output pixel, mapped back into the source.
            let pos = (start as f64 + (o as f64 + 0.5) * scale - 0.5).clamp(0.0, last);
            let i0 = pos.floor();
            let frac = pos - i0;
            plan.first.push(i0 as u32);
            if frac < 1e-6 || i0 + 1.0 > last {
                plan.weights.push(1.0);
            } else {
                plan.weights.push((1.0 - frac) as f32);
                plan.weights.push(frac as f32);
            }
        }
    }
    plan.begin.push(plan.weights.len() as u32);
    plan
}

/// If `rect` is a whole-pixel window of exactly `out_w` x `out_h` pixels that lies inside
/// `src`, returns its top-left corner. Such a crop needs no resampling at all.
fn whole_pixel_crop(
    src: &VideoFrame,
    rect: &CropRect,
    out_w: u32,
    out_h: u32,
) -> Option<(usize, usize)> {
    const TOLERANCE: f32 = 1e-3;
    let near = |v: f32, want: f32| (v - want).abs() <= TOLERANCE;
    let (x, y) = (rect.x.round(), rect.y.round());
    let fits = near(rect.width, out_w as f32)
        && near(rect.height, out_h as f32)
        && near(rect.x, x)
        && near(rect.y, y)
        && x >= 0.0
        && y >= 0.0
        && x as usize + out_w as usize <= src.width as usize
        && y as usize + out_h as usize <= src.height as usize;
    fits.then_some((x as usize, y as usize))
}

/// Exact path for [`scale_frame`]: copies a whole-pixel window row by row, mirrored if asked.
fn copy_window(src: &VideoFrame, (x0, y0): (usize, usize), out: &mut VideoFrame, mirror: bool) {
    let row_bytes = out.width as usize * 4;
    if row_bytes == 0 {
        return;
    }
    let out_stride = out.stride;
    out.data
        .par_chunks_mut(out_stride)
        .enumerate()
        .for_each(|(oy, dst)| {
            let from = (y0 + oy) * src.stride + x0 * 4;
            let line = &src.data[from..from + row_bytes];
            if mirror {
                for (d, s) in dst.chunks_exact_mut(4).zip(line.chunks_exact(4).rev()) {
                    d.copy_from_slice(s);
                }
            } else {
                dst[..row_bytes].copy_from_slice(line);
            }
        });
}

/// Copies the `rect` part of `src` into a new `out_w` x `out_h` frame, optionally flipping it
/// left to right. The new frame has no row padding and keeps the source timestamp. A source
/// that is empty or has too little data gives a black frame.
///
/// A crop that is a whole-pixel window of exactly the output size is copied unchanged. Otherwise
/// shrinking uses area averaging and enlarging uses bilinear sampling (see [`scale_general`]).
pub(crate) fn scale_frame(
    src: &VideoFrame,
    rect: &CropRect,
    out_w: u32,
    out_h: u32,
    mirror: bool,
) -> VideoFrame {
    let mut out = VideoFrame::new_bgra(out_w, out_h, src.timestamp_us);
    if !usable_source(src) || out_w == 0 || out_h == 0 {
        return out;
    }
    match whole_pixel_crop(src, rect, out_w, out_h) {
        Some(corner) => copy_window(src, corner, &mut out, mirror),
        None => scale_general(src, rect, &mut out, mirror),
    }
    out
}

/// True if `src` is non-empty and its data covers every row.
fn usable_source(src: &VideoFrame) -> bool {
    if src.width == 0 || src.height == 0 {
        return false;
    }
    let needed = src.stride * (src.height as usize - 1) + src.width as usize * 4;
    src.data.len() >= needed
}

/// General resampling path of [`scale_frame`], also used by the tests to check the fast path.
///
/// Per output row: the source rows it needs are blended into one line (vertical weights), then
/// that line is blended into the output pixels (horizontal weights). Both steps use per-frame
/// weight tables and `f32` maths on whole BGRA pixels; output rows are processed in parallel.
pub(crate) fn scale_general(src: &VideoFrame, rect: &CropRect, out: &mut VideoFrame, mirror: bool) {
    let (out_w, out_h) = (out.width, out.height);
    let columns = axis_plan(rect.x, rect.width, out_w, src.width, mirror);
    let rows = axis_plan(rect.y, rect.height, out_h, src.height, false);
    let (col_lo, col_hi) = columns.source_span();
    let line_len = col_hi - col_lo;
    let out_stride = out.stride;
    let row_bytes = out_w as usize * 4;

    out.data
        .par_chunks_mut(out_stride)
        .enumerate()
        .for_each_init(
            || vec![[0f32; 4]; line_len],
            |line, (oy, dst)| {
                // Vertical step: blend the source rows into `line`.
                let first_row = rows.first[oy] as usize;
                for (t, &weight) in rows.taps(oy).iter().enumerate() {
                    let from = (first_row + t) * src.stride + col_lo * 4;
                    let source = src.data[from..from + line_len * 4].chunks_exact(4);
                    if t == 0 {
                        for (l, p) in line.iter_mut().zip(source) {
                            *l = [
                                p[0] as f32 * weight,
                                p[1] as f32 * weight,
                                p[2] as f32 * weight,
                                p[3] as f32 * weight,
                            ];
                        }
                    } else {
                        for (l, p) in line.iter_mut().zip(source) {
                            l[0] += p[0] as f32 * weight;
                            l[1] += p[1] as f32 * weight;
                            l[2] += p[2] as f32 * weight;
                            l[3] += p[3] as f32 * weight;
                        }
                    }
                }
                // Horizontal step: blend `line` into the output pixels.
                for (ox, d) in dst[..row_bytes].chunks_exact_mut(4).enumerate() {
                    let first = columns.first[ox] as usize - col_lo;
                    let mut acc = [0f32; 4];
                    for (l, &weight) in line[first..].iter().zip(columns.taps(ox)) {
                        acc[0] += l[0] * weight;
                        acc[1] += l[1] * weight;
                        acc[2] += l[2] * weight;
                        acc[3] += l[3] * weight;
                    }
                    for c in 0..4 {
                        d[c] = (acc[c] + 0.5) as u8;
                    }
                }
            },
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-3;

    fn framing(zoom: f32, pan_x: f32, pan_y: f32) -> Framing {
        Framing {
            zoom,
            pan_x,
            pan_y,
            ..Framing::default()
        }
    }

    fn inside(rect: &CropRect, w: u32, h: u32) -> bool {
        rect.x >= 0.0
            && rect.y >= 0.0
            && rect.width > 0.0
            && rect.height > 0.0
            && rect.x + rect.width <= w as f32 + EPS
            && rect.y + rect.height <= h as f32 + EPS
    }

    #[test]
    fn zoom_one_is_full_frame_when_aspect_matches() {
        let r = crop_rect(1920, 1080, 1280, 720, &Framing::default());
        assert_eq!(
            r,
            CropRect {
                x: 0.0,
                y: 0.0,
                width: 1920.0,
                height: 1080.0
            }
        );
    }

    #[test]
    fn zoom_one_is_centred_with_output_aspect() {
        // 4:3 input to 16:9 output: full width, 360 of 480 rows, centred.
        let r = crop_rect(640, 480, 1280, 720, &Framing::default());
        assert!((r.width - 640.0).abs() < EPS);
        assert!((r.height - 360.0).abs() < EPS);
        assert!((r.x - 0.0).abs() < EPS);
        assert!((r.y - 60.0).abs() < EPS);

        // 16:9 input to 4:3 output: full height, 480 of 640 columns, centred.
        let r = crop_rect(640, 360, 800, 600, &Framing::default());
        assert!((r.height - 360.0).abs() < EPS);
        assert!((r.width - 480.0).abs() < EPS);
        assert!((r.x - 80.0).abs() < EPS);
        assert!((r.y - 0.0).abs() < EPS);
    }

    #[test]
    fn zoom_two_halves_the_rectangle() {
        let r = crop_rect(1920, 1080, 1280, 720, &framing(2.0, 0.0, 0.0));
        assert!((r.width - 960.0).abs() < EPS);
        assert!((r.height - 540.0).abs() < EPS);
        assert!((r.x - 480.0).abs() < EPS);
        assert!((r.y - 270.0).abs() < EPS);
    }

    #[test]
    fn pan_reaches_the_edges() {
        let right_bottom = crop_rect(1920, 1080, 1280, 720, &framing(2.0, 1.0, 1.0));
        assert!((right_bottom.x + right_bottom.width - 1920.0).abs() < EPS);
        assert!((right_bottom.y + right_bottom.height - 1080.0).abs() < EPS);

        let left_top = crop_rect(1920, 1080, 1280, 720, &framing(2.0, -1.0, -1.0));
        assert_eq!(left_top.x, 0.0);
        assert_eq!(left_top.y, 0.0);
    }

    #[test]
    fn rectangle_always_inside_input() {
        let sizes = [
            (1, 1),
            (7, 5),
            (641, 479),
            (640, 480),
            (1920, 1080),
            (1081, 1921),
            (33, 1000),
            (1000, 33),
        ];
        let outputs = [(1280, 720), (800, 600), (3, 7), (1, 1), (720, 1280)];
        let zooms = [0.5, 1.0, 1.37, 2.0, 3.99, 4.0, 9.0, f32::NAN];
        let pans = [-5.0, -1.0, -0.37, 0.0, 0.5, 1.0, 2.0, f32::NAN];
        for &(iw, ih) in &sizes {
            for &(ow, oh) in &outputs {
                for &zoom in &zooms {
                    for &pan in &pans {
                        let r = crop_rect(iw, ih, ow, oh, &framing(zoom, pan, -pan));
                        assert!(
                            inside(&r, iw, ih),
                            "{iw}x{ih} -> {ow}x{oh} {zoom} {pan}: {r:?}"
                        );
                        let want = ow as f32 / oh as f32;
                        assert!(
                            (r.width / r.height - want).abs() < want * 0.01 + 0.01
                                || r.width < 2.0
                                || r.height < 2.0,
                            "aspect {r:?} vs {want}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn empty_sizes_do_not_panic() {
        let r = crop_rect(0, 0, 100, 100, &Framing::default());
        assert_eq!((r.width, r.height), (0.0, 0.0));
        let r = crop_rect(100, 50, 0, 0, &Framing::default());
        assert!(inside(&r, 100, 50));
    }

    #[test]
    fn scaling_keeps_a_flat_colour() {
        let mut src = VideoFrame::new_bgra(5, 3, 77);
        for y in 0..3 {
            for x in 0..5 {
                src.set_pixel(x, y, [10, 20, 30, 255]);
            }
        }
        let rect = crop_rect(5, 3, 8, 4, &Framing::default());
        let out = scale_frame(&src, &rect, 8, 4, true);
        assert_eq!((out.width, out.height, out.timestamp_us), (8, 4, 77));
        for y in 0..4 {
            for x in 0..8 {
                assert_eq!(out.pixel(x, y), [10, 20, 30, 255]);
            }
        }
    }

    #[test]
    fn short_source_data_gives_black() {
        let mut src = VideoFrame::new_bgra(4, 4, 0);
        src.data.truncate(10);
        let rect = crop_rect(4, 4, 4, 4, &Framing::default());
        let out = scale_frame(&src, &rect, 4, 4, false);
        assert!(out.data.iter().all(|&b| b == 0));
    }

    /// A frame of repeatable noise, so any mix-up of pixels shows.
    fn noise(w: u32, h: u32) -> VideoFrame {
        let mut f = VideoFrame::new_bgra(w, h, 3);
        let mut state = 12345u32;
        for byte in &mut f.data {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (state >> 24) as u8;
        }
        f
    }

    fn flipped(f: &VideoFrame) -> VideoFrame {
        let mut out = f.clone();
        for y in 0..f.height {
            for x in 0..f.width {
                out.set_pixel(x, y, f.pixel(f.width - 1 - x, y));
            }
        }
        out
    }

    #[test]
    fn identity_fast_path_matches_the_general_path() {
        let src = noise(37, 23);
        let whole = crop_rect(37, 23, 37, 23, &Framing::default());
        // A window cut out of a larger frame, at whole-pixel offsets, takes the fast path too.
        let big = noise(64, 40);
        let window = CropRect {
            x: 5.0,
            y: 7.0,
            width: 32.0,
            height: 18.0,
        };
        for (frame, rect, w, h) in [(&src, whole, 37, 23), (&big, window, 32, 18)] {
            for mirror in [false, true] {
                let fast = scale_frame(frame, &rect, w, h, mirror);
                let mut general = VideoFrame::new_bgra(w, h, frame.timestamp_us);
                scale_general(frame, &rect, &mut general, mirror);
                assert_eq!(fast, general, "{w}x{h} mirror {mirror}");
            }
        }
        // The window really is a copy of the source pixels.
        let out = scale_frame(&big, &window, 32, 18, false);
        assert_eq!(out.pixel(0, 0), big.pixel(5, 7));
        assert_eq!(out.pixel(31, 17), big.pixel(36, 24));
    }

    #[test]
    fn a_fractional_offset_is_not_a_plain_copy() {
        let src = noise(16, 8);
        let shifted = CropRect {
            x: 0.5,
            y: 0.0,
            width: 15.0,
            height: 8.0,
        };
        let out = scale_frame(&src, &shifted, 15, 8, false);
        let copy = scale_frame(&src, &CropRect { x: 0.0, ..shifted }, 15, 8, false);
        assert_ne!(out.data, copy.data);
    }

    #[test]
    fn mirror_is_an_exact_flip_whatever_the_scale() {
        let src = noise(60, 36);
        for (w, h) in [(60, 36), (40, 24), (90, 54), (45, 36)] {
            let rect = crop_rect(60, 36, w, h, &Framing::default());
            let plain = scale_frame(&src, &rect, w, h, false);
            let mirrored = scale_frame(&src, &rect, w, h, true);
            assert_eq!(mirrored.data, flipped(&plain).data, "{w}x{h}");
        }
    }

    #[test]
    fn shrinking_keeps_thin_lines_at_every_phase() {
        // One-pixel bright lines every 10 px on a dark frame, shrunk by 1.5 to 100x20. Lines
        // fall on the output grid in three different ways (inside one pixel or split in two).
        let (w, h, dark, bright) = (150u32, 30u32, 20u8, 120u8);
        let mut src = VideoFrame::new_bgra(w, h, 0);
        for y in 0..h {
            for x in 0..w {
                let v = if x % 10 == 3 { bright } else { dark };
                src.set_pixel(x, y, [v, v, v, 255]);
            }
        }
        let rect = crop_rect(w, h, 100, 20, &Framing::default());
        let out = scale_frame(&src, &rect, 100, 20, false);
        // The weight of a line is how much brighter than the dark level the pixels around it
        // get, added up. Averaging over the covered area keeps it the same for every line.
        let mut weights = Vec::new();
        for line in 0..14u32 {
            let centre = ((line * 10 + 3) as f32 + 0.5) / 1.5;
            let from = centre as u32 - 2;
            let total: i32 = (from..from + 5)
                .map(|x| out.pixel(x, 10)[0] as i32 - dark as i32)
                .sum();
            weights.push(total as f32);
        }
        let mean = weights.iter().sum::<f32>() / weights.len() as f32;
        let expected = (bright - dark) as f32 / 1.5;
        assert!((mean - expected).abs() < expected * 0.05, "mean {mean}");
        for (i, weight) in weights.iter().enumerate() {
            assert!(
                (weight - mean).abs() <= mean * 0.15,
                "line {i} has weight {weight}, mean {mean}"
            );
        }
        // Every line is still clearly visible as a peak.
        for line in 0..14u32 {
            let centre = (((line * 10 + 3) as f32 + 0.5) / 1.5) as u32;
            let peak = (centre - 1..=centre + 1)
                .map(|x| out.pixel(x, 10)[0])
                .max()
                .unwrap();
            assert!(peak >= dark + 30, "line {line} peaks at {peak}");
        }
    }

    #[test]
    fn enlarging_is_bilinear() {
        let mut src = VideoFrame::new_bgra(2, 1, 0);
        src.set_pixel(0, 0, [0, 0, 0, 255]);
        src.set_pixel(1, 0, [100, 100, 100, 255]);
        let rect = CropRect {
            x: 0.0,
            y: 0.0,
            width: 2.0,
            height: 1.0,
        };
        let out = scale_frame(&src, &rect, 4, 1, false);
        let blue: Vec<u8> = (0..4).map(|x| out.pixel(x, 0)[0]).collect();
        assert_eq!(blue, [0, 25, 75, 100]);
    }

    #[test]
    fn shrinking_averages_the_covered_area() {
        // 4 -> 3 pixels: output pixels cover 4/3 source pixels each, with fractional weights.
        let mut src = VideoFrame::new_bgra(4, 4, 0);
        for y in 0..4 {
            for (x, v) in [0u8, 30, 60, 90].into_iter().enumerate() {
                src.set_pixel(x as u32, y, [v, v, v, 255]);
            }
        }
        let rect = crop_rect(4, 4, 3, 3, &Framing::default());
        let out = scale_frame(&src, &rect, 3, 3, false);
        // Areas [0, 4/3), [4/3, 8/3), [8/3, 4): (0*1 + 30*1/3) / (4/3) = 7.5, 45, 82.5.
        let blue: Vec<u8> = (0..3).map(|x| out.pixel(x, 0)[0]).collect();
        assert_eq!(blue, [8, 45, 83]);
    }
}
