//! Studio Light: lights the face and dims the room around it, like a soft key light.
//!
//! Faces come from Apple Vision a few times a second (the same ones auto-frame uses). The
//! light follows them with a short ease, and fades away when no face is seen, so the room is
//! never dimmed for nobody. The window does the same in `ui/engine/look.js` and
//! `compositor.js`; keep the numbers the same in both.

use rayon::prelude::*;
use studio_core::frame::VideoFrame;
use studio_segmentation::FaceBox;

use crate::appearance::light_curve;
use crate::framing::CropRect;

/// How much the room is darkened at full strength.
pub const DIM: f32 = 0.45;
/// The extra lift of the lit area at full strength (the light curve's gain is 1 + this).
pub const LIFT: f32 = 0.7;
/// The lit area, in face heights: across from the edge of the outer faces, and up and down.
pub const REACH: (f32, f32) = (1.0, 2.0);
/// The centre of the light sits this many face heights below the centre of the face.
pub const DROP: f32 = 0.35;
/// The light is full inside this share of its reach and gone beyond the second number.
pub const FALLOFF: (f32, f32) = (0.55, 1.35);
/// How fast the light follows the face, and how fast it fades in and out, in seconds.
pub const FOLLOW_SECONDS: f32 = 0.25;
pub const FADE_SECONDS: f32 = 0.5;
/// With no face for this long, the light fades away.
pub const LOST_SECONDS: f32 = 1.0;

/// Where the light is, in shares of the camera picture: centre and reach (half sizes).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    pub cx: f32,
    pub cy: f32,
    pub rx: f32,
    pub ry: f32,
}

/// The spot that lights `faces` (shares of a picture `aspect` = width / height wide).
pub fn spot_for(faces: &[FaceBox], aspect: f32) -> Option<Spot> {
    let biggest = faces.iter().map(|f| f.height).fold(0.0f32, f32::max);
    if biggest <= 0.0 || aspect <= 0.0 {
        return None;
    }
    let kept: Vec<_> = faces
        .iter()
        .filter(|f| f.height >= biggest * crate::autoframe::MIN_FACE_SHARE)
        .collect();
    let x0 = kept.iter().map(|f| f.x).fold(f32::MAX, f32::min);
    let x1 = kept.iter().map(|f| f.x + f.width).fold(f32::MIN, f32::max);
    let cy = kept.iter().map(|f| f.y + f.height / 2.0).sum::<f32>() / kept.len() as f32;
    // Face heights are shares of the picture height; across, they count `1 / aspect` as much.
    let h = biggest;
    Some(Spot {
        cx: (x0 + x1) / 2.0,
        cy: cy + DROP * h,
        rx: (x1 - x0) / 2.0 + REACH.0 * h / aspect,
        ry: REACH.1 * h,
    })
}

/// Follows the faces and lights the picture.
#[derive(Debug, Default)]
pub struct StudioLight {
    spot: Option<Spot>,
    wanted: Option<Spot>,
    presence: f32,
    last_face_us: Option<u64>,
    last_us: Option<u64>,
}

impl StudioLight {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Takes in the faces of a picture `aspect` (width / height) wide, seen at `now_us`.
    pub fn observe(&mut self, faces: &[FaceBox], aspect: f32, now_us: u64) {
        if let Some(spot) = spot_for(faces, aspect) {
            self.wanted = Some(spot);
            self.last_face_us = Some(now_us);
        }
    }

    /// Moves the light for the picture at `now_us`; returns where it is and how present
    /// (0..1) it is.
    pub fn step(&mut self, now_us: u64) -> Option<(Spot, f32)> {
        let dt = match self.last_us {
            Some(last) if now_us > last => ((now_us - last) as f32 / 1e6).min(0.5),
            Some(_) => 0.0,
            None => 1.0 / 30.0,
        };
        self.last_us = Some(now_us);
        let seen = self
            .last_face_us
            .is_some_and(|t| (now_us.saturating_sub(t) as f32 / 1e6) <= LOST_SECONDS);
        let ease = |seconds: f32| 1.0 - (-dt / seconds).exp();
        self.presence += (f32::from(u8::from(seen)) - self.presence) * ease(FADE_SECONDS);
        if let Some(wanted) = self.wanted {
            let k = ease(FOLLOW_SECONDS);
            self.spot = Some(match self.spot {
                None => wanted,
                Some(s) => Spot {
                    cx: s.cx + (wanted.cx - s.cx) * k,
                    cy: s.cy + (wanted.cy - s.cy) * k,
                    rx: s.rx + (wanted.rx - s.rx) * k,
                    ry: s.ry + (wanted.ry - s.ry) * k,
                },
            });
        }
        self.spot.map(|s| (s, self.presence))
    }
}

/// The spot in output pixels, for a picture cut by `rect` from an `input_w` × `input_h`
/// camera picture and scaled to `out_w` × `out_h` (mirrored when asked).
pub fn spot_in_output(
    spot: Spot,
    input: (u32, u32),
    rect: &CropRect,
    out: (u32, u32),
    mirror: bool,
) -> Spot {
    let (iw, ih) = (input.0 as f32, input.1 as f32);
    let sx = out.0 as f32 / rect.width.max(1e-3);
    let sy = out.1 as f32 / rect.height.max(1e-3);
    let mut cx = (spot.cx * iw - rect.x) * sx;
    if mirror {
        cx = out.0 as f32 - cx;
    }
    Spot {
        cx,
        cy: (spot.cy * ih - rect.y) * sy,
        rx: spot.rx * iw * sx,
        ry: spot.ry * ih * sy,
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much of the light reaches a pixel `d` spot-radii from the centre.
pub fn reach(d: f32) -> f32 {
    1.0 - smoothstep(FALLOFF.0, FALLOFF.1, d)
}

/// One channel (0..1) under the light: `lit` is [`reach`], `strength` and `presence` 0..1.
pub fn shade(v: f32, lit: f32, strength: f32, presence: f32) -> f32 {
    let dark = v * (1.0 - DIM * strength);
    let bright = light_curve(v, 1.0 + LIFT * strength);
    let lighted = dark + (bright - dark) * lit;
    v + (lighted - v) * presence
}

/// Lights `frame` in place; `spot` is in output pixels.
pub fn apply(frame: &mut VideoFrame, spot: Spot, strength: f32, presence: f32) {
    if strength <= 0.001 || presence <= 0.001 || spot.rx <= 0.0 || spot.ry <= 0.0 {
        return;
    }
    let (w, h) = (frame.width as usize, frame.height as usize);
    // Two tables: fully lit and fully dark; pixels in between mix them.
    let mut lit = [0f32; 256];
    let mut dark = [0f32; 256];
    for v in 0..256 {
        let x = v as f32 / 255.0;
        lit[v] = shade(x, 1.0, strength, presence) * 255.0;
        dark[v] = shade(x, 0.0, strength, presence) * 255.0;
    }
    frame
        .data
        .par_chunks_mut(frame.stride)
        .take(h)
        .enumerate()
        .for_each(|(y, row)| {
            let dy = (y as f32 + 0.5 - spot.cy) / spot.ry;
            for x in 0..w {
                let dx = (x as f32 + 0.5 - spot.cx) / spot.rx;
                let k = reach((dx * dx + dy * dy).sqrt());
                for c in &mut row[x * 4..x * 4 + 3] {
                    let v = *c as usize;
                    *c = (dark[v] + (lit[v] - dark[v]) * k).round().clamp(0.0, 255.0) as u8;
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face(cx: f32, cy: f32, h: f32) -> FaceBox {
        FaceBox {
            x: cx - h * 0.28,
            y: cy - h / 2.0,
            width: h * 0.56,
            height: h,
            confidence: 1.0,
        }
    }

    #[test]
    fn the_face_gets_brighter_and_the_room_darker() {
        let mut frame = VideoFrame::new_bgra(64, 36, 0);
        frame.data.fill(100);
        let spot = Spot {
            cx: 32.0,
            cy: 18.0,
            rx: 10.0,
            ry: 10.0,
        };
        apply(&mut frame, spot, 1.0, 1.0);
        assert!(
            frame.pixel(32, 18)[1] > 120,
            "lit {}",
            frame.pixel(32, 18)[1]
        );
        assert!(frame.pixel(1, 1)[1] < 70, "dimmed {}", frame.pixel(1, 1)[1]);
        assert_eq!(frame.pixel(1, 1)[3], 100, "alpha is left alone");
    }

    #[test]
    fn without_a_face_nothing_changes() {
        assert_eq!(shade(0.4, 0.0, 1.0, 0.0), 0.4);
        let mut light = StudioLight::new();
        assert!(light.step(0).is_none());
        light.observe(&[face(0.5, 0.4, 0.2)], 16.0 / 9.0, 0);
        let (_, presence) = light.step(33_333).unwrap();
        assert!(presence > 0.0 && presence < 0.2, "fades in: {presence}");
        // Two seconds without a face: the light has faded away.
        let mut last = 1.0;
        for i in 2..80u64 {
            last = light.step(i * 33_333).unwrap().1;
        }
        assert!(last < 0.05, "{last}");
    }

    #[test]
    fn the_light_sits_on_the_face_and_follows_it() {
        let mut light = StudioLight::new();
        light.observe(&[face(0.3, 0.4, 0.2)], 16.0 / 9.0, 0);
        let (first, _) = light.step(0).unwrap();
        assert!((first.cx - 0.3).abs() < 1e-4 && first.cy > 0.4);
        light.observe(&[face(0.7, 0.4, 0.2)], 16.0 / 9.0, 33_333);
        let (moving, _) = light.step(33_333).unwrap();
        assert!(
            moving.cx > 0.3 && moving.cx < 0.5,
            "eases, no jump: {}",
            moving.cx
        );
    }

    #[test]
    fn the_spot_follows_the_crop_and_the_mirror() {
        let spot = Spot {
            cx: 0.2,
            cy: 0.5,
            rx: 0.1,
            ry: 0.2,
        };
        let rect = CropRect {
            x: 0.0,
            y: 0.0,
            width: 640.0,
            height: 360.0,
        };
        let out = spot_in_output(spot, (1280, 720), &rect, (1280, 720), false);
        assert_eq!(
            (out.cx, out.cy, out.rx, out.ry),
            (512.0, 720.0, 256.0, 288.0)
        );
        let mirrored = spot_in_output(spot, (1280, 720), &rect, (1280, 720), true);
        assert_eq!(mirrored.cx, 768.0);
    }
}
