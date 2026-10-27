//! Eye contact: you read the screen, which sits below the camera, so you seem to look down.
//! A small trained network ([`crate::gaze`]) redraws each eye looking up by [`ANGLE`]
//! degrees: it moves the iris and opens the upper lid. Its warp and brightness mask are
//! applied to the full-resolution eye, faded at the edge, so the eyes stay sharp.
//!
//! It stays out of the way when it cannot help: closed or blinking eyes, a head turned away,
//! or eyes too small in the picture. It eases in and out instead of switching on a frame.

use studio_core::frame::VideoFrame;
use studio_segmentation::{Eye, EyePair};

use crate::framing::CropRect;
use crate::gaze;

/// How far the gaze moves up, in degrees: about the angle between a webcam on top of the
/// screen and the middle of the screen, seen from a normal distance.
pub const ANGLE: f32 = 13.0;
/// Eyes narrower than this many output pixels are too small to help.
pub const MIN_EYE: f32 = 14.0;
/// Eyes less open than this (lid gap / eye width) are closed or blinking.
pub const MIN_OPEN: f32 = 0.12;
/// With one eye this much smaller than the other, the head is turned too far.
pub const MIN_TURN: f32 = 0.6;
/// How fast the effect eases in and out, in seconds.
pub const EASE_SECONDS: f32 = 0.15;
/// Eyes older than this are not used.
const STALE_SECONDS: f32 = 0.25;

/// An eye in output pixels, in the network's order: the left corner, the upper lid left to
/// right, the right corner, the lower lid right to left.
type Points = [(f32, f32); 6];

/// Puts six outline points into the network's order, whatever order they came in.
pub fn canonical(points: &Points) -> Points {
    let left = (0..6)
        .min_by(|&a, &b| points[a].0.total_cmp(&points[b].0))
        .unwrap_or(0);
    let right = (0..6)
        .max_by(|&a, &b| points[a].0.total_cmp(&points[b].0))
        .unwrap_or(3);
    let (l, r) = (points[left], points[right]);
    let line_y = |x: f32| l.1 + (r.1 - l.1) * (x - l.0) / (r.0 - l.0).max(1e-3);
    let mut upper: Vec<_> = Vec::new();
    let mut lower: Vec<_> = Vec::new();
    for (i, &p) in points.iter().enumerate() {
        if i == left || i == right {
            continue;
        }
        if p.1 <= line_y(p.0) {
            upper.push(p);
        } else {
            lower.push(p);
        }
    }
    // A squinting eye can put three points on one side; split by height instead.
    if upper.len() != 2 {
        let mut rest: Vec<_> = upper.into_iter().chain(lower).collect();
        rest.sort_by(|a, b| a.1.total_cmp(&b.1));
        lower = rest.split_off(2);
        upper = rest;
    }
    upper.sort_by(|a, b| a.0.total_cmp(&b.0));
    lower.sort_by(|a, b| b.0.total_cmp(&a.0));
    [l, upper[0], upper[1], r, lower[0], lower[1]]
}

/// How open an eye is: the gap between the lids over its width.
pub fn openness(p: &Points) -> f32 {
    let width = (p[3].0 - p[0].0).abs().max(1e-3);
    let gap = ((p[5].1 - p[1].1) + (p[4].1 - p[2].1)) / 2.0;
    gap / width
}

fn to_output(
    eye: &Eye,
    input: (u32, u32),
    rect: &CropRect,
    out: (u32, u32),
    mirror: bool,
) -> Points {
    let sx = out.0 as f32 / rect.width.max(1e-3);
    let sy = out.1 as f32 / rect.height.max(1e-3);
    let mut points = [(0.0, 0.0); 6];
    for (p, [x, y]) in points.iter_mut().zip(eye.points) {
        let mut ox = (x * input.0 as f32 - rect.x) * sx;
        if mirror {
            ox = out.0 as f32 - ox;
        }
        *p = (ox, (y * input.1 as f32 - rect.y) * sy);
    }
    points
}

/// Follows the eyes and redraws them.
#[derive(Debug, Default)]
pub struct EyeContact {
    eyes: Vec<EyePair>,
    seen_us: Option<u64>,
    last_us: Option<u64>,
    strength: f32,
}

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

    /// Redraws the eyes in `frame` (the output picture, framed by `rect` from a camera picture
    /// of `input` size).
    pub fn apply(
        &mut self,
        frame: &mut VideoFrame,
        input: (u32, u32),
        rect: &CropRect,
        mirror: bool,
        now_us: u64,
    ) {
        let dt = match self.last_us {
            Some(last) if now_us > last => ((now_us - last) as f32 / 1e6).min(0.5),
            _ => 1.0 / 30.0,
        };
        self.last_us = Some(now_us);
        let out = (frame.width, frame.height);
        let fresh = self
            .seen_us
            .is_some_and(|t| now_us.saturating_sub(t) as f32 / 1e6 <= STALE_SECONDS);
        let eyes = if fresh { &self.eyes[..] } else { &[] };
        let plans = biggest_pair(eyes, input, rect, out, mirror)
            .filter(|(l, r)| usable(l, r))
            .and_then(|(l, r)| plan_pair(frame, &l, &r));
        // Eyes it cannot help are left alone straight away; good ones ease back in.
        let Some(plans) = plans else {
            self.strength = 0.0;
            return;
        };
        self.strength += (1.0 - self.strength) * (1.0 - (-dt / EASE_SECONDS).exp());
        for plan in &plans {
            draw(frame, plan, self.strength);
        }
    }
}

/// The biggest face's eyes in output pixels, left to right, in the network's order.
pub fn biggest_pair(
    eyes: &[EyePair],
    input: (u32, u32),
    rect: &CropRect,
    out: (u32, u32),
    mirror: bool,
) -> Option<(Points, Points)> {
    eyes.iter()
        .map(|pair| {
            let a = canonical(&to_output(&pair.left, input, rect, out, mirror));
            let b = canonical(&to_output(&pair.right, input, rect, out, mirror));
            if a[0].0 <= b[0].0 { (a, b) } else { (b, a) }
        })
        .max_by(|x, y| (x.0[3].0 - x.0[0].0).total_cmp(&(y.0[3].0 - y.0[0].0)))
}

/// Whether the network can help these eyes: big enough, open, and the head not turned away.
pub fn usable(left: &Points, right: &Points) -> bool {
    let (wl, wr) = (left[3].0 - left[0].0, right[3].0 - right[0].0);
    wl.min(wr) >= MIN_EYE
        && wl.min(wr) / wl.max(wr) >= MIN_TURN
        && openness(left) >= MIN_OPEN
        && openness(right) >= MIN_OPEN
}

/// What to do to one eye: its crop box in the picture and the network's maps.
#[derive(Debug, Clone)]
pub struct EyePlan {
    pub top: usize,
    pub left: usize,
    pub height: usize,
    pub width: usize,
    /// The box is in the mirrored picture: read and write it mirrored.
    pub mirrored: bool,
    pub warp: gaze::EyeWarp,
}

/// Runs the network for both eyes of `frame` (points in its pixels, left eye first).
pub fn plan_pair(frame: &VideoFrame, left: &Points, right: &Points) -> Option<[EyePlan; 2]> {
    // The network knows the left eye; the right one goes through it mirrored.
    let w = frame.width as f32 - 1.0;
    let mirrored = canonical(&right.map(|(x, y)| (w - x, y)));
    Some([plan(frame, left, false)?, plan(frame, &mirrored, true)?])
}

/// The crop around an eye, as the network was trained: 1.5 eye widths across, 2/3 as tall,
/// a little more above the corners than below. (top, left, height, width) in pixels.
fn eye_box(p: &Points) -> (i32, i32, usize, usize) {
    let (cx, cy) = ((p[0].0 + p[3].0) / 2.0, (p[0].1 + p[3].1) / 2.0);
    let half_w = (p[3].0 - p[0].0).abs() * 0.75;
    let h = 1.5 * half_w;
    let top = (cy - h * 7.0 / 12.0) as i32;
    let left = (cx - half_w) as i32;
    let bottom = (cy + h * 5.0 / 12.0) as i32;
    let right = (cx + half_w) as i32;
    (
        top,
        left,
        (bottom - top).max(0) as usize,
        (right - left).max(0) as usize,
    )
}

/// Bilinear sample of an RGB float image (h × w × 3), clamped at the edges.
fn sample(img: &[f32], h: usize, w: usize, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let at = |yy: usize, xx: usize, c: usize| img[(yy * w + xx) * 3 + c];
    let mut out = [0.0; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let top = at(y0, x0, c) + (at(y0, x1, c) - at(y0, x0, c)) * tx;
        let bottom = at(y1, x0, c) + (at(y1, x1, c) - at(y1, x0, c)) * tx;
        *o = top + (bottom - top) * ty;
    }
    out
}

/// Redraws one eye of `frame`. With `mirrored`, the points are in a mirrored picture and the
/// eye is read and written mirrored.
/// Picture x of crop column `x`, through the mirror when asked.
fn column(frame: &VideoFrame, left: usize, x: usize, mirrored: bool) -> usize {
    if mirrored {
        frame.width as usize - 1 - (left + x)
    } else {
        left + x
    }
}

/// The crop as RGB floats (the frame is BGRA).
fn read_crop(
    frame: &VideoFrame,
    top: usize,
    left: usize,
    oh: usize,
    ow: usize,
    mirrored: bool,
) -> Vec<f32> {
    let mut crop = vec![0f32; oh * ow * 3];
    for y in 0..oh {
        let row = &frame.data[(top + y) * frame.stride..];
        for x in 0..ow {
            let px = &row[column(frame, left, x, mirrored) * 4..];
            let at = (y * ow + x) * 3;
            crop[at] = f32::from(px[2]) / 255.0;
            crop[at + 1] = f32::from(px[1]) / 255.0;
            crop[at + 2] = f32::from(px[0]) / 255.0;
        }
    }
    crop
}

/// Runs the network for one eye (points in the picture, or in the mirrored picture).
pub fn plan(frame: &VideoFrame, p: &Points, mirrored: bool) -> Option<EyePlan> {
    let (top, left, oh, ow) = eye_box(p);
    let (fw, fh) = (frame.width as i32, frame.height as i32);
    if oh < 4 || ow < 4 || top < 0 || left < 0 || top + oh as i32 > fh || left + ow as i32 > fw {
        return None;
    }
    let crop = read_crop(frame, top as usize, left as usize, oh, ow, mirrored);
    // The network's input: the crop at 48 × 64, and the points in its pixels.
    let (sx, sy) = (gaze::W as f32 / ow as f32, gaze::H as f32 / oh as f32);
    let mut small = Vec::with_capacity(gaze::H * gaze::W * 3);
    for y in 0..gaze::H {
        for x in 0..gaze::W {
            let s = sample(
                &crop,
                oh,
                ow,
                (x as f32 + 0.5) / sx - 0.5,
                (y as f32 + 0.5) / sy - 0.5,
            );
            small.extend(s);
        }
    }
    let mut points = [(0.0, 0.0); 6];
    for (q, &(x, y)) in points.iter_mut().zip(p) {
        *q = ((x - left as f32) * sx, (y - top as f32) * sy);
    }
    let warp = gaze::infer(&small, &gaze::point_maps(&points), [ANGLE, 0.0]);
    Some(EyePlan {
        top: top as usize,
        left: left as usize,
        height: oh,
        width: ow,
        mirrored,
        warp,
    })
}

/// Draws a planned eye: warps the crop at full size, keeps the network's brightness, and
/// fades out at the edge of the box. `strength` 0..1 mixes it in.
pub fn draw(frame: &mut VideoFrame, plan: &EyePlan, strength: f32) {
    let (top, left, oh, ow, mirrored) =
        (plan.top, plan.left, plan.height, plan.width, plan.mirrored);
    if top + oh > frame.height as usize
        || column(frame, left, ow - 1, mirrored) >= frame.width as usize
    {
        return;
    }
    let crop = read_crop(frame, top, left, oh, ow, mirrored);
    let warp = &plan.warp;
    for y in 0..oh {
        let row = (top + y) * frame.stride;
        let gy = if oh > 1 {
            -1.0 + 2.0 * y as f32 / (oh - 1) as f32
        } else {
            0.0
        };
        for x in 0..ow {
            let gx = if ow > 1 {
                -1.0 + 2.0 * x as f32 / (ow - 1) as f32
            } else {
                0.0
            };
            let fade =
                ((1.0 - gx.abs().powi(4).max(gy.abs().powi(4))) * 3.0).clamp(0.0, 1.0) * strength;
            if fade <= 0.0 {
                continue;
            }
            // The network's maps, read at this point of the crop.
            let mx = ((x as f32 + 0.5) * gaze::W as f32 / ow as f32 - 0.5)
                .clamp(0.0, (gaze::W - 1) as f32);
            let my = ((y as f32 + 0.5) * gaze::H as f32 / oh as f32 - 0.5)
                .clamp(0.0, (gaze::H - 1) as f32);
            let (fx, fy, keep) = bilinear_maps(warp, mx, my);
            let src = sample(
                &crop,
                oh,
                ow,
                0.5 * (gx + fx + 1.0) * ow as f32,
                0.5 * (gy + fy + 1.0) * oh as f32,
            );
            let orig = &crop[(y * ow + x) * 3..][..3];
            let col = column(frame, left, x, mirrored);
            let px = &mut frame.data[row + col * 4..][..3];
            for (c, &bgr) in [2usize, 1, 0].iter().enumerate() {
                let redrawn = src[c] * keep + (1.0 - keep);
                let value = orig[c] + (redrawn - orig[c]) * fade;
                px[bgr] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    }
}

fn bilinear_maps(w: &gaze::EyeWarp, x: f32, y: f32) -> (f32, f32, f32) {
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(gaze::W - 1), (y0 + 1).min(gaze::H - 1));
    let (tx, ty) = (x - x0 as f32, y - y0 as f32);
    let mix = |f: &dyn Fn(usize) -> f32| {
        let i = |yy: usize, xx: usize| yy * gaze::W + xx;
        let top = f(i(y0, x0)) + (f(i(y0, x1)) - f(i(y0, x0))) * tx;
        let bottom = f(i(y1, x0)) + (f(i(y1, x1)) - f(i(y1, x0))) * tx;
        top + (bottom - top) * ty
    };
    (
        mix(&|i| w.flow[i * 2]),
        mix(&|i| w.flow[i * 2 + 1]),
        mix(&|i| w.keep[i]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An eye 40 px wide centred at (cx, cy), open by `open` of its width, in Vision's order
    /// for the eye on the left of the picture.
    fn points(cx: f32, cy: f32, open: f32) -> Points {
        let (w, g) = (40.0, 40.0 * open / 2.0);
        [
            (cx - w / 2.0, cy),
            (cx - w / 6.0, cy - g),
            (cx + w / 6.0, cy - g),
            (cx + w / 2.0, cy),
            (cx + w / 6.0, cy + g),
            (cx - w / 6.0, cy + g),
        ]
    }

    #[test]
    fn points_come_out_in_the_networks_order_whatever_order_they_came_in() {
        let p = points(100.0, 50.0, 0.3);
        let shuffled = [p[3], p[5], p[0], p[2], p[4], p[1]];
        assert_eq!(canonical(&shuffled), p);
        assert!((openness(&p) - 0.3).abs() < 1e-4);
    }

    fn face_frame() -> VideoFrame {
        // Skin with two dark eyes at y = 60, x = 70 and 150, in a 220 × 120 picture.
        let mut frame = VideoFrame::new_bgra(220, 120, 0);
        for y in 0..120 {
            for x in 0..220 {
                let dark = [70u32, 150]
                    .iter()
                    .any(|&c| (x as i32 - c as i32).abs() < 8 && (y as i32 - 60).abs() < 5);
                frame.set_pixel(
                    x,
                    y,
                    if dark {
                        [40, 40, 50, 255]
                    } else {
                        [150, 170, 200, 255]
                    },
                );
            }
        }
        frame
    }

    fn eyes(open: f32) -> EyePair {
        let to_share = |p: Points| Eye {
            points: p.map(|(x, y)| [x / 220.0, y / 120.0]),
        };
        EyePair {
            left: to_share(points(70.0, 60.0, open)),
            right: to_share(points(150.0, 60.0, open)),
        }
    }

    const WHOLE: CropRect = CropRect {
        x: 0.0,
        y: 0.0,
        width: 220.0,
        height: 120.0,
    };

    #[test]
    fn open_eyes_are_redrawn_and_the_rest_of_the_face_is_left_alone() {
        let mut contact = EyeContact::new();
        let mut frame = face_frame();
        let before = frame.clone();
        for i in 0..4u64 {
            frame = before.clone();
            contact.observe(&[eyes(0.3)], i * 33_333);
            contact.apply(&mut frame, (220, 120), &WHOLE, false, i * 33_333);
        }
        let changed = |x0: u32, x1: u32| {
            (x0..x1)
                .flat_map(|x| (40..80).map(move |y| (x, y)))
                .any(|(x, y)| frame.pixel(x, y) != before.pixel(x, y))
        };
        assert!(
            changed(40, 100) && changed(120, 180),
            "both eyes are redrawn"
        );
        assert_eq!(
            frame.pixel(110, 10),
            before.pixel(110, 10),
            "the forehead is not"
        );
        assert_eq!(
            frame.pixel(110, 110),
            before.pixel(110, 110),
            "nor the chin"
        );
    }

    #[test]
    fn closed_eyes_and_old_eyes_are_left_alone() {
        let mut contact = EyeContact::new();
        let before = face_frame();
        let mut frame = before.clone();
        for i in 0..4u64 {
            frame = before.clone();
            contact.observe(&[eyes(0.04)], i * 33_333);
            contact.apply(&mut frame, (220, 120), &WHOLE, false, i * 33_333);
        }
        assert_eq!(frame, before, "closed eyes");
        let mut contact = EyeContact::new();
        contact.observe(&[eyes(0.3)], 0);
        let mut frame = before.clone();
        contact.apply(&mut frame, (220, 120), &WHOLE, false, 2_000_000);
        assert_eq!(frame, before, "eyes from two seconds ago");
    }
}
