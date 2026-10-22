//! A tiny queue that holds the newest camera frames and never blocks the producer.

// Only the macOS capture uses it outside of tests.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use studio_core::frame::VideoFrame;

/// How many frames wait in the queue. Two is enough to smooth over a late reader without
/// adding noticeable delay.
pub(crate) const CAPACITY: usize = 2;

#[derive(Default)]
struct State {
    frames: VecDeque<VideoFrame>,
    dropped: u64,
}

/// Holds up to [`CAPACITY`] frames. When it is full, the oldest frame is thrown away, so
/// the reader always gets recent pictures and the camera thread never has to wait.
#[derive(Default)]
pub(crate) struct FrameQueue {
    state: Mutex<State>,
    ready: Condvar,
}

impl FrameQueue {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A poisoned lock only means another thread panicked while holding it. The
        // frames inside are still fine to use.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Adds a frame. Returns `true` if an older frame had to be dropped to make room.
    pub(crate) fn push(&self, frame: VideoFrame) -> bool {
        let mut state = self.lock();
        let dropped = state.frames.len() >= CAPACITY;
        if dropped {
            state.frames.pop_front();
            state.dropped += 1;
        }
        state.frames.push_back(frame);
        drop(state);
        self.ready.notify_one();
        dropped
    }

    /// Takes the oldest waiting frame, waiting up to `timeout` for one to arrive.
    pub(crate) fn pop(&self, timeout: Duration) -> Option<VideoFrame> {
        let deadline = Instant::now() + timeout;
        let mut state = self.lock();
        loop {
            if let Some(frame) = state.frames.pop_front() {
                return Some(frame);
            }
            let remaining = deadline.checked_duration_since(Instant::now())?;
            if remaining.is_zero() {
                return None;
            }
            state = self
                .ready
                .wait_timeout(state, remaining)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Throws away all waiting frames.
    pub(crate) fn clear(&self) {
        self.lock().frames.clear();
    }

    /// Number of frames dropped because the reader was too slow. Kept for diagnostics and
    /// tests.
    #[allow(dead_code)]
    pub(crate) fn dropped(&self) -> u64 {
        self.lock().dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn frame(timestamp_us: u64) -> VideoFrame {
        VideoFrame::new_bgra(2, 2, timestamp_us)
    }

    #[test]
    fn keeps_the_newest_frames() {
        let queue = FrameQueue::default();
        assert!(!queue.push(frame(1)));
        assert!(!queue.push(frame(2)));
        assert!(queue.push(frame(3)));
        assert_eq!(queue.dropped(), 1);
        assert_eq!(queue.pop(Duration::ZERO).unwrap().timestamp_us, 2);
        assert_eq!(queue.pop(Duration::ZERO).unwrap().timestamp_us, 3);
        assert!(queue.pop(Duration::ZERO).is_none());
    }

    #[test]
    fn pop_times_out_when_empty() {
        let queue = FrameQueue::default();
        let start = Instant::now();
        assert!(queue.pop(Duration::from_millis(30)).is_none());
        assert!(start.elapsed() >= Duration::from_millis(25));
    }

    #[test]
    fn pop_wakes_up_when_a_frame_arrives() {
        let queue = Arc::new(FrameQueue::default());
        let producer = Arc::clone(&queue);
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            producer.push(frame(7));
        });
        let got = queue.pop(Duration::from_secs(5));
        handle.join().unwrap();
        assert_eq!(got.unwrap().timestamp_us, 7);
    }

    #[test]
    fn clear_empties_the_queue() {
        let queue = FrameQueue::default();
        queue.push(frame(1));
        queue.clear();
        assert!(queue.pop(Duration::ZERO).is_none());
    }
}
