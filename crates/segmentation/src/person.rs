//! Native person masks. Instances belong to the serial thread that creates them.
use crate::Mask;
use studio_core::frame::VideoFrame;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Quality {
    #[default]
    Balanced,
    Accurate,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Native person segmentation is unsupported on this platform")]
    Unsupported,
    #[error("Invalid image: {0}")]
    InvalidImage(String),
    #[error("Vision failed: {0}")]
    Vision(String),
}

pub trait PersonSegmenter {
    fn reset(&mut self) -> Result<(), Error> {
        Ok(())
    }
    /// Encoded JPEG/PNG in upright, unmirrored source coordinates. No EXIF rotation.
    fn segment(&mut self, encoded: &[u8], quality: Quality) -> Result<Mask, Error>;
    /// A raw BGRA camera frame, upright and unmirrored. The mask may be smaller than the frame.
    fn segment_frame(&mut self, _frame: &VideoFrame, _quality: Quality) -> Result<Mask, Error> {
        Err(Error::Unsupported)
    }
}

#[cfg(target_os = "macos")]
pub use crate::vision::VisionSegmenter;

#[cfg(not(target_os = "macos"))]
pub struct VisionSegmenter;
#[cfg(not(target_os = "macos"))]
impl VisionSegmenter {
    pub fn new() -> Result<Self, Error> {
        Err(Error::Unsupported)
    }
}
#[cfg(not(target_os = "macos"))]
impl PersonSegmenter for VisionSegmenter {
    fn segment(&mut self, _: &[u8], _: Quality) -> Result<Mask, Error> {
        Err(Error::Unsupported)
    }
}
