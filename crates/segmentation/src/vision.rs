use crate::{
    Mask,
    person::{Error, PersonSegmenter, Quality},
};
use objc2::{
    AnyThread,
    rc::{Retained, autoreleasepool},
};
use objc2_core_foundation::CFData;
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelFormatType_32BGRA, kCVPixelFormatType_OneComponent8,
};
use objc2_foundation::NSArray;
use objc2_image_io::CGImageSource;
use objc2_vision::{
    VNGeneratePersonSegmentationRequest, VNGeneratePersonSegmentationRequestQualityLevel,
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
        if encoded.len() < 4 || encoded.len() > 8 * 1024 * 1024 {
            return Err(Error::InvalidImage("empty or oversized input".into()));
        }
        autoreleasepool(|_| {
            let start = std::time::Instant::now();
            let data = CFData::from_bytes(encoded);
            let source = unsafe { CGImageSource::with_data(&data, None) }
                .ok_or_else(|| Error::InvalidImage("decode failed".into()))?;
            let image = unsafe { source.image_at_index(0, None) }
                .ok_or_else(|| Error::InvalidImage("decode failed".into()))?;
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
        autoreleasepool(|_| {
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
