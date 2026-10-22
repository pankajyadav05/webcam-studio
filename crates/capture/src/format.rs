//! Choosing which video format of a camera to use.

// Only the macOS capture uses it outside of tests.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

/// One way a camera can deliver video: a picture size and a range of frame rates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Candidate {
    pub width: u32,
    pub height: u32,
    pub min_fps: f64,
    pub max_fps: f64,
}

/// The chosen candidate and the frame rate to ask for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pick {
    /// Position in the slice given to [`pick`].
    pub index: usize,
    /// The requested frame rate, moved into the candidate's range if it was outside.
    pub fps: f64,
}

/// How far `fps` is from the candidate's frame rate range. Zero when inside.
fn fps_distance(candidate: &Candidate, fps: f64) -> f64 {
    if fps < candidate.min_fps {
        candidate.min_fps - fps
    } else if fps > candidate.max_fps {
        fps - candidate.max_fps
    } else {
        0.0
    }
}

/// Picks the candidate closest to the wanted size, then closest to the wanted frame rate.
/// The first one wins a tie. Returns `None` for an empty list.
pub(crate) fn pick(candidates: &[Candidate], width: u32, height: u32, fps: u32) -> Option<Pick> {
    let wanted_fps = f64::from(fps);
    let key = |c: &Candidate| {
        let size = u64::from(c.width.abs_diff(width)) + u64::from(c.height.abs_diff(height));
        // Milli-frames per second as a whole number, so keys compare exactly.
        let rate = (fps_distance(c, wanted_fps) * 1000.0).round() as u64;
        (size, rate)
    };
    let (index, best) = candidates
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| key(candidate))?;
    Some(Pick {
        index,
        fps: wanted_fps.clamp(best.min_fps, best.max_fps.max(best.min_fps)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(width: u32, height: u32, min_fps: f64, max_fps: f64) -> Candidate {
        Candidate {
            width,
            height,
            min_fps,
            max_fps,
        }
    }

    #[test]
    fn exact_match_wins() {
        let list = [
            candidate(640, 480, 1.0, 30.0),
            candidate(1280, 720, 1.0, 30.0),
            candidate(1920, 1080, 1.0, 30.0),
        ];
        let pick = pick(&list, 1280, 720, 30).unwrap();
        assert_eq!(pick.index, 1);
        assert_eq!(pick.fps, 30.0);
    }

    #[test]
    fn closest_size_is_used_when_there_is_no_exact_match() {
        let list = [
            candidate(640, 480, 1.0, 30.0),
            candidate(1920, 1080, 1.0, 30.0),
        ];
        assert_eq!(pick(&list, 1600, 900, 30).unwrap().index, 1);
        assert_eq!(pick(&list, 800, 600, 30).unwrap().index, 0);
    }

    #[test]
    fn frame_rate_breaks_ties_between_equal_sizes() {
        let list = [
            candidate(1280, 720, 5.0, 15.0),
            candidate(1280, 720, 15.0, 60.0),
        ];
        let pick = pick(&list, 1280, 720, 30).unwrap();
        assert_eq!(pick.index, 1);
        assert_eq!(pick.fps, 30.0);
    }

    #[test]
    fn frame_rate_is_moved_into_range() {
        let slow = [candidate(1280, 720, 5.0, 15.0)];
        assert_eq!(pick(&slow, 1280, 720, 30).unwrap().fps, 15.0);
        let fast = [candidate(1280, 720, 60.0, 120.0)];
        assert_eq!(pick(&fast, 1280, 720, 30).unwrap().fps, 60.0);
    }

    #[test]
    fn size_beats_frame_rate() {
        let list = [
            candidate(640, 480, 1.0, 60.0),
            candidate(1280, 720, 1.0, 15.0),
        ];
        assert_eq!(pick(&list, 1280, 720, 30).unwrap().index, 1);
    }

    #[test]
    fn empty_list_gives_nothing() {
        assert!(pick(&[], 1280, 720, 30).is_none());
    }
}
