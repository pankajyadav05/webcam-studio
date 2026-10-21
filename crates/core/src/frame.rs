//! Video frames and audio chunks.

/// Pixel layout of a video frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PixelFormat {
    /// Blue, green, red and alpha, 8 bits each.
    Bgra8,
}

impl PixelFormat {
    /// Number of bytes one pixel takes up in this format.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Bgra8 => 4,
        }
    }
}

/// One video frame with its pixel data.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bytes from the start of one row to the start of the next.
    pub stride: usize,
    /// Pixel layout of `data`.
    pub format: PixelFormat,
    /// Raw pixel bytes, `stride * height` long.
    pub data: Vec<u8>,
    /// Capture time in microseconds.
    pub timestamp_us: u64,
}

impl VideoFrame {
    /// Creates a black BGRA frame with no row padding.
    pub fn new_bgra(width: u32, height: u32, timestamp_us: u64) -> Self {
        let stride = width as usize * 4;
        Self {
            width,
            height,
            stride,
            format: PixelFormat::Bgra8,
            data: vec![0; stride * height as usize],
            timestamp_us,
        }
    }

    /// Returns the pixel at `(x, y)`. Panics if the coordinates are outside the frame.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let off = self.offset(x, y);
        [
            self.data[off],
            self.data[off + 1],
            self.data[off + 2],
            self.data[off + 3],
        ]
    }

    /// Writes the pixel at `(x, y)`. Panics if the coordinates are outside the frame.
    pub fn set_pixel(&mut self, x: u32, y: u32, px: [u8; 4]) {
        let off = self.offset(x, y);
        self.data[off..off + 4].copy_from_slice(&px);
    }

    /// Byte offset of the pixel at `(x, y)`, checking the bounds first.
    fn offset(&self, x: u32, y: u32) -> usize {
        assert!(
            x < self.width && y < self.height,
            "pixel ({x}, {y}) is outside the {}x{} frame",
            self.width,
            self.height
        );
        y as usize * self.stride + x as usize * self.format.bytes_per_pixel()
    }
}

/// A short run of interleaved audio samples.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioChunk {
    /// Samples per second per channel.
    pub sample_rate: u32,
    /// Number of interleaved channels.
    pub channels: u16,
    /// Interleaved samples in the range -1.0 to 1.0.
    pub samples: Vec<f32>,
    /// Capture time in microseconds.
    pub timestamp_us: u64,
}

impl AudioChunk {
    /// Number of frames (one sample per channel each). Returns 0 when `channels` is 0.
    pub fn frames(&self) -> usize {
        match self.channels {
            0 => 0,
            channels => self.samples.len() / channels as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_round_trip() {
        let mut frame = VideoFrame::new_bgra(4, 3, 0);
        assert_eq!(frame.stride, 16);
        assert_eq!(frame.data.len(), 48);
        assert_eq!(frame.pixel(3, 2), [0, 0, 0, 0]);

        frame.set_pixel(3, 2, [1, 2, 3, 255]);
        assert_eq!(frame.pixel(3, 2), [1, 2, 3, 255]);
        assert_eq!(frame.pixel(2, 2), [0, 0, 0, 0]);
        assert_eq!(&frame.data[44..48], &[1, 2, 3, 255]);
    }

    #[test]
    #[should_panic]
    fn pixel_out_of_bounds_panics() {
        let frame = VideoFrame::new_bgra(4, 3, 0);
        let _ = frame.pixel(4, 0);
    }

    #[test]
    fn audio_frames_counts_per_channel() {
        let chunk = AudioChunk {
            sample_rate: 48_000,
            channels: 2,
            samples: vec![0.0; 10],
            timestamp_us: 0,
        };
        assert_eq!(chunk.frames(), 5);
    }
}
