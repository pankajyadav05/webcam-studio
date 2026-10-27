//! Touch-ups to the person's look: skin smoothing, low-light correction and warmth.
//!
//! The window does the same in WebGL (`ui/engine/compositor.js`, `ui/engine/look.js`); keep
//! the constants and the order of the steps the same in both, so meetings look like the
//! preview.
//!
//! 1. Skin smoothing: a guided filter of the picture with itself, at half size. It flattens
//!    small changes (pores, blemishes, noise) and keeps strong edges (eyes, brows, lips, hair).
//!    It is mixed in only where both the area and the pixel itself are a skin tone (so eyes,
//!    brows and lips stay sharp), and never fully, so skin keeps some texture.
//! 2. Low light: a curve that lifts the shadows, with its strength set from how dark the
//!    picture is and eased over time so it never flickers.
//! 3. Warmth: more red and less blue, or the opposite.

use rayon::prelude::*;
use studio_core::frame::VideoFrame;
use studio_core::settings::Appearance;

/// Smoothing radius at half size, as a share of the picture height (4 px at 720p).
pub const SMOOTH_RADIUS: f32 = 0.006;
/// Edge threshold of the smoothing (standard deviation in 0..1 units) at strength 0 and 1.
pub const SMOOTH_SIGMA: (f32, f32) = (0.02, 0.06);
/// How much of the smoothed picture is used on skin, at strength 0 and 1.
pub const SMOOTH_MIX: (f32, f32) = (0.3, 0.85);
/// Low light: the average brightness (0..1) a dark picture is lifted to.
pub const LIGHT_TARGET: f32 = 0.45;
/// Low light: the strongest lift.
pub const LIGHT_MAX_GAIN: f32 = 2.4;
/// Low light: how fast the lift follows the picture, as a time constant in seconds.
pub const LIGHT_SECONDS: f32 = 0.8;
/// Warmth: change of the red and blue channels at warmth 1.
pub const WARMTH_SHIFT: f32 = 0.12;

/// Low-light gain wanted for a picture whose average brightness is `mean` (0..1).
pub fn light_gain(mean: f32) -> f32 {
    let m = mean.clamp(0.02, 1.0);
    if m >= LIGHT_TARGET {
        return 1.0;
    }
    (LIGHT_TARGET * (1.0 - m) / (m * (1.0 - LIGHT_TARGET))).clamp(1.0, LIGHT_MAX_GAIN)
}

/// The low-light curve: `x` stays in 0..1, 0 and 1 stay put, and the slope at black is `gain`.
pub fn light_curve(x: f32, gain: f32) -> f32 {
    x * gain / (1.0 + (gain - 1.0) * x)
}

/// How much a colour (0..1 sRGB) looks like skin, 0..1. Works for light and dark skin, as it
/// tests the colour (Cb, Cr) more than the brightness.
pub fn skin_weight(r: f32, g: f32, b: f32) -> f32 {
    let y = (0.299 * r + 0.587 * g + 0.114 * b) * 255.0;
    let cb = 128.0 + (-0.168_736 * r - 0.331_264 * g + 0.5 * b) * 255.0;
    let cr = 128.0 + (0.5 * r - 0.418_688 * g - 0.081_312 * b) * 255.0;
    band(cb, 77.0, 127.0, 6.0) * band(cr, 133.0, 173.0, 6.0) * band(y, 35.0, 250.0, 15.0)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 1 inside `lo..hi`, 0 outside, with soft edges `soft` wide on each side.
fn band(x: f32, lo: f32, hi: f32, soft: f32) -> f32 {
    smoothstep(lo - soft, lo + soft, x) * (1.0 - smoothstep(hi - soft, hi + soft, x))
}

/// Applies the touch-ups and keeps what they need from frame to frame.
#[derive(Debug, Default)]
pub struct Look {
    /// Current low-light gain, eased towards the wanted one.
    gain: f32,
    last_us: Option<u64>,
    moments: Vec<f32>,
    scratch: Vec<f32>,
    coefficients: Vec<f32>,
}

/// Floats per half-size pixel: r, g, b, r², g², b².
const MOMENTS: usize = 6;
/// Floats per half-size pixel: a (r, g, b), skin, b (r, g, b).
const COEFFS: usize = 7;

impl Look {
    pub fn new() -> Self {
        Self {
            gain: 1.0,
            ..Self::default()
        }
    }

    /// Forgets the eased low-light gain, so it starts again from no change.
    pub fn reset(&mut self) {
        self.gain = 1.0;
        self.last_us = None;
    }

    /// The low-light gain in use now.
    pub fn gain(&self) -> f32 {
        self.gain
    }

    /// Touches up `frame` in place. Does nothing when no touch-up is on.
    pub fn apply(&mut self, frame: &mut VideoFrame, appearance: &Appearance) {
        let appearance = appearance.clamped();
        if !appearance.changes_colour() || frame.width == 0 || frame.height == 0 {
            self.reset();
            return;
        }
        self.ease_gain(frame, appearance.low_light);
        let luts = curves(self.gain, appearance.warmth);
        if appearance.smoothing > 0.001 {
            self.smooth(frame, appearance.smoothing, &luts);
        } else {
            apply_luts(frame, &luts);
        }
    }

    fn ease_gain(&mut self, frame: &VideoFrame, on: bool) {
        if !on {
            self.gain = 1.0;
            self.last_us = None;
            return;
        }
        let wanted = light_gain(mean_brightness(frame));
        let dt = match self.last_us {
            Some(last) if frame.timestamp_us > last => {
                ((frame.timestamp_us - last) as f32 / 1e6).min(0.5)
            }
            Some(_) => 0.0,
            None => 1.0 / 30.0,
        };
        self.last_us = Some(frame.timestamp_us);
        if self.gain < 1.0 {
            self.gain = 1.0;
        }
        self.gain += (wanted - self.gain) * (1.0 - (-dt / LIGHT_SECONDS).exp());
    }

    fn smooth(&mut self, frame: &mut VideoFrame, strength: f32, luts: &[[u8; 256]; 3]) {
        let (w, h) = (frame.width as usize, frame.height as usize);
        let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
        let radius = ((h as f32 * SMOOTH_RADIUS).round() as usize).max(1);
        let sigma = SMOOTH_SIGMA.0 + (SMOOTH_SIGMA.1 - SMOOTH_SIGMA.0) * strength;
        let eps = sigma * sigma;
        let mix = SMOOTH_MIX.0 + (SMOOTH_MIX.1 - SMOOTH_MIX.0) * strength;

        // Half size: the colour and its square, averaged over 2 × 2 pixels.
        self.moments.resize(hw * hh * MOMENTS, 0.0);
        let (data, stride) = (&frame.data, frame.stride);
        self.moments
            .par_chunks_mut(hw * MOMENTS)
            .enumerate()
            .for_each(|(y, row)| {
                for x in 0..hw {
                    let mut sum = [0.0f32; MOMENTS];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(w - 1), (2 * y + dy).min(h - 1));
                        let p = sy * stride + sx * 4;
                        let (b, g, r) = (
                            data[p] as f32 / 255.0,
                            data[p + 1] as f32 / 255.0,
                            data[p + 2] as f32 / 255.0,
                        );
                        sum[0] += r;
                        sum[1] += g;
                        sum[2] += b;
                        sum[3] += r * r;
                        sum[4] += g * g;
                        sum[5] += b * b;
                    }
                    for (out, s) in row[x * MOMENTS..][..MOMENTS].iter_mut().zip(sum) {
                        *out = s * 0.25;
                    }
                }
            });
        box_filter(
            &mut self.moments,
            &mut self.scratch,
            hw,
            hh,
            MOMENTS,
            radius,
        );

        // Guided filter coefficients, and how much each spot looks like skin.
        self.coefficients.resize(hw * hh * COEFFS, 0.0);
        self.coefficients
            .par_chunks_mut(COEFFS)
            .zip(self.moments.par_chunks(MOMENTS))
            .for_each(|(c, m)| {
                for i in 0..3 {
                    let variance = (m[3 + i] - m[i] * m[i]).max(0.0);
                    let a = variance / (variance + eps);
                    c[i] = a;
                    c[4 + i] = m[i] - a * m[i];
                }
                c[3] = skin_weight(m[0], m[1], m[2]);
            });
        box_filter(
            &mut self.coefficients,
            &mut self.scratch,
            hw,
            hh,
            COEFFS,
            radius,
        );

        // Full size: q = a·I + b, mixed in on skin, then the light and warmth curves.
        let coefficients = &self.coefficients;
        frame
            .data
            .par_chunks_mut(stride)
            .take(h)
            .enumerate()
            .for_each(|(y, row)| {
                let fy = ((y as f32 + 0.5) * 0.5 - 0.5).clamp(0.0, (hh - 1) as f32);
                let (y0, ty) = (fy.floor() as usize, fy.fract());
                let y1 = (y0 + 1).min(hh - 1);
                for x in 0..w {
                    let fx = ((x as f32 + 0.5) * 0.5 - 0.5).clamp(0.0, (hw - 1) as f32);
                    let (x0, tx) = (fx.floor() as usize, fx.fract());
                    let x1 = (x0 + 1).min(hw - 1);
                    let at =
                        |xx: usize, yy: usize| &coefficients[(yy * hw + xx) * COEFFS..][..COEFFS];
                    let (c00, c10, c01, c11) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
                    let mut c = [0.0f32; COEFFS];
                    for (i, v) in c.iter_mut().enumerate() {
                        let top = c00[i] + (c10[i] - c00[i]) * tx;
                        let bottom = c01[i] + (c11[i] - c01[i]) * tx;
                        *v = top + (bottom - top) * ty;
                    }
                    let p = x * 4;
                    let weight =
                        c[3] * skin_weight(
                            row[p + 2] as f32 / 255.0,
                            row[p + 1] as f32 / 255.0,
                            row[p] as f32 / 255.0,
                        ) * mix;
                    // BGRA in memory; channel i of the filter is r, g, b.
                    for (i, offset) in [2usize, 1, 0].into_iter().enumerate() {
                        let original = row[p + offset] as f32 / 255.0;
                        let smoothed = c[i] * original + c[4 + i];
                        let value = original + (smoothed - original) * weight;
                        row[p + offset] = luts[i][(value.clamp(0.0, 1.0) * 255.0).round() as usize];
                    }
                }
            });
    }
}

/// Average brightness (0..1) of a BGRA frame, from every eighth pixel.
pub fn mean_brightness(frame: &VideoFrame) -> f32 {
    let (mut sum, mut n) = (0.0f64, 0u32);
    for y in (0..frame.height as usize).step_by(8) {
        let row = &frame.data[y * frame.stride..];
        for x in (0..frame.width as usize).step_by(8) {
            let p = &row[x * 4..x * 4 + 3];
            sum += (0.114 * p[0] as f64 + 0.587 * p[1] as f64 + 0.299 * p[2] as f64) / 255.0;
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { (sum / n as f64) as f32 }
}

/// One table per channel (r, g, b): the low-light curve, then warmth.
fn curves(gain: f32, warmth: f32) -> [[u8; 256]; 3] {
    let scale = [
        1.0 + WARMTH_SHIFT * warmth,
        1.0,
        1.0 - WARMTH_SHIFT * warmth,
    ];
    let mut luts = [[0u8; 256]; 3];
    for (lut, scale) in luts.iter_mut().zip(scale) {
        for (v, out) in lut.iter_mut().enumerate() {
            let x = light_curve(v as f32 / 255.0, gain) * scale;
            *out = (x.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    luts
}

fn apply_luts(frame: &mut VideoFrame, luts: &[[u8; 256]; 3]) {
    let (w, h) = (frame.width as usize, frame.height as usize);
    frame
        .data
        .par_chunks_mut(frame.stride)
        .take(h)
        .for_each(|row| {
            for px in row[..w * 4].chunks_exact_mut(4) {
                px[2] = luts[0][px[2] as usize];
                px[1] = luts[1][px[1] as usize];
                px[0] = luts[2][px[0] as usize];
            }
        });
}

/// Box average of an interleaved image with `channels` floats per pixel, `radius` pixels each
/// way, in place. Edges repeat the last pixel, as a GPU texture does.
fn box_filter(
    image: &mut [f32],
    scratch: &mut Vec<f32>,
    w: usize,
    h: usize,
    channels: usize,
    radius: usize,
) {
    let row_len = w * channels;
    let norm = 1.0 / (2 * radius + 1) as f32;
    scratch.resize(image.len(), 0.0);
    // Across: a running sum per row.
    scratch
        .par_chunks_mut(row_len)
        .zip(image.par_chunks(row_len))
        .for_each(|(out, row)| {
            for c in 0..channels {
                let at = |x: isize| row[(x.clamp(0, w as isize - 1) as usize) * channels + c];
                let mut sum: f32 = (-(radius as isize)..=radius as isize).map(at).sum();
                for x in 0..w {
                    out[x * channels + c] = sum * norm;
                    sum += at(x as isize + radius as isize + 1) - at(x as isize - radius as isize);
                }
            }
        });
    // Down: each output row adds up its neighbours.
    let source = &*scratch;
    image
        .par_chunks_mut(row_len)
        .enumerate()
        .for_each(|(y, out)| {
            out.fill(0.0);
            for dy in -(radius as isize)..=radius as isize {
                let sy = (y as isize + dy).clamp(0, h as isize - 1) as usize;
                for (o, s) in out.iter_mut().zip(&source[sy * row_len..][..row_len]) {
                    *o += s;
                }
            }
            for o in out.iter_mut() {
                *o *= norm;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> VideoFrame {
        let mut frame = VideoFrame::new_bgra(w, h, 0);
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = f(x, y);
                frame.set_pixel(x, y, [b, g, r, 255]);
            }
        }
        frame
    }

    /// A light skin tone and a dark one, both in 0..1.
    const LIGHT_SKIN: [f32; 3] = [0.88, 0.70, 0.60];
    const DARK_SKIN: [f32; 3] = [0.45, 0.30, 0.22];

    #[test]
    fn skin_tones_count_as_skin_and_other_colours_do_not() {
        for [r, g, b] in [LIGHT_SKIN, DARK_SKIN] {
            assert!(skin_weight(r, g, b) > 0.9, "skin {r} {g} {b}");
        }
        for [r, g, b] in [
            [0.2, 0.4, 0.9],
            [0.3, 0.7, 0.3],
            [0.5, 0.5, 0.5],
            [0.02, 0.02, 0.02],
        ] {
            assert!(skin_weight(r, g, b) < 0.05, "not skin {r} {g} {b}");
        }
    }

    #[test]
    fn nothing_changes_with_every_touch_up_off() {
        let input = frame(32, 18, |x, y| [(x * 8) as u8, (y * 14) as u8, 90]);
        let mut out = input.clone();
        Look::new().apply(&mut out, &Appearance::default());
        assert_eq!(out, input);
    }

    #[test]
    fn smoothing_flattens_blemishes_on_skin_and_keeps_strong_edges() {
        let skin = LIGHT_SKIN.map(|v| (v * 255.0) as u8);
        // Skin with a speckle of small dots on the left, and a hard dark edge on the right.
        let input = frame(64, 64, |x, y| {
            if x >= 48 {
                [20, 20, 20]
            } else if (x + 3 * y) % 5 == 0 {
                [skin[0] - 14, skin[1] - 14, skin[2] - 14]
            } else {
                skin
            }
        });
        let mut out = input.clone();
        let mut look = Look::new();
        look.apply(
            &mut out,
            &Appearance {
                smoothing: 1.0,
                ..Appearance::default()
            },
        );
        let spread = |f: &VideoFrame| {
            let values: Vec<i32> = (8..40).map(|x| f.pixel(x, 30)[2] as i32).collect();
            values.iter().max().unwrap() - values.iter().min().unwrap()
        };
        assert!(
            spread(&out) * 3 < spread(&input),
            "{} vs {}",
            spread(&out),
            spread(&input)
        );
        // The dark area is not skin, and the edge stays sharp.
        assert_eq!(out.pixel(60, 30), input.pixel(60, 30));
        assert!(out.pixel(46, 30)[2] > 150);
    }

    #[test]
    fn smoothing_leaves_colours_that_are_not_skin_alone() {
        let input = frame(32, 32, |x, y| {
            if (x + y) % 3 == 0 {
                [40, 90, 200]
            } else {
                [60, 110, 220]
            }
        });
        let mut out = input.clone();
        Look::new().apply(
            &mut out,
            &Appearance {
                smoothing: 1.0,
                ..Appearance::default()
            },
        );
        let changed = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .map(|(x, y)| (out.pixel(x, y)[0] as i32 - input.pixel(x, y)[0] as i32).abs())
            .max()
            .unwrap();
        assert!(changed <= 2, "changed by {changed}");
    }

    #[test]
    fn low_light_brightens_a_dark_picture_gradually() {
        assert_eq!(light_gain(0.6), 1.0);
        assert!(light_gain(0.2) > 2.0);
        assert_eq!(light_gain(0.0), LIGHT_MAX_GAIN);
        assert_eq!(light_curve(0.0, 2.0), 0.0);
        assert!((light_curve(1.0, 2.0) - 1.0).abs() < 1e-6);

        let dark = frame(32, 18, |_, _| [40, 40, 40]);
        let mut look = Look::new();
        let on = Appearance {
            low_light: true,
            ..Appearance::default()
        };
        let mut first = dark.clone();
        look.apply(&mut first, &on);
        let after_one = first.pixel(5, 5)[2];
        assert!(after_one > 40, "first frame already lifts a little");
        let mut later = dark.clone();
        for i in 1..=60u64 {
            later = dark.clone();
            later.timestamp_us = i * 33_333;
            look.apply(&mut later, &on);
        }
        assert!(
            later.pixel(5, 5)[2] > after_one + 20,
            "{} vs {after_one}",
            later.pixel(5, 5)[2]
        );
        // A bright picture is left alone.
        let bright = frame(32, 18, |_, _| [190, 190, 190]);
        let mut out = bright.clone();
        Look::new().apply(&mut out, &on);
        assert_eq!(out, bright);
    }

    #[test]
    fn warmth_moves_red_and_blue_in_opposite_ways() {
        let grey = frame(8, 8, |_, _| [128, 128, 128]);
        let mut warm = grey.clone();
        Look::new().apply(
            &mut warm,
            &Appearance {
                warmth: 1.0,
                ..Appearance::default()
            },
        );
        let [b, g, r, _] = warm.pixel(3, 3);
        assert!(r > 128 && b < 128 && g == 128, "{r} {g} {b}");
        let mut cool = grey.clone();
        Look::new().apply(
            &mut cool,
            &Appearance {
                warmth: -1.0,
                ..Appearance::default()
            },
        );
        let [b, _, r, _] = cool.pixel(3, 3);
        assert!(r < 128 && b > 128);
    }

    #[test]
    fn box_filter_averages_with_repeated_edges() {
        let mut image = vec![0.0, 0.0, 3.0, 0.0, 0.0];
        box_filter(&mut image, &mut Vec::new(), 5, 1, 1, 1);
        assert_eq!(image, vec![0.0, 1.0, 1.0, 1.0, 0.0]);
    }
}
