//! Auto-frame: keeps the people in the picture framed and in the middle, like a camera
//! operator would. Faces come from Apple Vision a few times a second; this module turns them
//! into a zoom and pan and moves there smoothly.
//!
//! The window does the same in `ui/engine/autoframe.js`. Keep the constants and the steps the
//! same in both, so meetings frame you the way the preview does.
//!
//! - The crop is about [`FACE_HEIGHTS_TALL`] face heights tall (head and shoulders), wide
//!   enough for everyone, and never closer than [`MAX_ZOOM`].
//! - The faces sit [`FACE_LINE`] of the way down the crop, which leaves natural headroom.
//! - Small moves are ignored ([`DEAD_ZONE`], [`RESIZE_ZONE`]), so the picture stays still
//!   while you talk; bigger moves are followed with a critically damped spring.
//! - With no face for [`LOST_SECONDS`], the picture slowly goes back to the full view.

use studio_core::settings::Framing;
use studio_segmentation::FaceBox;

use crate::framing::crop_rect;
use crate::showtell::Focus;

/// The closest auto-frame zooms.
pub const MAX_ZOOM: f32 = 2.2;
/// The crop is this many face heights tall.
pub const FACE_HEIGHTS_TALL: f32 = 3.6;
/// Space left and right of the outer faces, in face heights.
pub const SIDE_SPACE: f32 = 0.9;
/// Faces smaller than this share of the biggest one (people far behind) are not framed.
pub const MIN_FACE_SHARE: f32 = 0.45;
/// Where the faces sit, as a share of the crop height from the top.
pub const FACE_LINE: f32 = 0.40;
/// The aim changes only when it moves by more than this share of the crop.
pub const DEAD_ZONE: f32 = 0.08;
/// The zoom changes only when the size changes by more than this share.
pub const RESIZE_ZONE: f32 = 0.12;
/// With no face for this long, go back to the full picture.
pub const LOST_SECONDS: f32 = 2.0;
/// Stiffness of the movement, in radians per second. About 1.2 s to settle.
pub const SPRING: f32 = 4.0;
/// Longest step of the movement, so it behaves the same at any frame rate.
const STEP_SECONDS: f32 = 1.0 / 60.0;

/// Where the crop is: its centre (shares of the picture width and height) and its size
/// (share of the largest crop; 1 is the full picture).
#[derive(Debug, Clone, Copy, PartialEq)]
struct View {
    cx: f32,
    cy: f32,
    size: f32,
}

impl View {
    fn as_array(self) -> [f32; 3] {
        [self.cx, self.cy, self.size]
    }

    fn from_array([cx, cy, size]: [f32; 3]) -> Self {
        Self { cx, cy, size }
    }
}

/// The picture geometry a view is worked out in.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    iw: f32,
    ih: f32,
    max_w: f32,
    max_h: f32,
}

impl Geometry {
    fn new(input_w: u32, input_h: u32, out_w: u32, out_h: u32) -> Option<Self> {
        if input_w == 0 || input_h == 0 {
            return None;
        }
        let full = crop_rect(input_w, input_h, out_w, out_h, &Framing::default());
        Some(Self {
            iw: input_w as f32,
            ih: input_h as f32,
            max_w: full.width,
            max_h: full.height,
        })
    }

    /// The view moved so the crop stays inside the picture.
    fn fit(&self, view: View) -> View {
        let size = view.size.clamp(1.0 / MAX_ZOOM, 1.0);
        let half_w = self.max_w * size / 2.0 / self.iw;
        let half_h = self.max_h * size / 2.0 / self.ih;
        View {
            cx: view.cx.clamp(half_w, 1.0 - half_w),
            cy: view.cy.clamp(half_h, 1.0 - half_h),
            size,
        }
    }

    fn view_of(&self, framing: &Framing, input: (u32, u32), out: (u32, u32)) -> View {
        let r = crop_rect(input.0, input.1, out.0, out.1, framing);
        View {
            cx: (r.x + r.width / 2.0) / self.iw,
            cy: (r.y + r.height / 2.0) / self.ih,
            size: r.height / self.max_h,
        }
    }

    fn framing_of(&self, view: View, start: &Framing) -> Framing {
        let (w, h) = (self.max_w * view.size, self.max_h * view.size);
        let pan = |centre: f32, length: f32, total: f32| {
            let margin = total - length;
            if margin > 0.5 {
                ((centre * total - length / 2.0) / margin * 2.0 - 1.0).clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        Framing {
            zoom: (1.0 / view.size).clamp(1.0, 4.0),
            pan_x: pan(view.cx, w, self.iw),
            pan_y: pan(view.cy, h, self.ih),
            ..start.clone()
        }
    }

    /// The view that frames `faces` (shares of the picture, top-left origin).
    fn frame_faces(&self, faces: &[FaceBox]) -> Option<View> {
        let biggest = faces.iter().map(|f| f.height).fold(0.0f32, f32::max);
        if biggest <= 0.0 {
            return None;
        }
        let kept = faces
            .iter()
            .filter(|f| f.height >= biggest * MIN_FACE_SHARE);
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for f in kept {
            x0 = x0.min(f.x * self.iw);
            y0 = y0.min(f.y * self.ih);
            x1 = x1.max((f.x + f.width) * self.iw);
            y1 = y1.max((f.y + f.height) * self.ih);
        }
        let face_h = biggest * self.ih;
        let aspect = self.max_w / self.max_h;
        let crop_h = (face_h * FACE_HEIGHTS_TALL)
            .max((x1 - x0 + 2.0 * SIDE_SPACE * face_h) / aspect)
            .max(y1 - y0 + (FACE_HEIGHTS_TALL - 1.0) * face_h);
        let size = (crop_h / self.max_h).clamp(1.0 / MAX_ZOOM, 1.0);
        let crop_h = size * self.max_h;
        Some(self.fit(View {
            cx: (x0 + x1) / 2.0 / self.iw,
            cy: ((y0 + y1) / 2.0 + (0.5 - FACE_LINE) * crop_h) / self.ih,
            size,
        }))
    }
}

/// Turns faces into a smoothly moving framing.
#[derive(Debug, Default)]
pub struct AutoFramer {
    view: Option<View>,
    velocity: [f32; 3],
    target: Option<View>,
    last_face_us: Option<u64>,
    last_us: Option<u64>,
    /// Show-and-tell: a part of the picture to show instead of the faces.
    focus: Option<Focus>,
    /// Auto-frame is off: with no focus, go back to the framing given to `step`.
    manual: bool,
}

impl AutoFramer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets everything; the next step starts from the framing it is given.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// What to aim at: the faces (auto-frame on) or the framing given to `step`, unless
    /// `focus` (Show-and-tell) asks for a part of the picture.
    pub fn set_aim(&mut self, follow_faces: bool, focus: Option<Focus>) {
        self.manual = !follow_faces;
        self.focus = focus;
    }

    /// Whether the framer is still moving the picture. With auto-frame off it stops once the
    /// picture is back at the manual framing.
    pub fn busy(&self) -> bool {
        self.view.is_some()
    }

    /// Takes in the faces found in a picture of `input_w` × `input_h` at `now_us`.
    pub fn observe(&mut self, faces: &[FaceBox], input: (u32, u32), out: (u32, u32), now_us: u64) {
        let Some(geometry) = Geometry::new(input.0, input.1, out.0, out.1) else {
            return;
        };
        let Some(wanted) = geometry.frame_faces(faces) else {
            return;
        };
        self.last_face_us = Some(now_us);
        let moved = match self.target {
            None => true,
            Some(t) => {
                let crop_w = geometry.max_w * t.size / geometry.iw;
                let crop_h = geometry.max_h * t.size / geometry.ih;
                (wanted.cx - t.cx).abs() > DEAD_ZONE * crop_w
                    || (wanted.cy - t.cy).abs() > DEAD_ZONE * crop_h
                    || (wanted.size - t.size).abs() > RESIZE_ZONE * t.size
            }
        };
        if moved {
            self.target = Some(wanted);
        }
    }

    /// Moves towards the aim and returns the framing for the picture at `now_us`. The first
    /// step starts from `start` (the framing in use before), so turning auto-frame on glides
    /// from there.
    pub fn step(
        &mut self,
        start: &Framing,
        input: (u32, u32),
        out: (u32, u32),
        now_us: u64,
    ) -> Framing {
        let Some(geometry) = Geometry::new(input.0, input.1, out.0, out.1) else {
            return Framing {
                auto_frame: true,
                ..start.clone()
            };
        };
        let view = match self.view {
            Some(view) => view,
            None => geometry.fit(geometry.view_of(start, input, out)),
        };
        let lost = self
            .last_face_us
            .is_none_or(|t| now_us.saturating_sub(t) as f32 / 1e6 > LOST_SECONDS);
        if lost && self.last_face_us.is_some() {
            self.target = Some(View {
                cx: 0.5,
                cy: 0.5,
                size: 1.0,
            });
        }
        let home = geometry.fit(geometry.view_of(start, input, out));
        let target = match self.focus {
            Some(f) => geometry.fit(View {
                cx: f.cx,
                cy: f.cy,
                size: (f.height * geometry.ih / geometry.max_h)
                    .max(f.width * geometry.iw / geometry.max_w),
            }),
            None if self.manual => home,
            None => self.target.map(|t| geometry.fit(t)).unwrap_or(view),
        };
        let mut dt = match self.last_us {
            Some(last) if now_us > last => ((now_us - last) as f32 / 1e6).min(0.25),
            _ => 0.0,
        };
        self.last_us = Some(now_us);
        let (mut x, goal) = (view.as_array(), target.as_array());
        while dt > 0.0 {
            let h = dt.min(STEP_SECONDS);
            for i in 0..3 {
                let a = SPRING * SPRING * (goal[i] - x[i]) - 2.0 * SPRING * self.velocity[i];
                self.velocity[i] += a * h;
                x[i] += self.velocity[i] * h;
            }
            dt -= h;
        }
        let view = geometry.fit(View::from_array(x));
        let resting = self.manual
            && self.focus.is_none()
            && (0..3)
                .all(|i| (x[i] - home.as_array()[i]).abs() < 2e-3 && self.velocity[i].abs() < 1e-2);
        if resting {
            // Back at the manual framing: hand over exactly, and stop.
            self.view = None;
            self.velocity = [0.0; 3];
            self.last_us = None;
            return start.clone();
        }
        self.view = Some(view);
        geometry.framing_of(view, start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IN: (u32, u32) = (1280, 720);
    const OUT: (u32, u32) = (1280, 720);

    fn face(cx: f32, cy: f32, h: f32) -> FaceBox {
        // A face is a little taller than wide; in a 16:9 picture that is about h × 0.5 wide.
        let w = h * 0.5;
        FaceBox {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            width: w,
            height: h,
            confidence: 1.0,
        }
    }

    /// Runs the framer for `seconds` at 30 frames a second, seeing `faces` every 3rd frame.
    fn run(
        framer: &mut AutoFramer,
        faces: &[FaceBox],
        from_us: u64,
        seconds: f32,
    ) -> (Framing, u64) {
        let mut framing = Framing::default();
        let frames = (seconds * 30.0) as u64;
        let mut t = from_us;
        for i in 0..frames {
            t = from_us + i * 33_333;
            if i % 3 == 0 {
                framer.observe(faces, IN, OUT, t);
            }
            framing = framer.step(&Framing::default(), IN, OUT, t);
        }
        (framing, t)
    }

    fn crop_centre(f: &Framing) -> (f32, f32, f32) {
        let r = crop_rect(IN.0, IN.1, OUT.0, OUT.1, f);
        (
            (r.x + r.width / 2.0) / 1280.0,
            (r.y + r.height / 2.0) / 720.0,
            r.height / 720.0,
        )
    }

    #[test]
    fn a_person_off_to_one_side_ends_up_framed_in_the_middle() {
        let mut framer = AutoFramer::new();
        let person = face(0.6, 0.45, 0.2);
        let (framing, _) = run(&mut framer, &[person], 0, 4.0);
        assert!(framing.zoom > 1.3, "zooms in: {}", framing.zoom);
        let (cx, cy, h) = crop_centre(&framing);
        assert!((cx - 0.6).abs() < 0.02, "centred across: {cx}");
        // The face sits FACE_LINE down the crop.
        let face_line = (0.45 - (cy - h / 2.0)) / h;
        assert!(
            (face_line - FACE_LINE).abs() < 0.03,
            "face line {face_line}"
        );
    }

    #[test]
    fn the_move_is_smooth_and_starts_from_the_framing_in_use() {
        let mut framer = AutoFramer::new();
        framer.observe(&[face(0.7, 0.4, 0.2)], IN, OUT, 0);
        let first = framer.step(&Framing::default(), IN, OUT, 0);
        assert_eq!(first.zoom, 1.0, "no jump on the first frame");
        let mut last = first.zoom;
        for i in 1..30 {
            let f = framer.step(&Framing::default(), IN, OUT, i * 33_333);
            assert!(f.zoom - last < 0.08, "a frame moves only a little");
            assert!(f.zoom >= last - 1e-4, "no overshoot back and forth");
            last = f.zoom;
        }
    }

    #[test]
    fn small_moves_are_ignored_and_big_ones_followed() {
        let mut framer = AutoFramer::new();
        let (settled, t) = run(&mut framer, &[face(0.5, 0.4, 0.2)], 0, 4.0);
        let (still, t) = run(&mut framer, &[face(0.51, 0.41, 0.205)], t, 2.0);
        let (a, b) = (crop_centre(&still), crop_centre(&settled));
        assert!(
            (a.0 - b.0).abs() < 0.002 && (a.1 - b.1).abs() < 0.002 && (a.2 - b.2).abs() < 0.002,
            "a small move keeps the picture still: {a:?} vs {b:?}"
        );
        let (moved, _) = run(&mut framer, &[face(0.6, 0.4, 0.2)], t, 4.0);
        assert!((crop_centre(&moved).0 - 0.6).abs() < 0.02);
    }

    #[test]
    fn two_people_are_both_in_the_picture() {
        let mut framer = AutoFramer::new();
        let (framing, _) = run(
            &mut framer,
            &[face(0.3, 0.4, 0.18), face(0.7, 0.42, 0.17)],
            0,
            4.0,
        );
        let r = crop_rect(IN.0, IN.1, OUT.0, OUT.1, &framing);
        assert!(r.x / 1280.0 < 0.3 - 0.045 && (r.x + r.width) / 1280.0 > 0.7 + 0.045);
    }

    #[test]
    fn a_tiny_face_never_zooms_past_the_limit() {
        let mut framer = AutoFramer::new();
        let (framing, _) = run(&mut framer, &[face(0.5, 0.5, 0.03)], 0, 5.0);
        assert!((framing.zoom - MAX_ZOOM).abs() < 0.01, "{}", framing.zoom);
    }

    #[test]
    fn people_far_behind_are_not_framed() {
        let mut framer = AutoFramer::new();
        let (alone, _) = run(&mut framer, &[face(0.4, 0.4, 0.22)], 0, 4.0);
        let mut framer = AutoFramer::new();
        let (with_passer_by, _) = run(
            &mut framer,
            &[face(0.4, 0.4, 0.22), face(0.9, 0.3, 0.05)],
            0,
            4.0,
        );
        let (a, b) = (crop_centre(&alone), crop_centre(&with_passer_by));
        assert!(
            (a.0 - b.0).abs() < 1e-4 && (a.2 - b.2).abs() < 1e-4,
            "{a:?} vs {b:?}"
        );
    }

    #[test]
    fn with_no_face_for_a_while_it_goes_back_to_the_full_picture() {
        let mut framer = AutoFramer::new();
        let (zoomed, t) = run(&mut framer, &[face(0.6, 0.4, 0.2)], 0, 4.0);
        assert!(zoomed.zoom > 1.3);
        let (held, t) = run(&mut framer, &[], t, 1.5);
        assert!(
            (held.zoom - zoomed.zoom).abs() < 0.01,
            "holds through a short look away"
        );
        let (back, _) = run(&mut framer, &[], t, 4.0);
        assert!((back.zoom - 1.0).abs() < 0.01, "{}", back.zoom);
    }

    #[test]
    fn show_and_tell_zooms_to_the_object_and_hands_back_the_manual_framing() {
        let mut framer = AutoFramer::new();
        let manual = Framing::default();
        framer.set_aim(false, None);
        assert_eq!(framer.step(&manual, IN, OUT, 0), manual);
        assert!(!framer.busy(), "nothing to do");
        let object = Focus {
            cx: 0.7,
            cy: 0.5,
            width: 0.3,
            height: 0.5,
        };
        framer.set_aim(false, Some(object));
        let mut f = manual.clone();
        for i in 0..90u64 {
            f = framer.step(&manual, IN, OUT, i * 33_333);
        }
        let (cx, cy, h) = crop_centre(&f);
        assert!(
            (cx - 0.7).abs() < 0.02 && (cy - 0.5).abs() < 0.02 && (h - 0.5).abs() < 0.03,
            "{cx} {cy} {h}"
        );
        framer.set_aim(false, None);
        for i in 90..240u64 {
            f = framer.step(&manual, IN, OUT, i * 33_333);
        }
        assert_eq!(f, manual, "exactly the manual framing again");
        assert!(!framer.busy());
    }

    #[test]
    fn the_crop_never_leaves_the_picture() {
        let mut framer = AutoFramer::new();
        let (framing, _) = run(&mut framer, &[face(0.98, 0.05, 0.15)], 0, 4.0);
        assert!(framing.pan_x <= 1.0 && framing.pan_y >= -1.0);
        let r = crop_rect(IN.0, IN.1, OUT.0, OUT.1, &framing);
        assert!(r.x + r.width <= 1280.5 && r.y >= -0.5);
    }
}
