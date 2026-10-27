//! What Apple Vision sees of the people in a picture: faces (auto-frame, Studio Light), hands
//! (Show-and-tell) and eyes (Eye contact). Instances belong to the thread that creates them.
use crate::person::Error;
use studio_core::frame::VideoFrame;

/// A face in a picture, in shares of the picture size, with the origin at the top left.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FaceBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
}

/// A hand, as the box around its joints, in shares of the picture (top-left origin).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HandBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
}

/// One eye: the centre and size of its opening and where the pupil is, in shares of the
/// picture (top-left origin; `width` is a share of the picture width, `height` of its height).
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Eye {
    pub cx: f32,
    pub cy: f32,
    pub width: f32,
    pub height: f32,
    pub pupil_x: f32,
    pub pupil_y: f32,
}

/// The eyes of one face.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EyePair {
    pub left: Eye,
    pub right: Eye,
}

/// Everything found in one picture.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Sighting {
    pub faces: Vec<FaceBox>,
    pub hands: Vec<HandBox>,
    pub eyes: Vec<EyePair>,
}

/// What to look for. Faces always come back; hands and eyes cost more and are asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Want {
    pub hands: bool,
    pub eyes: bool,
}

/// Faces with less confidence than this are not used.
pub const MIN_CONFIDENCE: f32 = 0.5;
/// Hand joints with less confidence than this are not used.
pub const MIN_JOINT_CONFIDENCE: f32 = 0.3;

pub trait FaceDetector {
    /// What is in an encoded JPEG or PNG, upright and not mirrored.
    fn detect(&mut self, encoded: &[u8], want: Want) -> Result<Sighting, Error>;
    /// What is in a raw BGRA camera frame, upright and not mirrored.
    fn detect_frame(&mut self, frame: &VideoFrame, want: Want) -> Result<Sighting, Error>;
}

/// The eye described by its outline points and pupil (shares of the picture, top-left).
pub fn eye_from_points(outline: &[(f32, f32)], pupil: Option<(f32, f32)>) -> Option<Eye> {
    if outline.len() < 3 {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for &(x, y) in outline {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (px, py) = pupil.unwrap_or((cx, cy));
    Some(Eye {
        cx,
        cy,
        width: x1 - x0,
        height: y1 - y0,
        pupil_x: px,
        pupil_y: py,
    })
}

#[cfg(target_os = "macos")]
pub use crate::vision::VisionFaceDetector;

#[cfg(not(target_os = "macos"))]
pub struct VisionFaceDetector;
#[cfg(not(target_os = "macos"))]
impl VisionFaceDetector {
    pub fn new() -> Result<Self, Error> {
        Err(Error::Unsupported)
    }
}
#[cfg(not(target_os = "macos"))]
impl FaceDetector for VisionFaceDetector {
    fn detect(&mut self, _: &[u8], _: Want) -> Result<Sighting, Error> {
        Err(Error::Unsupported)
    }
    fn detect_frame(&mut self, _: &VideoFrame, _: Want) -> Result<Sighting, Error> {
        Err(Error::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_eye_is_measured_from_its_outline() {
        let eye = eye_from_points(
            &[(0.4, 0.3), (0.5, 0.28), (0.6, 0.3), (0.5, 0.32)],
            Some((0.5, 0.31)),
        )
        .unwrap();
        assert!((eye.cx - 0.5).abs() < 1e-6 && (eye.cy - 0.3).abs() < 1e-6);
        assert!((eye.width - 0.2).abs() < 1e-6 && (eye.height - 0.04).abs() < 1e-6);
        assert_eq!((eye.pupil_x, eye.pupil_y), (0.5, 0.31));
        assert!(eye_from_points(&[(0.1, 0.1)], None).is_none());
    }
}
