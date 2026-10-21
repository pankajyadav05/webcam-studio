//! Traits for virtual camera and microphone devices.

use crate::error::Result;
use crate::frame::{AudioChunk, VideoFrame};

/// Video size and frame rate a virtual camera should output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputFormat {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Frames per second.
    pub fps: u32,
}

/// A virtual camera that other apps can select as a video source.
pub trait VirtualCamera: Send {
    /// Name shown to other apps.
    fn name(&self) -> &str;
    /// Starts the camera with the given output format.
    fn start(&mut self, format: OutputFormat) -> Result<()>;
    /// Sends one frame to the camera.
    fn push_frame(&mut self, frame: &VideoFrame) -> Result<()>;
    /// Stops the camera.
    fn stop(&mut self) -> Result<()>;
}

/// A virtual microphone that other apps can select as an audio source.
pub trait VirtualMicrophone: Send {
    /// Name shown to other apps.
    fn name(&self) -> &str;
    /// Starts the microphone with the given sample rate and channel count.
    fn start(&mut self, sample_rate: u32, channels: u16) -> Result<()>;
    /// Sends one chunk of audio to the microphone.
    fn push_audio(&mut self, chunk: &AudioChunk) -> Result<()>;
    /// Stops the microphone.
    fn stop(&mut self) -> Result<()>;
}
