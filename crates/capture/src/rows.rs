//! Copying rows of BGRA pixels that may have padding at the end of each row.

// Only the macOS capture uses it outside of tests.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use studio_core::frame::VideoFrame;

/// Builds a [`VideoFrame`] with no row padding from `rows`, where each row of pixels starts
/// `stride` bytes after the previous one. Returns `None` if `rows` is too short or the
/// stride is smaller than one row.
pub(crate) fn frame_from_bgra_rows(
    rows: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    timestamp_us: u64,
) -> Option<VideoFrame> {
    let row_bytes = width as usize * 4;
    let height_rows = height as usize;
    if height_rows == 0 || row_bytes == 0 || stride < row_bytes {
        return None;
    }
    if rows.len() < stride * (height_rows - 1) + row_bytes {
        return None;
    }
    let mut frame = VideoFrame::new_bgra(width, height, timestamp_us);
    for (row, target) in frame.data.chunks_exact_mut(row_bytes).enumerate() {
        let from = row * stride;
        target.copy_from_slice(&rows[from..from + row_bytes]);
    }
    Some(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_row_padding() {
        // 2x2 pixels, 4 bytes of padding after each row.
        let stride = 8 + 4;
        let mut rows = vec![0xEEu8; stride * 2];
        rows[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        rows[stride..stride + 8].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);
        let frame = frame_from_bgra_rows(&rows, stride, 2, 2, 42).unwrap();
        assert_eq!(frame.stride, 8);
        assert_eq!(frame.timestamp_us, 42);
        assert_eq!(frame.data, (1..=16).collect::<Vec<u8>>());
        assert_eq!(frame.pixel(1, 1), [13, 14, 15, 16]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(frame_from_bgra_rows(&[0; 8], 8, 2, 2, 0).is_none());
        assert!(frame_from_bgra_rows(&[0; 16], 4, 2, 2, 0).is_none());
        assert!(frame_from_bgra_rows(&[], 0, 0, 0, 0).is_none());
    }
}
