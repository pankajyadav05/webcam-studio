//! Turns raw person masks into the matte the compositor uses. A Rust port of the window's
//! `ui/engine/matte.js`, `tuning.js` and the alpha step of `compositor.js`, so meetings get the
//! same cutout as the preview.
//!
//! Per mask, at the model's resolution:
//! * Temporal smoothing. Small confidence changes settle over the steadiness time; large ones
//!   follow at once where the picture moved. Where the picture is still, a large change is the
//!   model changing its mind (a chair behind the shoulders, for example), so the pixel joins the
//!   person slowly and leaves quickly.
//!
//! Then at the output size: bilinear upscaling, a short blur for the edge softness, and the
//! "cut uncertain areas" contrast curve.

use rayon::prelude::*;
use studio_core::frame::VideoFrame;
use studio_core::settings::Cutout;
use studio_segmentation::Mask;

/// The numbers behind the three Advanced sliders. Matches `ui/engine/tuning.js`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    pub curve_low: f32,
    pub curve_high: f32,
    pub curve_mix: f32,
    /// Edge softness in pixels of a 1280-wide picture.
    pub radius: f32,
    /// Time constant for small changes, in milliseconds.
    pub still_ms: f32,
}

fn map(n: f32, low: f32, middle: f32, high: f32) -> f32 {
    if n < 0.5 {
        low + (middle - low) * n * 2.0
    } else {
        middle + (high - middle) * (n - 0.5) * 2.0
    }
}

impl Params {
    pub fn from(cutout: Cutout) -> Self {
        let c = cutout.clamped();
        Self {
            curve_low: map(c.cutoff, 0.025, 0.22, 0.4),
            curve_high: map(c.cutoff, 0.975, 0.88, 0.8),
            curve_mix: map(c.cutoff, 0.25, 0.85, 1.0),
            radius: map(c.softness, 2.0, 6.0, 12.0),
            still_ms: map(c.steadiness, 20.0, 80.0, 240.0),
        }
    }
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

fn smoothstep(low: f32, high: f32, v: f32) -> f32 {
    let t = clamp01((v - low) / (high - low));
    t * t * (3.0 - 2.0 * t)
}

/// A grey copy of a frame, averaged down to a coarse grid (0..=255 per cell).
#[derive(Debug, Clone, PartialEq)]
pub struct LumaGrid {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

/// Averages `frame` into `width` x `height` cells of luma.
pub fn luma_grid(frame: &VideoFrame, width: usize, height: usize) -> LumaGrid {
    let (fw, fh) = (frame.width as usize, frame.height as usize);
    let (width, height) = (width.max(1), height.max(1));
    let mut data = vec![0.0; width * height];
    if fw == 0 || fh == 0 || frame.data.len() < frame.stride * (fh - 1) + fw * 4 {
        return LumaGrid {
            width,
            height,
            data,
        };
    }
    data.par_chunks_mut(width)
        .enumerate()
        .for_each(|(gy, row)| {
            let (y0, y1) = (
                gy * fh / height,
                ((gy + 1) * fh / height).max(gy * fh / height + 1),
            );
            for (gx, cell) in row.iter_mut().enumerate() {
                let (x0, x1) = (
                    gx * fw / width,
                    ((gx + 1) * fw / width).max(gx * fw / width + 1),
                );
                let mut sum = 0u32;
                // Every other pixel in each direction is plenty for a motion estimate.
                let mut n = 0u32;
                for y in (y0..y1.min(fh)).step_by(2) {
                    let line = &frame.data[y * frame.stride..];
                    for x in (x0..x1.min(fw)).step_by(2) {
                        let p = &line[x * 4..x * 4 + 3];
                        sum += (p[0] as u32 * 29 + p[1] as u32 * 150 + p[2] as u32 * 77) >> 8;
                        n += 1;
                    }
                }
                *cell = sum as f32 / n.max(1) as f32;
            }
        });
    LumaGrid {
        width,
        height,
        data,
    }
}

/// Where the picture changed recently, 0 (still) to 1 (moving), on the luma grid.
#[derive(Debug, Default)]
struct MotionGate {
    width: usize,
    height: usize,
    previous: Option<Vec<f32>>,
    gate: Vec<f32>,
    spread: Vec<f32>,
}

impl MotionGate {
    fn reset(&mut self) {
        self.previous = None;
    }

    fn update(&mut self, luma: &LumaGrid, dt: f32) {
        let (w, h) = (luma.width, luma.height);
        if (w, h) != (self.width, self.height) || self.gate.len() != w * h {
            *self = Self {
                width: w,
                height: h,
                gate: vec![0.0; w * h],
                spread: vec![0.0; w * h],
                previous: None,
            };
        }
        let Some(previous) = self.previous.as_mut() else {
            self.previous = Some(luma.data.clone());
            self.gate.fill(0.0);
            return;
        };
        // Motion reaches two cells past what moved, then softens.
        for y in 0..h {
            for x in 0..w {
                let mut most = 0.0f32;
                for yy in y.saturating_sub(2)..=(y + 2).min(h - 1) {
                    for xx in x.saturating_sub(2)..=(x + 2).min(w - 1) {
                        let j = yy * w + xx;
                        most = most.max((luma.data[j] - previous[j]).abs());
                    }
                }
                self.spread[y * w + x] = smoothstep(5.0, 15.0, most);
            }
        }
        let memory = (-dt / 50.0).exp();
        let mut next = vec![0.0; w * h];
        for y in 0..h {
            for x in 0..w {
                let (mut sum, mut n) = (0.0, 0.0);
                for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        sum += self.spread[yy * w + xx];
                        n += 1.0;
                    }
                }
                let i = y * w + x;
                next[i] = (sum / n).max(self.gate[i] * memory);
            }
        }
        self.gate = next;
        previous.copy_from_slice(&luma.data);
    }

    /// Bilinear lookup at pixel `(x, y)` of a `width` x `height` mask.
    fn at(&self, x: usize, y: usize, width: usize, height: usize) -> f32 {
        let (gw, gh) = (self.width, self.height);
        let gx = ((x as f32 + 0.5) * gw as f32 / width as f32 - 0.5).clamp(0.0, (gw - 1) as f32);
        let gy = ((y as f32 + 0.5) * gh as f32 / height as f32 - 0.5).clamp(0.0, (gh - 1) as f32);
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(gw - 1), (y0 + 1).min(gh - 1));
        let (fx, fy) = (gx - x0 as f32, gy - y0 as f32);
        let g = &self.gate;
        (g[y0 * gw + x0] * (1.0 - fx) + g[y0 * gw + x1] * fx) * (1.0 - fy)
            + (g[y1 * gw + x0] * (1.0 - fx) + g[y1 * gw + x1] * fx) * fy
    }
}

/// Temporal smoothing of the model's masks. Keeps its history from mask to mask.
#[derive(Debug, Default)]
pub struct MatteFilter {
    width: usize,
    height: usize,
    history: Vec<f32>,
    motion: MotionGate,
}

impl MatteFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets the history; the next mask is taken as it is.
    pub fn reset(&mut self) {
        self.history.clear();
        self.motion.reset();
    }

    /// Smooths `mask` (made `elapsed_ms` after the previous one) and returns the result, 0..=1
    /// per pixel at the mask's size. `luma` is the frame the mask was made from.
    pub fn apply(
        &mut self,
        mask: &Mask,
        elapsed_ms: f32,
        luma: Option<&LumaGrid>,
        still_ms: f32,
    ) -> &[f32] {
        let (w, h) = (mask.width as usize, mask.height as usize);
        let count = w * h;
        let values = &mask.alpha[..count.min(mask.alpha.len())];
        let first = (w, h) != (self.width, self.height)
            || self.history.len() != count
            || values.len() != count;
        let dt = elapsed_ms.max(1.0);
        if first {
            self.width = w;
            self.height = h;
            self.history = values.iter().map(|&v| v as f32 / 255.0).collect();
            self.history.resize(count, 1.0);
            self.motion.reset();
            if let Some(luma) = luma {
                self.motion.update(luma, dt);
            }
            return &self.history;
        }
        let gated = luma.map(|luma| self.motion.update(luma, dt)).is_some();
        let still = 1.0 - (-dt / still_ms).exp();
        let fast = 1.0 - (-dt / 2.0).exp();
        // Still areas: join over 25x and leave over 3x the steadiness time.
        let join = 1.0 - (-dt / (still_ms * 25.0)).exp();
        let leave = 1.0 - (-dt / (still_ms * 3.0)).exp();
        let motion = &self.motion;
        self.history
            .par_chunks_mut(w)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, h_val) in row.iter_mut().enumerate() {
                    let value = values[y * w + x] as f32 / 255.0;
                    let delta = value - *h_val;
                    let change = delta.abs();
                    let blend = if change <= 0.04 {
                        still
                    } else if !gated {
                        // The window's original motion ramp toward a 2 ms time constant.
                        let moving = smoothstep(0.04, 0.18, change);
                        let tau = still_ms * (1.0 - moving) + 2.0 * moving;
                        1.0 - (-dt / tau).exp()
                    } else {
                        let rest = if delta > 0.0 { join } else { leave };
                        let big = rest + (fast - rest) * motion.at(x, y, w, h);
                        if change >= 0.18 {
                            big
                        } else {
                            still + (big - still) * smoothstep(0.04, 0.18, change)
                        }
                    };
                    *h_val += blend * delta;
                }
            });
        &self.history
    }
}

/// Scales `alpha` (0..=1, `width` x `height`) to the output size and applies the edge softness
/// and the cutoff curve. Returns a mask the compositor can use directly.
pub fn finish(
    alpha: &[f32],
    width: usize,
    height: usize,
    out_w: usize,
    out_h: usize,
    params: &Params,
) -> Mask {
    let mut up = vec![0.0f32; out_w * out_h];
    if width > 0 && height > 0 && alpha.len() >= width * height {
        let sx = width as f32 / out_w as f32;
        let sy = height as f32 / out_h as f32;
        up.par_chunks_mut(out_w).enumerate().for_each(|(y, row)| {
            let gy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, (height - 1) as f32);
            let y0 = gy as usize;
            let y1 = (y0 + 1).min(height - 1);
            let fy = gy - y0 as f32;
            for (x, out) in row.iter_mut().enumerate() {
                let gx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, (width - 1) as f32);
                let x0 = gx as usize;
                let x1 = (x0 + 1).min(width - 1);
                let fx = gx - x0 as f32;
                *out = (alpha[y0 * width + x0] * (1.0 - fx) + alpha[y0 * width + x1] * fx)
                    * (1.0 - fy)
                    + (alpha[y1 * width + x0] * (1.0 - fx) + alpha[y1 * width + x1] * fx) * fy;
            }
        });
    } else {
        up.fill(1.0);
    }
    // Two box passes make a soft, tent-shaped blur with roughly the slider's reach.
    let radius = ((params.radius * out_w as f32 / 1280.0) / 2.0).round() as usize;
    if radius > 0 {
        for _ in 0..2 {
            box_blur(&mut up, out_w, out_h, radius);
        }
    }
    let (lo, hi, mix) = (params.curve_low, params.curve_high, params.curve_mix);
    let alpha = up
        .par_iter()
        .map(|&a| {
            let a = clamp01(a);
            ((a + (smoothstep(lo, hi, a) - a) * mix) * 255.0).round() as u8
        })
        .collect();
    Mask {
        width: out_w as u32,
        height: out_h as u32,
        alpha,
    }
}

/// Separable running-sum box blur of a single-channel image, edges clamped.
fn box_blur(data: &mut [f32], width: usize, height: usize, radius: usize) {
    let r = radius as isize;
    let norm = 1.0 / (2 * radius + 1) as f32;
    data.par_chunks_mut(width).for_each(|row| {
        let src = row.to_vec();
        let at = |i: isize| src[i.clamp(0, width as isize - 1) as usize];
        let mut sum: f32 = (-r..=r).map(at).sum();
        for (x, out) in row.iter_mut().enumerate() {
            *out = sum * norm;
            let x = x as isize;
            sum += at(x + r + 1) - at(x - r);
        }
    });
    let mut columns = vec![0.0f32; width * height];
    columns
        .par_chunks_mut(height)
        .enumerate()
        .for_each(|(x, col)| {
            let at = |y: isize| data[y.clamp(0, height as isize - 1) as usize * width + x];
            let mut sum: f32 = (-r..=r).map(at).sum();
            for (y, out) in col.iter_mut().enumerate() {
                *out = sum * norm;
                let y = y as isize;
                sum += at(y + r + 1) - at(y - r);
            }
        });
    data.par_chunks_mut(width).enumerate().for_each(|(y, row)| {
        for (x, out) in row.iter_mut().enumerate() {
            *out = columns[x * height + y];
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(w: u32, h: u32, v: u8) -> Mask {
        Mask {
            width: w,
            height: h,
            alpha: vec![v; (w * h) as usize],
        }
    }
    fn grid(v: f32) -> LumaGrid {
        LumaGrid {
            width: 1,
            height: 1,
            data: vec![v],
        }
    }

    #[test]
    fn default_sliders_match_the_window() {
        let p = Params::from(Cutout::default());
        assert_eq!(
            (p.curve_low, p.curve_high, p.curve_mix, p.radius, p.still_ms),
            (0.22, 0.88, 0.85, 6.0, 80.0)
        );
        let p = Params::from(Cutout {
            cutoff: 0.0,
            softness: 1.0,
            steadiness: 0.0,
        });
        assert_eq!((p.curve_low, p.radius, p.still_ms), (0.025, 12.0, 20.0));
    }

    #[test]
    fn a_still_picture_holds_back_a_flip_the_model_makes_on_its_own() {
        let mut f = MatteFilter::new();
        f.apply(&mask(4, 4, 0), 33.3, Some(&grid(120.0)), 80.0);
        f.apply(&mask(4, 4, 0), 33.3, Some(&grid(120.0)), 80.0);
        let flicker = f.apply(&mask(4, 4, 255), 33.3, Some(&grid(120.0)), 80.0)[0];
        assert!(flicker < 0.05, "one-frame flip leaked {flicker}");
        let mut held = 0.0;
        for _ in 0..15 {
            held = f.apply(&mask(4, 4, 255), 33.3, Some(&grid(120.0)), 80.0)[0];
        }
        assert!(
            held > 0.05 && held < 0.5,
            "sustained flip after 0.5 s: {held}"
        );
        let mut gone = held;
        for _ in 0..8 {
            gone = f.apply(&mask(4, 4, 0), 33.3, Some(&grid(120.0)), 80.0)[0];
        }
        assert!(gone < held * 0.45, "release {held} -> {gone}");
    }

    #[test]
    fn a_moving_picture_lets_the_mask_follow_at_once() {
        let mut f = MatteFilter::new();
        f.apply(&mask(4, 4, 0), 33.3, Some(&grid(60.0)), 80.0);
        f.apply(&mask(4, 4, 0), 33.3, Some(&grid(60.0)), 80.0);
        assert!(f.apply(&mask(4, 4, 255), 33.3, Some(&grid(160.0)), 80.0)[0] > 0.98);
    }

    #[test]
    fn reset_takes_the_next_mask_as_it_is() {
        let mut f = MatteFilter::new();
        f.apply(&mask(4, 4, 0), 33.3, Some(&grid(90.0)), 80.0);
        f.reset();
        assert_eq!(
            f.apply(&mask(4, 4, 255), 33.3, Some(&grid(90.0)), 80.0)[0],
            1.0
        );
    }

    #[test]
    fn finish_scales_softens_and_applies_the_curve() {
        // Left half person, right half background, 4x2 scaled to 64x8.
        let alpha = [1.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0];
        let out = finish(&alpha, 4, 2, 64, 8, &Params::from(Cutout::default()));
        assert_eq!((out.width, out.height, out.alpha.len()), (64, 8, 512));
        assert_eq!(out.alpha[0], 255);
        assert_eq!(out.alpha[63], 0);
        let row: Vec<u8> = out.alpha[..64].to_vec();
        assert!(
            row.windows(2).all(|w| w[0] >= w[1]),
            "edge is not monotonic: {row:?}"
        );
        assert!(
            row.iter().any(|&a| a > 20 && a < 235),
            "edge has no soft pixels"
        );
    }

    #[test]
    fn luma_grid_averages_cells() {
        let mut frame = VideoFrame::new_bgra(4, 2, 0);
        for px in frame.data.chunks_exact_mut(4).take(2) {
            px[..3].copy_from_slice(&[255, 255, 255]);
        }
        let g = luma_grid(&frame, 2, 1);
        assert!(g.data[0] > 250.0 && g.data[1] < 1.0, "{:?}", g.data);
    }
}
