//! Show-and-tell: hold something up to the camera and the picture zooms in on it; lower it
//! and the picture goes back. The window does the same in `ui/engine/showtell.js`; keep the
//! numbers the same in both.
//!
//! What counts as "holding something up", measured against the person's own face so it
//! works near or far from the camera: a hand at least about half as big as the face, raised
//! to the upper body (not at the desk), not on the face (a chin in the hand does not count),
//! and held still for a moment. Hands that wave or talk keep moving, so they do not trigger it.

use studio_segmentation::{FaceBox, HandBox};

/// A held-up hand is at least this share of the face's height.
pub const MIN_HAND: f32 = 0.5;
/// ...and its centre is at most this many face heights below the bottom of the face.
pub const REACH_DOWN: f32 = 1.6;
/// With no face in view, a hand must be at least this tall (share of the picture height),
/// and its centre higher than this (share of the picture height).
pub const MIN_HAND_ALONE: f32 = 0.12;
pub const DESK_LINE: f32 = 0.72;
/// How long the hand must stay still before the picture zooms in, in seconds.
pub const HOLD_SECONDS: f32 = 0.7;
/// "Still": the hand's centre moves less than this share of the face height (or of the
/// picture height × 0.2 with no face).
pub const STILL: f32 = 0.35;
/// With no hand held up for this long, the picture goes back, in seconds.
pub const RELEASE_SECONDS: f32 = 0.8;
/// The object is about this many times the size of the hands holding it.
pub const SPREAD: f32 = 2.6;
/// The object usually sits above the fingers: the view moves up this share of its height.
pub const LIFT: f32 = 0.15;
/// While showing, the view moves only when the object moves more than this share of it.
pub const DEAD_ZONE: f32 = 0.12;

/// The part of the picture to show, in shares of the picture (centre and size).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Focus {
    pub cx: f32,
    pub cy: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Default)]
pub struct ShowTell {
    anchor: Option<(f32, f32)>,
    since_us: Option<u64>,
    focus: Option<Focus>,
    last_seen_us: Option<u64>,
}

fn secs(from: u64, to: u64) -> f32 {
    to.saturating_sub(from) as f32 / 1e6
}

impl ShowTell {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The part of the picture to show now, if something is held up.
    pub fn focus(&self) -> Option<Focus> {
        self.focus
    }

    /// Takes in the hands and faces seen at `now_us`.
    pub fn observe(&mut self, hands: &[HandBox], faces: &[FaceBox], now_us: u64) {
        let face = faces.iter().max_by(|a, b| a.height.total_cmp(&b.height));
        let held: Vec<_> = hands.iter().filter(|h| held_up(h, face)).collect();
        let still = face.map_or(STILL * 0.2, |f| STILL * f.height);
        if held.is_empty() {
            self.anchor = None;
            self.since_us = None;
            if self
                .last_seen_us
                .is_none_or(|t| secs(t, now_us) > RELEASE_SECONDS)
            {
                self.focus = None;
            }
            return;
        }
        let x0 = held.iter().map(|h| h.x).fold(f32::MAX, f32::min);
        let y0 = held.iter().map(|h| h.y).fold(f32::MAX, f32::min);
        let x1 = held.iter().map(|h| h.x + h.width).fold(f32::MIN, f32::max);
        let y1 = held.iter().map(|h| h.y + h.height).fold(f32::MIN, f32::max);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (w, h) = ((x1 - x0) * SPREAD, (y1 - y0) * SPREAD);
        let wanted = Focus {
            cx,
            cy: cy - LIFT * h,
            width: w,
            height: h,
        };
        if let Some(focus) = self.focus {
            self.last_seen_us = Some(now_us);
            let moved = (wanted.cx - focus.cx).abs() > DEAD_ZONE * focus.width
                || (wanted.cy - focus.cy).abs() > DEAD_ZONE * focus.height
                || (wanted.height - focus.height).abs() > 2.0 * DEAD_ZONE * focus.height;
            if moved {
                self.focus = Some(wanted);
            }
            return;
        }
        match (self.anchor, self.since_us) {
            (Some((ax, ay)), Some(since)) if (cx - ax).abs() < still && (cy - ay).abs() < still => {
                if secs(since, now_us) >= HOLD_SECONDS {
                    self.focus = Some(wanted);
                    self.last_seen_us = Some(now_us);
                }
            }
            _ => {
                self.anchor = Some((cx, cy));
                self.since_us = Some(now_us);
            }
        }
    }
}

/// Whether a hand looks held up to show something.
fn held_up(hand: &HandBox, face: Option<&FaceBox>) -> bool {
    let (cx, cy) = (hand.x + hand.width / 2.0, hand.y + hand.height / 2.0);
    match face {
        Some(f) => {
            let on_face = cx > f.x && cx < f.x + f.width && cy > f.y && cy < f.y + f.height;
            hand.height.max(hand.width) >= MIN_HAND * f.height
                && cy <= f.y + f.height * (1.0 + REACH_DOWN)
                && !on_face
        }
        None => hand.height >= MIN_HAND_ALONE && cy < DESK_LINE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A face 0.2 tall, centred at (0.5, 0.35).
    const FACE: FaceBox = FaceBox {
        x: 0.44,
        y: 0.25,
        width: 0.12,
        height: 0.2,
        confidence: 1.0,
    };

    fn hand(cx: f32, cy: f32, size: f32) -> HandBox {
        HandBox {
            x: cx - size * 0.4,
            y: cy - size / 2.0,
            width: size * 0.8,
            height: size,
            confidence: 0.9,
        }
    }

    fn feed(show: &mut ShowTell, hands: &[HandBox], from_ms: u64, to_ms: u64) {
        for t in (from_ms..to_ms).step_by(100) {
            show.observe(hands, &[FACE], t * 1000);
        }
    }

    #[test]
    fn a_hand_held_up_and_still_zooms_in_after_a_moment() {
        let mut show = ShowTell::new();
        let up = [hand(0.7, 0.4, 0.12)];
        feed(&mut show, &up, 0, 600);
        assert!(show.focus().is_none(), "not yet");
        feed(&mut show, &up, 600, 900);
        let focus = show.focus().expect("zoomed in");
        assert!((focus.cx - 0.7).abs() < 0.1 && focus.cy < 0.4);
        assert!(focus.height > 0.25);
    }

    #[test]
    fn waving_talking_typing_and_resting_hands_do_not_trigger_it() {
        let mut show = ShowTell::new();
        for i in 0..30u64 {
            let x = if i % 2 == 0 { 0.7 } else { 0.8 };
            show.observe(&[hand(x, 0.4, 0.15)], &[FACE], i * 100_000);
        }
        assert!(show.focus().is_none(), "a moving hand");
        let mut show = ShowTell::new();
        feed(&mut show, &[hand(0.7, 0.5, 0.06)], 0, 2000);
        assert!(show.focus().is_none(), "a small hand, far from the camera");
        let mut show = ShowTell::new();
        feed(&mut show, &[hand(0.5, 0.92, 0.12)], 0, 2000);
        assert!(show.focus().is_none(), "hands at the desk");
        let mut show = ShowTell::new();
        feed(&mut show, &[hand(0.5, 0.4, 0.12)], 0, 2000);
        assert!(show.focus().is_none(), "a chin in the hand");
    }

    #[test]
    fn lowering_the_object_goes_back_after_a_short_wait() {
        let mut show = ShowTell::new();
        feed(&mut show, &[hand(0.7, 0.4, 0.15)], 0, 1000);
        assert!(show.focus().is_some());
        feed(&mut show, &[], 1000, 1500);
        assert!(
            show.focus().is_some(),
            "a dropped frame or two does not end it"
        );
        feed(&mut show, &[], 1500, 2000);
        assert!(show.focus().is_none());
    }
}
