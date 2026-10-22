//! Plain checks and pixel copying. No operating system calls, so it is tested everywhere.

// Only the macOS camera uses these outside of tests.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use crate::{OUTPUT_FPS, OUTPUT_HEIGHT, OUTPUT_WIDTH};
use studio_core::device::OutputFormat;
use studio_core::error::{Result, StudioError};
use studio_core::frame::{PixelFormat, VideoFrame};

/// Bytes per BGRA pixel.
const BYTES_PER_PIXEL: usize = 4;

/// Accepts only the one format the Camera Extension offers.
pub(crate) fn validate_format(format: OutputFormat) -> Result<()> {
    if format.width == OUTPUT_WIDTH && format.height == OUTPUT_HEIGHT && format.fps == OUTPUT_FPS {
        return Ok(());
    }
    Err(StudioError::Invalid(format!(
        "the Webcam Studio Camera supports only {OUTPUT_WIDTH}x{OUTPUT_HEIGHT} at {OUTPUT_FPS} fps, \
         but {}x{} at {} fps was requested",
        format.width, format.height, format.fps
    )))
}

/// Number of bytes needed to hold `height` rows of `row_bytes` bytes that start `stride`
/// bytes apart. The last row does not need padding after it.
fn bytes_needed(stride: usize, row_bytes: usize, height: usize) -> usize {
    match height {
        0 => 0,
        h => stride * (h - 1) + row_bytes,
    }
}

/// Checks that a frame has the output size and that its data is long enough.
pub(crate) fn validate_frame(frame: &VideoFrame) -> Result<()> {
    if frame.format != PixelFormat::Bgra8 {
        return Err(StudioError::Invalid("frames must be BGRA".to_owned()));
    }
    if frame.width != OUTPUT_WIDTH || frame.height != OUTPUT_HEIGHT {
        return Err(StudioError::Invalid(format!(
            "frame is {}x{} but the camera needs {OUTPUT_WIDTH}x{OUTPUT_HEIGHT}",
            frame.width, frame.height
        )));
    }
    let row_bytes = frame.width as usize * BYTES_PER_PIXEL;
    if frame.stride < row_bytes {
        return Err(StudioError::Invalid(format!(
            "frame stride {} is smaller than one row ({row_bytes} bytes)",
            frame.stride
        )));
    }
    let needed = bytes_needed(frame.stride, row_bytes, frame.height as usize);
    if frame.data.len() < needed {
        return Err(StudioError::Invalid(format!(
            "frame data has {} bytes but {needed} are needed",
            frame.data.len()
        )));
    }
    Ok(())
}

/// Copies `height` rows of `width` BGRA pixels from `src` to `dst`.
///
/// The two buffers may have different row lengths (`src_stride`, `dst_stride`). Padding
/// bytes at the end of each destination row are left as they are.
pub(crate) fn copy_bgra_rows(
    src: &[u8],
    src_stride: usize,
    dst: &mut [u8],
    dst_stride: usize,
    width: u32,
    height: u32,
) -> Result<()> {
    let row_bytes = width as usize * BYTES_PER_PIXEL;
    let height = height as usize;
    if src_stride < row_bytes || dst_stride < row_bytes {
        return Err(StudioError::Invalid(
            "row stride is smaller than one row of pixels".to_owned(),
        ));
    }
    if src.len() < bytes_needed(src_stride, row_bytes, height)
        || dst.len() < bytes_needed(dst_stride, row_bytes, height)
    {
        return Err(StudioError::Invalid(
            "pixel buffer is too small for the frame".to_owned(),
        ));
    }
    for row in 0..height {
        let from = row * src_stride;
        let to = row * dst_stride;
        dst[to..to + row_bytes].copy_from_slice(&src[from..from + row_bytes]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(width: u32, height: u32, fps: u32) -> OutputFormat {
        OutputFormat { width, height, fps }
    }

    #[test]
    fn only_the_fixed_format_is_accepted() {
        assert!(validate_format(format(1280, 720, 30)).is_ok());
        for bad in [
            format(1920, 1080, 30),
            format(1280, 720, 60),
            format(640, 480, 30),
        ] {
            let message = validate_format(bad).unwrap_err().to_string();
            assert!(message.contains("1280x720 at 30 fps"), "{message}");
        }
    }

    #[test]
    fn frame_checks() {
        let good = VideoFrame::new_bgra(1280, 720, 0);
        assert!(validate_frame(&good).is_ok());

        let small = VideoFrame::new_bgra(640, 360, 0);
        assert!(validate_frame(&small).is_err());

        let mut short = VideoFrame::new_bgra(1280, 720, 0);
        short.data.truncate(100);
        assert!(validate_frame(&short).is_err());

        let mut narrow = VideoFrame::new_bgra(1280, 720, 0);
        narrow.stride = 16;
        assert!(validate_frame(&narrow).is_err());
    }

    #[test]
    fn padded_frame_is_accepted() {
        let mut frame = VideoFrame::new_bgra(1280, 720, 0);
        frame.stride = 1280 * 4 + 64;
        frame.data = vec![0; frame.stride * 720];
        assert!(validate_frame(&frame).is_ok());
    }

    #[test]
    fn copy_with_different_strides() {
        // 3x2 image. The source has 4 bytes of padding per row, the destination 8.
        let (width, height) = (3u32, 2u32);
        let src_stride = 12 + 4;
        let dst_stride = 12 + 8;
        let mut src = vec![0xEEu8; src_stride * 2];
        for row in 0..2usize {
            for i in 0..12usize {
                src[row * src_stride + i] = (row * 12 + i) as u8;
            }
        }
        let mut dst = vec![0xAAu8; dst_stride * 2];
        copy_bgra_rows(&src, src_stride, &mut dst, dst_stride, width, height).unwrap();

        for row in 0..2usize {
            for i in 0..12usize {
                assert_eq!(dst[row * dst_stride + i], (row * 12 + i) as u8);
            }
            // Destination padding is untouched.
            assert!(
                dst[row * dst_stride + 12..(row + 1) * dst_stride]
                    .iter()
                    .all(|&b| b == 0xAA)
            );
        }
    }

    #[test]
    fn copy_tight_to_tight() {
        let src: Vec<u8> = (0..24).collect();
        let mut dst = vec![0u8; 24];
        copy_bgra_rows(&src, 12, &mut dst, 12, 3, 2).unwrap();
        assert_eq!(src, dst);
    }

    #[test]
    fn copy_rejects_short_buffers_and_bad_strides() {
        let src = vec![0u8; 24];
        let mut dst = vec![0u8; 10];
        assert!(copy_bgra_rows(&src, 12, &mut dst, 12, 3, 2).is_err());

        let mut dst = vec![0u8; 24];
        assert!(copy_bgra_rows(&src, 8, &mut dst, 12, 3, 2).is_err());
        assert!(copy_bgra_rows(&src, 12, &mut dst, 8, 3, 2).is_err());
    }

    #[test]
    fn copy_of_zero_rows_is_fine() {
        let mut dst = [];
        copy_bgra_rows(&[], 0, &mut dst, 0, 0, 0).unwrap();
    }
}
