//! Person masks for the native renderer and the browser pipeline.

pub mod person;
pub mod protocol;
#[cfg(target_os = "macos")]
mod vision;
pub use person::{PersonSegmenter, Quality, VisionSegmenter};

use studio_core::frame::VideoFrame;

/// Per-pixel person mask; 255 = person, 0 = background.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    /// Mask width in pixels.
    pub width: u32,
    /// Mask height in pixels.
    pub height: u32,
    /// One byte per pixel, row by row, `width * height` long.
    pub alpha: Vec<u8>,
}

impl Mask {
    /// Creates a mask where every pixel is the person (255).
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            alpha: vec![255; width as usize * height as usize],
        }
    }

    /// Returns the mask value at `(x, y)`. Panics if the coordinates are outside the mask.
    pub fn get(&self, x: u32, y: u32) -> u8 {
        assert!(
            x < self.width && y < self.height,
            "mask position ({x}, {y}) is outside the {}x{} mask",
            self.width,
            self.height
        );
        self.alpha[y as usize * self.width as usize + x as usize]
    }
}

/// Something that can tell which pixels of a frame show the person.
pub trait Segmenter: Send {
    /// Returns a mask for `frame`. The mask normally has the same size as the frame.
    fn segment(&mut self, frame: &VideoFrame) -> Mask;
    /// Short name of this segmenter, for logs and the settings screen.
    fn name(&self) -> &'static str;
}

/// Treats every pixel as the person. Default until a model is wired in.
#[derive(Debug, Clone, Copy, Default)]
pub struct FullForegroundSegmenter;

impl Segmenter for FullForegroundSegmenter {
    fn segment(&mut self, frame: &VideoFrame) -> Mask {
        Mask::full(frame.width, frame.height)
    }

    fn name(&self) -> &'static str {
        "full-foreground"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_mask_is_all_person() {
        let mask = Mask::full(3, 2);
        assert_eq!(mask.alpha.len(), 6);
        assert!(mask.alpha.iter().all(|&a| a == 255));
        assert_eq!(mask.get(2, 1), 255);
    }

    #[test]
    fn get_reads_row_major() {
        let mask = Mask {
            width: 2,
            height: 2,
            alpha: vec![1, 2, 3, 4],
        };
        assert_eq!(mask.get(0, 0), 1);
        assert_eq!(mask.get(1, 0), 2);
        assert_eq!(mask.get(0, 1), 3);
        assert_eq!(mask.get(1, 1), 4);
    }

    #[test]
    #[should_panic]
    fn get_outside_panics() {
        let _ = Mask::full(2, 2).get(2, 0);
    }

    #[test]
    fn full_foreground_matches_frame_size() {
        let frame = VideoFrame::new_bgra(5, 4, 0);
        let mut segmenter = FullForegroundSegmenter;
        let mask = segmenter.segment(&frame);
        assert_eq!((mask.width, mask.height), (5, 4));
        assert_eq!(mask, Mask::full(5, 4));
        assert_eq!(segmenter.name(), "full-foreground");
    }
}
