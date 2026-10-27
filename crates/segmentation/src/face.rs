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

/// One eye: its six outline points (two corners, two on the upper lid, two on the lower lid),
/// in shares of the picture with the origin at the top left, in Apple Vision's order.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Eye {
    pub points: [[f32; 2]; 6],
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

/// The eye from Vision's outline points (shares of the picture, top-left), if there are six.
pub fn eye_from_points(outline: &[(f32, f32)]) -> Option<Eye> {
    if outline.len() != 6 {
        return None;
    }
    let mut points = [[0.0; 2]; 6];
    for (p, &(x, y)) in points.iter_mut().zip(outline) {
        *p = [x, y];
    }
    Some(Eye { points })
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
    fn an_eye_needs_six_points() {
        let six = [
            (0.4, 0.3),
            (0.45, 0.29),
            (0.55, 0.29),
            (0.6, 0.3),
            (0.55, 0.31),
            (0.45, 0.31),
        ];
        assert_eq!(eye_from_points(&six).unwrap().points[3], [0.6, 0.3]);
        assert!(eye_from_points(&six[..5]).is_none());
    }
}
