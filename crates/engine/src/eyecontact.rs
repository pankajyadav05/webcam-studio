//! Eye contact: you read the screen, which sits below the camera, so your eyes look down in
//! the picture. This lifts each pupil back towards the middle of the eye with a small, soft
//! warp of the eye area, so you seem to look at the people you talk to.
//!
//! It is deliberately gentle: it only undoes a downward look, by at most a fraction of the
//! eye, and does nothing to closed or half-closed eyes. The window does the same in
//! `ui/engine/compositor.js` and `look.js`; keep the numbers the same in both.

use rayon::prelude::*;
use studio_core::frame::VideoFrame;
use studio_segmentation::{Eye, EyePair};

use crate::framing::CropRect;

/// Where the pupil should sit, as a share of the eye opening's height above its centre.
pub const AIM: f32 = 0.05;
/// How much of the downward look is undone.
pub const AMOUNT: f32 = 0.85;
/// The largest lift, as a share of the eye's width.
pub const MAX_LIFT: f32 = 0.11;
/// Eyes less open than this (height / width) are closed or blinking and left alone.
pub const MIN_OPEN: f32 = 0.14;
/// The warped area around each eye, in eye widths (half sizes across and down).
pub const REACH: (f32, f32) = (0.75, 0.6);
/// The warp is full inside this share of the area and fades out to its edge.
pub const CORE: f32 = 0.35;
/// How fast the lift follows the eyes, in seconds (smooths out landmark jitter).
pub const EASE_SECONDS: f32 = 0.08;

/// One eye in output pixels, with the lift to apply.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EyeWarp {
    pub cx: f32,
    pub cy: f32,
    pub width: f32,
    pub lift: f32,
}

/// The lift for an eye in output pixels: how far the pupil sits below where it should.
pub fn lift_for(cy: f32, width: f32, height: f32, pupil_y: f32) -> f32 {
    if width <= 0.0 || height < MIN_OPEN * width {
        return 0.0;
    }
    let drop = pupil_y - (cy - AIM * height);
    (drop * AMOUNT).clamp(0.0, MAX_LIFT * width)
}

/// An eye from the camera picture, moved into output pixels.
fn to_output(
    eye: &Eye,
    input: (u32, u32),
    rect: &CropRect,
    out: (u32, u32),
    mirror: bool,
) -> EyeWarp {
    let (iw, ih) = (input.0 as f32, input.1 as f32);
    let sx = out.0 as f32 / rect.width.max(1e-3);
    let sy = out.1 as f32 / rect.height.max(1e-3);
    let mut cx = (eye.cx * iw - rect.x) * sx;
    if mirror {
        cx = out.0 as f32 - cx;
    }
    let cy = (eye.cy * ih - rect.y) * sy;
    let width = eye.width * iw * sx;
    let height = eye.height * ih * sy;
    let pupil_y = (eye.pupil_y * ih - rect.y) * sy;
    EyeWarp {
        cx,
        cy,
        width,
        lift: lift_for(cy, width, height, pupil_y),
    }
}

/// Follows the eyes from picture to picture.
#[derive(Debug, Default)]
pub struct EyeContact {
    eyes: Vec<EyePair>,
    lift: [f32; 2],
    last_us: Option<u64>,
    seen_us: Option<u64>,
}

/// Eyes older than this are not used (the face moved on).
const STALE_SECONDS: f32 = 0.3;

impl EyeContact {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn observe(&mut self, eyes: &[EyePair], now_us: u64) {
        self.eyes = eyes.to_vec();
        self.seen_us = Some(now_us);
    }

    /// The warps for the picture at `now_us`, framed by `rect`. Only the biggest face counts.
    pub fn step(
        &mut self,
        input: (u32, u32),
        rect: &CropRect,
        out: (u32, u32),
        mirror: bool,
        now_us: u64,
    ) -> Vec<EyeWarp> {
        let dt = match self.last_us {
            Some(last) if now_us > last => ((now_us - last) as f32 / 1e6).min(0.5),
            _ => 1.0 / 30.0,
        };
        self.last_us = Some(now_us);
        let fresh = self
            .seen_us
            .is_some_and(|t| now_us.saturating_sub(t) as f32 / 1e6 <= STALE_SECONDS);
        let pair = self
            .eyes
            .iter()
            .max_by(|a, b| a.left.width.total_cmp(&b.left.width))
            .filter(|_| fresh);
        let Some(pair) = pair else {
            self.lift = [0.0; 2];
            return Vec::new();
        };
        let k = 1.0 - (-dt / EASE_SECONDS).exp();
        let mut warps = [
            to_output(&pair.left, input, rect, out, mirror),
            to_output(&pair.right, input, rect, out, mirror),
        ];
        // Both eyes move together, by the smaller wanted lift, so they never look different
        // ways; a blink on one side stops both.
        let wanted = warps[0].lift.min(warps[1].lift);
        for (i, warp) in warps.iter_mut().enumerate() {
            self.lift[i] += (wanted - self.lift[i]) * k;
            warp.lift = self.lift[i];
        }
        warps.to_vec()
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of the lift a point gets, `d` area-radii from the eye's centre.
pub fn weight(d: f32) -> f32 {
    1.0 - smoothstep(CORE, 1.0, d)
}

/// Moves the eye areas of `frame` in place.
pub fn apply(frame: &mut VideoFrame, warps: &[EyeWarp]) {
    for warp in warps {
        if warp.lift < 0.05 || warp.width < 4.0 {
            continue;
        }
        let (rx, ry) = (REACH.0 * warp.width, REACH.1 * warp.width);
        let x0 = (warp.cx - rx).floor().max(0.0) as usize;
        let x1 = ((warp.cx + rx).ceil() as usize).min(frame.width as usize);
        let y0 = (warp.cy - ry).floor().max(0.0) as usize;
        let y1 = ((warp.cy + ry).ceil() as usize).min(frame.height as usize);
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        // Read from a copy: each pixel takes the one `lift` below it, so the eye moves up.
        let h = frame.height as usize;
        let reach_down = (warp.lift.ceil() as usize + 2).min(h);
        let src_y1 = (y1 + reach_down).min(h);
        let stride = frame.stride;
        let copy: Vec<u8> = frame.data[y0 * stride..src_y1 * stride].to_vec();
        let row_at = |y: usize| &copy[(y - y0) * stride..(y - y0 + 1) * stride];
        frame.data[y0 * stride..y1 * stride]
            .par_chunks_mut(stride)
            .enumerate()
            .for_each(|(i, row)| {
                let y = y0 + i;
                let dy = (y as f32 + 0.5 - warp.cy) / ry;
                for x in x0..x1 {
                    let dx = (x as f32 + 0.5 - warp.cx) / rx;
                    let w = weight((dx * dx + dy * dy).sqrt());
                    if w <= 0.0 {
                        continue;
                    }
                    let sy = y as f32 + warp.lift * w;
                    let (top, t) = (sy.floor() as usize, sy.fract());
                    let top = top.min(src_y1 - 1);
                    let bottom = (top + 1).min(src_y1 - 1);
                    let (a, b) = (row_at(top), row_at(bottom));
                    for c in 0..4 {
                        let p = x * 4 + c;
                        row[p] = (f32::from(a[p]) * (1.0 - t) + f32::from(b[p]) * t).round() as u8;
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_downward_look_is_lifted_and_only_a_little() {
        // An eye 40 px wide and 12 px open, centred at y = 100.
        assert_eq!(
            lift_for(100.0, 40.0, 12.0, 99.0),
            0.0,
            "looking at the camera"
        );
        let down = lift_for(100.0, 40.0, 12.0, 103.0);
        assert!(down > 2.0 && down < 4.0, "{down}");
        assert_eq!(
            lift_for(100.0, 40.0, 12.0, 120.0),
            MAX_LIFT * 40.0,
            "never more than the limit"
        );
        assert_eq!(
            lift_for(100.0, 40.0, 3.0, 103.0),
            0.0,
            "a closed eye is left alone"
        );
    }

    #[test]
    fn the_pupil_moves_up_and_the_rest_of_the_face_stays() {
        // A dark 4 px pupil at y = 104 in a light 80 × 60 picture.
        let mut frame = VideoFrame::new_bgra(80, 60, 0);
        frame.data.fill(200);
        for y in 32..36 {
            for x in 38..42 {
                frame.set_pixel(x, y, [20, 20, 20, 255]);
            }
        }
        let before = frame.clone();
        apply(
            &mut frame,
            &[EyeWarp {
                cx: 40.0,
                cy: 30.0,
                width: 40.0,
                lift: 3.0,
            }],
        );
        assert_eq!(frame.pixel(40, 30)[0], 20, "the pupil is now higher");
        assert_eq!(frame.pixel(40, 35)[0], 200, "and gone from where it was");
        assert_eq!(
            frame.pixel(2, 2),
            before.pixel(2, 2),
            "outside the eye nothing moves"
        );
    }

    #[test]
    fn both_eyes_move_together_and_stale_eyes_are_ignored() {
        let eye = |cx: f32, open: f32| Eye {
            cx,
            cy: 0.4,
            width: 0.04,
            height: open,
            pupil_x: cx,
            pupil_y: 0.41,
        };
        let rect = CropRect {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 720.0,
        };
        let mut contact = EyeContact::new();
        contact.observe(
            &[EyePair {
                left: eye(0.45, 0.02),
                right: eye(0.55, 0.002),
            }],
            0,
        );
        let warps = contact.step((1280, 720), &rect, (1280, 720), false, 0);
        assert!(
            warps.iter().all(|w| w.lift == 0.0),
            "one eye closed: neither moves"
        );
        contact.observe(
            &[EyePair {
                left: eye(0.45, 0.02),
                right: eye(0.55, 0.02),
            }],
            33_333,
        );
        let mut warps = Vec::new();
        for i in 1..20u64 {
            warps = contact.step((1280, 720), &rect, (1280, 720), false, 33_333 + i * 10_000);
        }
        assert!(warps[0].lift > 1.0 && (warps[0].lift - warps[1].lift).abs() < 1e-4);
        assert!(
            contact
                .step((1280, 720), &rect, (1280, 720), false, 2_000_000)
                .is_empty()
        );
    }
}
