//! In-memory virtual devices that do not talk to the operating system.
//!
//! They check what they are given and count it. They are useful for tests and for
//! running the app without a real virtual camera or microphone installed.

use studio_core::device::{OutputFormat, VirtualCamera, VirtualMicrophone};
use studio_core::error::{Result, StudioError};
use studio_core::frame::{AudioChunk, VideoFrame};

/// A virtual camera that only counts the frames it receives.
#[derive(Debug)]
pub struct NullCamera {
    name: String,
    format: Option<OutputFormat>,
    frames: u64,
}

impl NullCamera {
    /// Creates a stopped camera called `name`.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            format: None,
            frames: 0,
        }
    }

    /// Total frames accepted since creation. Stopping does not reset it.
    pub fn frames_received(&self) -> u64 {
        self.frames
    }

    /// Whether the camera has been started and not stopped.
    pub fn is_running(&self) -> bool {
        self.format.is_some()
    }
}

impl VirtualCamera for NullCamera {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self, format: OutputFormat) -> Result<()> {
        if format.width == 0 || format.height == 0 || format.fps == 0 {
            return Err(StudioError::Invalid(format!(
                "output format {}x{} at {} fps has a zero value",
                format.width, format.height, format.fps
            )));
        }
        self.format = Some(format);
        Ok(())
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<()> {
        let Some(format) = self.format else {
            return Err(StudioError::Invalid(
                "camera is not started; call start before push_frame".to_owned(),
            ));
        };
        if frame.width != format.width || frame.height != format.height {
            return Err(StudioError::Invalid(format!(
                "frame is {}x{} but the camera was started at {}x{}",
                frame.width, frame.height, format.width, format.height
            )));
        }
        let needed = frame.stride * frame.height as usize;
        if frame.data.len() < needed {
            return Err(StudioError::Invalid(format!(
                "frame data is {} bytes but stride * height is {needed}",
                frame.data.len()
            )));
        }
        self.frames += 1;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.format = None;
        Ok(())
    }
}

/// A virtual microphone that only counts the samples it receives.
#[derive(Debug)]
pub struct NullMicrophone {
    name: String,
    /// Sample rate and channel count given to `start`, while running.
    config: Option<(u32, u16)>,
    samples: u64,
}

impl NullMicrophone {
    /// Creates a stopped microphone called `name`.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            config: None,
            samples: 0,
        }
    }

    /// Total individual samples (all channels counted) accepted since creation.
    /// Stopping does not reset it.
    pub fn samples_received(&self) -> u64 {
        self.samples
    }

    /// Whether the microphone has been started and not stopped.
    pub fn is_running(&self) -> bool {
        self.config.is_some()
    }
}

impl VirtualMicrophone for NullMicrophone {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self, sample_rate: u32, channels: u16) -> Result<()> {
        if sample_rate == 0 || channels == 0 {
            return Err(StudioError::Invalid(format!(
                "{sample_rate} Hz with {channels} channels has a zero value"
            )));
        }
        self.config = Some((sample_rate, channels));
        Ok(())
    }

    fn push_audio(&mut self, chunk: &AudioChunk) -> Result<()> {
        let Some((sample_rate, channels)) = self.config else {
            return Err(StudioError::Invalid(
                "microphone is not started; call start before push_audio".to_owned(),
            ));
        };
        if chunk.sample_rate != sample_rate || chunk.channels != channels {
            return Err(StudioError::Invalid(format!(
                "chunk is {} Hz with {} channels but the microphone was started at {sample_rate} Hz with {channels} channels",
                chunk.sample_rate, chunk.channels
            )));
        }
        if chunk.samples.len() % channels as usize != 0 {
            return Err(StudioError::Invalid(format!(
                "chunk has {} samples, which is not a whole number of {channels}-channel frames",
                chunk.samples.len()
            )));
        }
        self.samples += chunk.samples.len() as u64;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.config = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMAT: OutputFormat = OutputFormat {
        width: 8,
        height: 4,
        fps: 30,
    };

    fn chunk(sample_rate: u32, channels: u16, len: usize) -> AudioChunk {
        AudioChunk {
            sample_rate,
            channels,
            samples: vec![0.0; len],
            timestamp_us: 0,
        }
    }

    #[test]
    fn camera_start_push_stop() {
        let mut cam = NullCamera::new("Test cam");
        assert_eq!(cam.name(), "Test cam");
        assert!(!cam.is_running());

        cam.start(FORMAT).unwrap();
        assert!(cam.is_running());
        cam.push_frame(&VideoFrame::new_bgra(8, 4, 0)).unwrap();
        cam.push_frame(&VideoFrame::new_bgra(8, 4, 33_000)).unwrap();
        assert_eq!(cam.frames_received(), 2);

        cam.stop().unwrap();
        assert!(!cam.is_running());
        assert_eq!(cam.frames_received(), 2);
    }

    #[test]
    fn camera_rejects_wrong_size() {
        let mut cam = NullCamera::new("Test cam");
        cam.start(FORMAT).unwrap();
        let err = cam.push_frame(&VideoFrame::new_bgra(9, 4, 0)).unwrap_err();
        assert!(matches!(err, StudioError::Invalid(_)));
        let err = cam.push_frame(&VideoFrame::new_bgra(8, 5, 0)).unwrap_err();
        assert!(matches!(err, StudioError::Invalid(_)));
        assert_eq!(cam.frames_received(), 0);
    }

    #[test]
    fn camera_rejects_short_data() {
        let mut cam = NullCamera::new("Test cam");
        cam.start(FORMAT).unwrap();
        let mut frame = VideoFrame::new_bgra(8, 4, 0);
        frame.data.truncate(10);
        assert!(matches!(
            cam.push_frame(&frame),
            Err(StudioError::Invalid(_))
        ));
    }

    #[test]
    fn camera_push_before_start_or_after_stop_fails() {
        let mut cam = NullCamera::new("Test cam");
        let frame = VideoFrame::new_bgra(8, 4, 0);
        assert!(matches!(
            cam.push_frame(&frame),
            Err(StudioError::Invalid(_))
        ));
        cam.start(FORMAT).unwrap();
        cam.stop().unwrap();
        assert!(matches!(
            cam.push_frame(&frame),
            Err(StudioError::Invalid(_))
        ));
        assert_eq!(cam.frames_received(), 0);
    }

    #[test]
    fn camera_rejects_zero_format() {
        let mut cam = NullCamera::new("Test cam");
        let bad = OutputFormat { fps: 0, ..FORMAT };
        assert!(matches!(cam.start(bad), Err(StudioError::Invalid(_))));
        assert!(!cam.is_running());
    }

    #[test]
    fn microphone_start_push_stop() {
        let mut mic = NullMicrophone::new("Test mic");
        assert_eq!(mic.name(), "Test mic");
        assert!(!mic.is_running());

        mic.start(48_000, 2).unwrap();
        assert!(mic.is_running());
        mic.push_audio(&chunk(48_000, 2, 960)).unwrap();
        mic.push_audio(&chunk(48_000, 2, 480)).unwrap();
        assert_eq!(mic.samples_received(), 1440);

        mic.stop().unwrap();
        assert!(!mic.is_running());
        assert_eq!(mic.samples_received(), 1440);
    }

    #[test]
    fn microphone_rejects_mismatch() {
        let mut mic = NullMicrophone::new("Test mic");
        mic.start(48_000, 2).unwrap();
        assert!(matches!(
            mic.push_audio(&chunk(44_100, 2, 960)),
            Err(StudioError::Invalid(_))
        ));
        assert!(matches!(
            mic.push_audio(&chunk(48_000, 1, 960)),
            Err(StudioError::Invalid(_))
        ));
        // 3 samples cannot be split into whole stereo frames.
        assert!(matches!(
            mic.push_audio(&chunk(48_000, 2, 3)),
            Err(StudioError::Invalid(_))
        ));
        assert_eq!(mic.samples_received(), 0);
    }

    #[test]
    fn microphone_push_before_start_fails() {
        let mut mic = NullMicrophone::new("Test mic");
        assert!(matches!(
            mic.push_audio(&chunk(48_000, 2, 960)),
            Err(StudioError::Invalid(_))
        ));
        assert!(matches!(mic.start(0, 2), Err(StudioError::Invalid(_))));
        assert!(!mic.is_running());
    }
}
