//! Loads a background picture from disk into a frame the engine can use.

use std::path::Path;

use image::{DynamicImage, ImageDecoder, ImageReader, imageops::FilterType};
use studio_core::frame::{PixelFormat, VideoFrame};

/// Pictures larger than this are scaled down on load. The output is 1280x720, so a bigger
/// picture would only cost memory.
const MAX_WIDTH: u32 = 1920;
const MAX_HEIGHT: u32 = 1080;

/// Reads the picture at `path` and converts it to a BGRA frame.
///
/// PNG and JPEG are supported. Transparent parts are blended onto black. The error text is
/// ready to show to the person using the app.
pub fn load_background(path: &str) -> Result<VideoFrame, String> {
    let fail = |reason: String| format!("Could not open that image: {reason}");

    if path.trim().is_empty() {
        return Err(fail("no file was named.".to_string()));
    }
    let reader = ImageReader::open(Path::new(path.trim()))
        .map_err(|e| fail(io_reason(&e)))?
        .with_guessed_format()
        .map_err(|e| fail(io_reason(&e)))?;
    let mut decoder = reader.into_decoder().map_err(|e| fail(decode_reason(&e)))?;
    // Phone photos are often stored sideways with a note on how to turn them.
    let orientation = decoder.orientation().ok();
    let mut picture = DynamicImage::from_decoder(decoder).map_err(|e| fail(decode_reason(&e)))?;
    if let Some(orientation) = orientation {
        picture.apply_orientation(orientation);
    }
    if picture.width() > MAX_WIDTH || picture.height() > MAX_HEIGHT {
        picture = picture.resize(MAX_WIDTH, MAX_HEIGHT, FilterType::Triangle);
    }
    Ok(to_frame(&picture))
}

/// Decodes a picture the window drew (PNG or JPEG bytes) into a BGRA frame.
pub fn decode_background(bytes: &[u8]) -> Result<VideoFrame, String> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err("The background picture is empty or too large.".to_string());
    }
    let mut picture = image::load_from_memory(bytes).map_err(|e| {
        format!(
            "Could not read the background picture: {}",
            decode_reason(&e)
        )
    })?;
    if picture.width() > MAX_WIDTH || picture.height() > MAX_HEIGHT {
        picture = picture.resize(MAX_WIDTH, MAX_HEIGHT, FilterType::Triangle);
    }
    Ok(to_frame(&picture))
}

fn io_reason(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "the file was not found.".to_string(),
        std::io::ErrorKind::PermissionDenied => "the app is not allowed to read it.".to_string(),
        _ => format!("{error}."),
    }
}

fn decode_reason(error: &image::ImageError) -> String {
    match error {
        image::ImageError::Unsupported(_) => {
            "this kind of file is not supported. Use a PNG or JPEG picture.".to_string()
        }
        image::ImageError::IoError(io) => io_reason(io),
        other => format!("the file is damaged or is not a picture ({other})."),
    }
}

/// Converts to BGRA with no row padding. Transparent pixels are blended onto black.
fn to_frame(picture: &DynamicImage) -> VideoFrame {
    let rgba = picture.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut data = Vec::with_capacity(rgba.as_raw().len());
    for px in rgba.as_raw().chunks_exact(4) {
        let alpha = u32::from(px[3]);
        let blend = |c: u8| (u32::from(c) * alpha / 255) as u8;
        data.extend_from_slice(&[blend(px[2]), blend(px[1]), blend(px[0]), 255]);
    }
    VideoFrame {
        width,
        height,
        stride: width as usize * 4,
        format: PixelFormat::Bgra8,
        data,
        timestamp_us: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A file in the temp folder that is removed when dropped.
    struct TempFile(std::path::PathBuf);

    impl TempFile {
        fn new(extension: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("webcam-studio-test-{}-{n}.{extension}", std::process::id());
            Self(std::env::temp_dir().join(name))
        }

        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Writes a PNG of the given size where every pixel is `rgba`.
    fn write_png(file: &TempFile, width: u32, height: u32, rgba: [u8; 4]) {
        let img = image::RgbaImage::from_pixel(width, height, image::Rgba(rgba));
        img.save(&file.0).unwrap();
    }

    #[test]
    fn png_becomes_a_bgra_frame() {
        let file = TempFile::new("png");
        write_png(&file, 4, 3, [10, 20, 30, 255]);
        let frame = load_background(file.path()).unwrap();
        assert_eq!((frame.width, frame.height), (4, 3));
        assert_eq!(frame.stride, 16);
        assert_eq!(frame.format, PixelFormat::Bgra8);
        assert_eq!(frame.data.len(), 48);
        // Red 10, green 20, blue 30 is stored as blue, green, red, alpha.
        assert_eq!(frame.pixel(0, 0), [30, 20, 10, 255]);
        assert_eq!(frame.pixel(3, 2), [30, 20, 10, 255]);
    }

    #[test]
    fn transparency_is_blended_onto_black() {
        let file = TempFile::new("png");
        write_png(&file, 2, 2, [200, 100, 50, 0]);
        let frame = load_background(file.path()).unwrap();
        assert_eq!(frame.pixel(1, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn big_pictures_are_scaled_down() {
        let file = TempFile::new("png");
        write_png(&file, 3840, 100, [1, 2, 3, 255]);
        let frame = load_background(file.path()).unwrap();
        assert_eq!(frame.width, 1920);
        assert_eq!(frame.height, 50);
        assert_eq!(frame.data.len(), 1920 * 50 * 4);
    }

    #[test]
    fn a_missing_file_gives_a_plain_message() {
        let error = load_background("/definitely/not/here.png").unwrap_err();
        assert_eq!(error, "Could not open that image: the file was not found.");
    }

    #[test]
    fn an_empty_path_gives_a_plain_message() {
        let error = load_background("  ").unwrap_err();
        assert!(error.starts_with("Could not open that image:"));
    }

    #[test]
    fn a_file_that_is_not_a_picture_is_refused() {
        let file = TempFile::new("png");
        std::fs::write(&file.0, b"this is not a picture").unwrap();
        let error = load_background(file.path()).unwrap_err();
        assert!(error.starts_with("Could not open that image:"), "{error}");
    }
}
