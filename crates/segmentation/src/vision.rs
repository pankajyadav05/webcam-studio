use crate::{
    Mask,
    face::{
        EyePair, FaceBox, FaceDetector, HandBox, MIN_CONFIDENCE, MIN_JOINT_CONFIDENCE, Sighting,
        Want, eye_from_points,
    },
    person::{Error, PersonSegmenter, Quality},
};
use objc2::{
    AnyThread,
    rc::{Retained, autoreleasepool},
};
use objc2_core_foundation::CFData;
use objc2_core_graphics::CGImage;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_32BGRA, kCVPixelFormatType_OneComponent8,
};
use objc2_foundation::NSArray;
use objc2_image_io::CGImageSource;
use objc2_vision::{
    VNDetectFaceLandmarksRequest, VNDetectFaceRectanglesRequest, VNDetectHumanHandPoseRequest,
    VNFaceLandmarkRegion2D, VNFaceObservation, VNGeneratePersonSegmentationRequest,
    VNGeneratePersonSegmentationRequestQualityLevel, VNHumanHandPoseObservation,
    VNHumanHandPoseObservationJointNameIndexMCP, VNHumanHandPoseObservationJointNameIndexTip,
    VNHumanHandPoseObservationJointNameLittleMCP, VNHumanHandPoseObservationJointNameLittleTip,
    VNHumanHandPoseObservationJointNameMiddleMCP, VNHumanHandPoseObservationJointNameMiddleTip,
    VNHumanHandPoseObservationJointNameRingTip, VNHumanHandPoseObservationJointNameThumbCMC,
    VNHumanHandPoseObservationJointNameThumbTip, VNHumanHandPoseObservationJointNameWrist,
    VNRequest, VNSequenceRequestHandler,
};
use std::ptr::NonNull;
use studio_core::frame::{PixelFormat, VideoFrame};

pub struct VisionSegmenter {
    request: Retained<VNGeneratePersonSegmentationRequest>,
    quality: Option<Quality>,
    sequence: Retained<VNSequenceRequestHandler>,
}
impl VisionSegmenter {
    pub fn new() -> Result<Self, Error> {
        if objc2::runtime::AnyClass::get(c"VNGeneratePersonSegmentationRequest").is_none() {
            return Err(Error::Unsupported);
        }
        // Request and its temporal state never cross threads.
        let request = unsafe { VNGeneratePersonSegmentationRequest::new() };
        unsafe { request.setOutputPixelFormat(kCVPixelFormatType_OneComponent8) };
        Ok(Self {
            request,
            quality: None,
            sequence: unsafe { VNSequenceRequestHandler::init(VNSequenceRequestHandler::alloc()) },
        })
    }
}
impl VisionSegmenter {
    fn set_quality(&mut self, quality: Quality) {
        if self.quality != Some(quality) {
            unsafe {
                self.request.setQualityLevel(match quality {
                    Quality::Balanced => VNGeneratePersonSegmentationRequestQualityLevel::Balanced,
                    Quality::Accurate => VNGeneratePersonSegmentationRequestQualityLevel::Accurate,
                })
            };
            self.quality = Some(quality);
        }
    }

    fn requests(&self) -> Retained<NSArray<VNRequest>> {
        NSArray::from_slice(&[self.request.as_ref() as &VNRequest])
    }

    /// Copies the mask of the last request out of Vision's buffer.
    fn read_mask(&self) -> Result<Mask, Error> {
        let results =
            unsafe { self.request.results() }.ok_or_else(|| Error::Vision("no results".into()))?;
        let observation = results
            .firstObject()
            .ok_or_else(|| Error::Vision("no mask observation".into()))?;
        let buffer = unsafe { observation.pixelBuffer() };
        let flags = CVPixelBufferLockFlags::ReadOnly;
        let code = unsafe { CVPixelBufferLockBaseAddress(&buffer, flags) };
        if code != 0 {
            return Err(Error::Vision(format!("mask lock {code}")));
        }
        // Always unlock, including malformed buffers. Rows may have padding.
        let result = (|| {
            let width = CVPixelBufferGetWidth(&buffer);
            let height = CVPixelBufferGetHeight(&buffer);
            let stride = CVPixelBufferGetBytesPerRow(&buffer);
            if CVPixelBufferGetPixelFormatType(&buffer) != kCVPixelFormatType_OneComponent8 {
                return Err(Error::Vision("unexpected mask pixel format".into()));
            }
            let base = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
            if base.is_null()
                || width == 0
                || height == 0
                || stride < width
                || width * height > 16_777_216
            {
                return Err(Error::Vision("invalid mask buffer".into()));
            }
            let mut alpha = Vec::with_capacity(width * height);
            for y in 0..height {
                // The read lock keeps these width-byte rows alive.
                alpha.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(base.add(y * stride), width)
                });
            }
            Ok(Mask {
                width: width as u32,
                height: height as u32,
                alpha,
            })
        })();
        unsafe { CVPixelBufferUnlockBaseAddress(&buffer, flags) };
        result
    }
}

impl PersonSegmenter for VisionSegmenter {
    fn reset(&mut self) -> Result<(), Error> {
        *self = Self::new()?;
        Ok(())
    }
    fn segment(&mut self, encoded: &[u8], quality: Quality) -> Result<Mask, Error> {
        autoreleasepool(|_| {
            let start = std::time::Instant::now();
            let image = decode(encoded)?;
            self.set_quality(quality);
            unsafe {
                self.sequence
                    .performRequests_onCGImage_error(&self.requests(), &image)
            }
            .map_err(|e| Error::Vision(e.to_string()))?;
            let result = self.read_mask();
            log::debug!(
                "Vision {quality:?}: {:.2} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
            result
        })
    }

    fn segment_frame(&mut self, frame: &VideoFrame, quality: Quality) -> Result<Mask, Error> {
        autoreleasepool(|_| {
            let buffer = pixel_buffer(frame)?;
            self.set_quality(quality);
            unsafe {
                self.sequence
                    .performRequests_onCVPixelBuffer_error(&self.requests(), &buffer)
            }
            .map_err(|e| Error::Vision(e.to_string()))?;
            self.read_mask()
        })
    }
}

/// Decodes a JPEG or PNG for Vision.
fn decode(encoded: &[u8]) -> Result<objc2_core_foundation::CFRetained<CGImage>, Error> {
    if encoded.len() < 4 || encoded.len() > 8 * 1024 * 1024 {
        return Err(Error::InvalidImage("empty or oversized input".into()));
    }
    let data = CFData::from_bytes(encoded);
    let source = unsafe { CGImageSource::with_data(&data, None) }
        .ok_or_else(|| Error::InvalidImage("decode failed".into()))?;
    unsafe { source.image_at_index(0, None) }
        .ok_or_else(|| Error::InvalidImage("decode failed".into()))
}

/// Copies a BGRA frame into a pixel buffer Vision can read.
fn pixel_buffer(frame: &VideoFrame) -> Result<Retained<CVPixelBuffer>, Error> {
    let (width, height) = (frame.width as usize, frame.height as usize);
    let row = width * 4;
    if frame.format != PixelFormat::Bgra8
        || width == 0
        || height == 0
        || width * height > 16_777_216
        || frame.stride < row
        || frame.data.len() < frame.stride * (height - 1) + row
    {
        return Err(Error::InvalidImage("not a complete BGRA frame".into()));
    }
    let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
    let code = unsafe {
        CVPixelBufferCreate(
            None,
            width,
            height,
            kCVPixelFormatType_32BGRA,
            None,
            NonNull::from(&mut out),
        )
    };
    if code != 0 || out.is_null() {
        return Err(Error::Vision(format!("pixel buffer create {code}")));
    }
    // CVPixelBufferCreate returns a +1 reference; Retained releases it.
    let buffer = unsafe { Retained::from_raw(out) }
        .ok_or_else(|| Error::Vision("pixel buffer create".into()))?;
    let code = unsafe { CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) };
    if code != 0 {
        return Err(Error::Vision(format!("pixel buffer lock {code}")));
    }
    let base = CVPixelBufferGetBaseAddress(&buffer).cast::<u8>();
    let stride = CVPixelBufferGetBytesPerRow(&buffer);
    if !base.is_null() && stride >= row {
        for y in 0..height {
            // The write lock keeps the buffer's rows alive; source rows are bounds checked above.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    frame.data.as_ptr().add(y * frame.stride),
                    base.add(y * stride),
                    row,
                )
            };
        }
    }
    unsafe { CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) };
    if base.is_null() || stride < row {
        return Err(Error::Vision("invalid pixel buffer".into()));
    }
    Ok(buffer)
}

/// Apple Vision's faces, hands and eyes.
pub struct VisionFaceDetector {
    faces: Retained<VNDetectFaceRectanglesRequest>,
    landmarks: Retained<VNDetectFaceLandmarksRequest>,
    hands: Retained<VNDetectHumanHandPoseRequest>,
    sequence: Retained<VNSequenceRequestHandler>,
}

/// Vision's points have the origin at the bottom left; ours at the top left.
fn flip(x: f64, y: f64) -> (f32, f32) {
    (x as f32, 1.0 - y as f32)
}

impl VisionFaceDetector {
    pub fn new() -> Result<Self, Error> {
        if objc2::runtime::AnyClass::get(c"VNDetectFaceRectanglesRequest").is_none() {
            return Err(Error::Unsupported);
        }
        let hands = unsafe { VNDetectHumanHandPoseRequest::new() };
        unsafe { hands.setMaximumHandCount(2) };
        Ok(Self {
            faces: unsafe { VNDetectFaceRectanglesRequest::new() },
            landmarks: unsafe { VNDetectFaceLandmarksRequest::new() },
            hands,
            sequence: unsafe { VNSequenceRequestHandler::init(VNSequenceRequestHandler::alloc()) },
        })
    }

    fn requests(&self, want: Want) -> Retained<NSArray<VNRequest>> {
        // Landmarks also give the face boxes, so faces are asked for once either way.
        let face: &VNRequest = if want.eyes {
            self.landmarks.as_ref()
        } else {
            self.faces.as_ref()
        };
        if want.hands {
            NSArray::from_slice(&[face, self.hands.as_ref() as &VNRequest])
        } else {
            NSArray::from_slice(&[face])
        }
    }

    fn read(&self, want: Want) -> Sighting {
        let mut sighting = Sighting::default();
        let faces = if want.eyes {
            unsafe { self.landmarks.results() }
        } else {
            unsafe { self.faces.results() }
        };
        if let Some(results) = faces {
            for face in (0..results.count()).map(|i| results.objectAtIndex(i)) {
                let confidence = unsafe { face.confidence() };
                let r = unsafe { face.boundingBox() };
                let (x, y, w, h) = (
                    r.origin.x as f32,
                    r.origin.y as f32,
                    r.size.width as f32,
                    r.size.height as f32,
                );
                if confidence < MIN_CONFIDENCE
                    || w <= 0.0
                    || h <= 0.0
                    || ![x, y, w, h].iter().all(|v| v.is_finite())
                {
                    continue;
                }
                sighting.faces.push(FaceBox {
                    x,
                    y: 1.0 - y - h,
                    width: w,
                    height: h,
                    confidence,
                });
                if want.eyes {
                    if let Some(pair) = read_eyes(&face) {
                        sighting.eyes.push(pair);
                    }
                }
            }
        }
        if want.hands {
            if let Some(results) = unsafe { self.hands.results() } {
                for hand in (0..results.count()).map(|i| results.objectAtIndex(i)) {
                    if let Some(found) = read_hand(&hand) {
                        sighting.hands.push(found);
                    }
                }
            }
        }
        sighting
    }
}

/// The points of a landmark region, in shares of the picture (top-left origin).
fn region_points(
    region: Option<Retained<VNFaceLandmarkRegion2D>>,
    face: objc2_core_foundation::CGRect,
) -> Vec<(f32, f32)> {
    let Some(region) = region else {
        return Vec::new();
    };
    let count = unsafe { region.pointCount() };
    let points = unsafe { region.normalizedPoints() };
    if points.is_null() || count == 0 || count > 1024 {
        return Vec::new();
    }
    // Vision keeps the points alive as long as the region.
    let points = unsafe { std::slice::from_raw_parts(points, count) };
    points
        .iter()
        .map(|p| {
            flip(
                face.origin.x + p.x * face.size.width,
                face.origin.y + p.y * face.size.height,
            )
        })
        .filter(|(x, y)| x.is_finite() && y.is_finite())
        .collect()
}

fn read_eyes(face: &VNFaceObservation) -> Option<EyePair> {
    let landmarks = unsafe { face.landmarks() }?;
    let bounds = unsafe { face.boundingBox() };
    let centre = |points: Vec<(f32, f32)>| {
        (!points.is_empty()).then(|| {
            let n = points.len() as f32;
            (
                points.iter().map(|p| p.0).sum::<f32>() / n,
                points.iter().map(|p| p.1).sum::<f32>() / n,
            )
        })
    };
    // Both eyes are treated alike; Vision's "left" is the one on the left of the picture.
    let left = eye_from_points(
        &region_points(unsafe { landmarks.leftEye() }, bounds),
        centre(region_points(unsafe { landmarks.leftPupil() }, bounds)),
    )?;
    let right = eye_from_points(
        &region_points(unsafe { landmarks.rightEye() }, bounds),
        centre(region_points(unsafe { landmarks.rightPupil() }, bounds)),
    )?;
    Some(EyePair { left, right })
}

fn read_hand(hand: &VNHumanHandPoseObservation) -> Option<HandBox> {
    let joints = unsafe {
        [
            VNHumanHandPoseObservationJointNameWrist,
            VNHumanHandPoseObservationJointNameThumbCMC,
            VNHumanHandPoseObservationJointNameThumbTip,
            VNHumanHandPoseObservationJointNameIndexMCP,
            VNHumanHandPoseObservationJointNameIndexTip,
            VNHumanHandPoseObservationJointNameMiddleMCP,
            VNHumanHandPoseObservationJointNameMiddleTip,
            VNHumanHandPoseObservationJointNameRingTip,
            VNHumanHandPoseObservationJointNameLittleMCP,
            VNHumanHandPoseObservationJointNameLittleTip,
        ]
    };
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let (mut seen, mut sum) = (0, 0.0f32);
    for name in joints {
        let Ok(point) = (unsafe { hand.recognizedPointForJointName_error(name) }) else {
            continue;
        };
        let confidence = unsafe { point.confidence() };
        if confidence < MIN_JOINT_CONFIDENCE {
            continue;
        }
        let at = unsafe { point.location() };
        let (x, y) = flip(at.x, at.y);
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
        seen += 1;
        sum += confidence;
    }
    (seen >= 4).then(|| HandBox {
        x: x0,
        y: y0,
        width: (x1 - x0).max(1e-4),
        height: (y1 - y0).max(1e-4),
        confidence: sum / seen as f32,
    })
}

impl FaceDetector for VisionFaceDetector {
    fn detect(&mut self, encoded: &[u8], want: Want) -> Result<Sighting, Error> {
        autoreleasepool(|_| {
            let image = decode(encoded)?;
            unsafe {
                self.sequence
                    .performRequests_onCGImage_error(&self.requests(want), &image)
            }
            .map_err(|e| Error::Vision(e.to_string()))?;
            Ok(self.read(want))
        })
    }

    fn detect_frame(&mut self, frame: &VideoFrame, want: Want) -> Result<Sighting, Error> {
        autoreleasepool(|_| {
            let buffer = pixel_buffer(frame)?;
            unsafe {
                self.sequence
                    .performRequests_onCVPixelBuffer_error(&self.requests(want), &buffer)
            }
            .map_err(|e| Error::Vision(e.to_string()))?;
            Ok(self.read(want))
        })
    }
}
