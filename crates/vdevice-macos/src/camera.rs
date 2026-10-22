//! The macOS virtual camera: turns engine frames into sample buffers and puts them on the
//! Camera Extension's sink stream.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFString, CFType};
use objc2_core_media::{
    CMClock, CMFormatDescription, CMSampleBuffer, CMSampleTimingInfo, CMSimpleQueue, CMTime,
    CMTimeFlags, CMVideoFormatDescriptionCreateForImageBuffer, kCMSimpleQueueError_QueueIsFull,
};
use objc2_core_media_io::{CMIODeviceID, CMIOStreamID};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferPool, CVPixelBufferUnlockBaseAddress, kCVPixelBufferHeightKey,
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferPixelFormatTypeKey,
    kCVPixelBufferPoolMinimumBufferCountKey, kCVPixelBufferWidthKey, kCVPixelFormatType_32BGRA,
    kCVReturnSuccess,
};
use studio_core::device::{OutputFormat, VirtualCamera};
use studio_core::error::{Result, StudioError};
use studio_core::frame::VideoFrame;

use crate::pixels::{copy_bgra_rows, validate_format, validate_frame};
use crate::{OUTPUT_FPS, OUTPUT_HEIGHT, OUTPUT_WIDTH, cmio};

/// How many buffers the pool keeps ready.
const POOL_MINIMUM_BUFFERS: i32 = 4;

/// Duration of one frame: 1/30 second.
fn frame_duration() -> CMTime {
    CMTime {
        value: 1,
        timescale: OUTPUT_FPS as i32,
        flags: CMTimeFlags::Valid,
        epoch: 0,
    }
}

/// The "no time" value (`kCMTimeInvalid`), used for the decode time.
fn invalid_time() -> CMTime {
    CMTime {
        value: 0,
        timescale: 0,
        flags: CMTimeFlags(0),
        epoch: 0,
    }
}

/// Wraps a 32-bit number for a Core Foundation dictionary.
fn cf_number(value: i32) -> CFRetained<CFNumber> {
    CFNumber::new_i32(value)
}

/// Builds `CVPixelBuffer`s from a pool and wraps them in `CMSampleBuffer`s.
pub(crate) struct SampleBufferBuilder {
    pool: CFRetained<CVPixelBufferPool>,
    format_description: Option<CFRetained<CMFormatDescription>>,
    clock: CFRetained<CMClock>,
}

// SAFETY: The pool, the format description and the clock are Core Foundation / Core Video
// objects that Apple documents as thread-safe. The builder is also only used through
// `&mut self`, so one thread uses it at a time.
unsafe impl Send for SampleBufferBuilder {}

impl SampleBufferBuilder {
    /// Creates the pixel buffer pool: 1280x720 BGRA, backed by IOSurface so the extension
    /// process can read the pixels without a copy.
    pub(crate) fn new() -> Result<Self> {
        let pool = create_pool()?;
        Ok(Self {
            pool,
            format_description: None,
            clock: host_time_clock(),
        })
    }

    /// Copies `frame` into a pooled pixel buffer and wraps it in a sample buffer that is
    /// stamped with the current host time.
    pub(crate) fn build(&mut self, frame: &VideoFrame) -> Result<CFRetained<CMSampleBuffer>> {
        validate_frame(frame)?;
        let pixel_buffer = create_pixel_buffer(&self.pool)?;
        fill_pixel_buffer(&pixel_buffer, frame)?;

        let format_description = match &self.format_description {
            Some(description) => description.clone(),
            None => {
                let description = create_format_description(&pixel_buffer)?;
                self.format_description = Some(description.clone());
                description
            }
        };

        // SAFETY: The clock is a valid host time clock.
        let now = unsafe { self.clock.time() };
        let timing = CMSampleTimingInfo {
            duration: frame_duration(),
            presentationTimeStamp: now,
            decodeTimeStamp: invalid_time(),
        };
        create_sample_buffer(&pixel_buffer, &format_description, &timing)
    }
}

fn host_time_clock() -> CFRetained<CMClock> {
    // SAFETY: Takes no arguments; returns the shared host clock, retained.
    unsafe { CMClock::host_time_clock() }
}

/// Reads one of Core Video's constant dictionary keys.
fn cf_key(key: &'static CFString) -> &'static CFString {
    key
}

fn create_pool() -> Result<CFRetained<CVPixelBufferPool>> {
    // SAFETY: These are constant strings exported by Core Video. They are valid for the
    // whole life of the process.
    let (width_key, height_key, format_key, iosurface_key, min_count_key) = unsafe {
        (
            cf_key(kCVPixelBufferWidthKey),
            cf_key(kCVPixelBufferHeightKey),
            cf_key(kCVPixelBufferPixelFormatTypeKey),
            cf_key(kCVPixelBufferIOSurfacePropertiesKey),
            cf_key(kCVPixelBufferPoolMinimumBufferCountKey),
        )
    };

    // An empty IOSurface properties dictionary asks Core Video for IOSurface backing.
    let iosurface_properties = CFDictionary::<CFString, CFType>::empty();
    let width = cf_number(OUTPUT_WIDTH as i32);
    let height = cf_number(OUTPUT_HEIGHT as i32);
    let format = cf_number(kCVPixelFormatType_32BGRA as i32);
    let buffer_attributes = CFDictionary::<CFString, CFType>::from_slices(
        &[width_key, height_key, format_key, iosurface_key],
        &[&width, &height, &format, &iosurface_properties],
    );

    let min_count = cf_number(POOL_MINIMUM_BUFFERS);
    let pool_attributes =
        CFDictionary::<CFString, CFType>::from_slices(&[min_count_key], &[&min_count]);

    let mut pool: *mut CVPixelBufferPool = ptr::null_mut();
    // SAFETY: Both dictionaries are valid for the call and `pool` is valid for one pointer
    // write.
    let status = unsafe {
        CVPixelBufferPool::create(
            None,
            Some(pool_attributes.as_ref()),
            Some(buffer_attributes.as_ref()),
            NonNull::from(&mut pool),
        )
    };
    if status != kCVReturnSuccess {
        return Err(StudioError::Backend(format!(
            "could not create the pixel buffer pool (CVReturn {status})"
        )));
    }
    let pool = NonNull::new(pool)
        .ok_or_else(|| StudioError::Backend("the pixel buffer pool is missing".to_owned()))?;
    // SAFETY: The call succeeded, so we own one reference ("Create" rule).
    Ok(unsafe { CFRetained::from_raw(pool) })
}

fn create_pixel_buffer(pool: &CVPixelBufferPool) -> Result<CFRetained<CVPixelBuffer>> {
    let mut buffer: *mut CVPixelBuffer = ptr::null_mut();
    // SAFETY: `pool` is valid and `buffer` is valid for one pointer write.
    let status =
        unsafe { CVPixelBufferPool::create_pixel_buffer(None, pool, NonNull::from(&mut buffer)) };
    if status != kCVReturnSuccess {
        return Err(StudioError::Backend(format!(
            "could not get a pixel buffer from the pool (CVReturn {status})"
        )));
    }
    let buffer = NonNull::new(buffer)
        .ok_or_else(|| StudioError::Backend("the pixel buffer is missing".to_owned()))?;
    // SAFETY: The call succeeded, so we own one reference ("Create" rule).
    Ok(unsafe { CFRetained::from_raw(buffer) })
}

/// Locks the pixel buffer, copies the frame into it row by row and unlocks it again.
fn fill_pixel_buffer(buffer: &CVPixelBuffer, frame: &VideoFrame) -> Result<()> {
    // SAFETY: `buffer` is a valid pixel buffer. Every successful lock is paired with the
    // unlock below.
    let status = unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::empty()) };
    if status != kCVReturnSuccess {
        return Err(StudioError::Backend(format!(
            "could not lock the pixel buffer (CVReturn {status})"
        )));
    }
    let result = copy_into_locked(buffer, frame);
    // SAFETY: The buffer was locked above and `destination` no longer exists.
    unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::empty()) };
    result
}

/// Copies the frame into a pixel buffer whose base address is locked.
fn copy_into_locked(buffer: &CVPixelBuffer, frame: &VideoFrame) -> Result<()> {
    let base = CVPixelBufferGetBaseAddress(buffer).cast::<u8>();
    let bytes_per_row = CVPixelBufferGetBytesPerRow(buffer);
    let rows = CVPixelBufferGetHeight(buffer);
    if base.is_null() || rows < frame.height as usize {
        return Err(StudioError::Backend(
            "the pixel buffer has no memory or is too small".to_owned(),
        ));
    }
    // SAFETY: The buffer is locked, so `base` points to `bytes_per_row * rows` bytes that
    // stay valid and unshared until we unlock it. The slice is dropped before that.
    let destination = unsafe { std::slice::from_raw_parts_mut(base, bytes_per_row * rows) };
    copy_bgra_rows(
        &frame.data,
        frame.stride,
        destination,
        bytes_per_row,
        frame.width,
        frame.height,
    )
}

fn create_format_description(buffer: &CVPixelBuffer) -> Result<CFRetained<CMFormatDescription>> {
    let mut description: *const CMFormatDescription = ptr::null();
    // SAFETY: `buffer` is valid and `description` is valid for one pointer write.
    let status = unsafe {
        CMVideoFormatDescriptionCreateForImageBuffer(None, buffer, NonNull::from(&mut description))
    };
    if status != 0 {
        return Err(StudioError::Backend(format!(
            "could not describe the video format (OSStatus {status})"
        )));
    }
    let description = NonNull::new(description.cast_mut()).ok_or_else(|| {
        StudioError::Backend("the video format description is missing".to_owned())
    })?;
    // SAFETY: The call succeeded, so we own one reference ("Create" rule).
    Ok(unsafe { CFRetained::from_raw(description) })
}

fn create_sample_buffer(
    buffer: &CVPixelBuffer,
    description: &CMFormatDescription,
    timing: &CMSampleTimingInfo,
) -> Result<CFRetained<CMSampleBuffer>> {
    let mut sample: *mut CMSampleBuffer = ptr::null_mut();
    // SAFETY: All references are valid for the call, and the sample buffer pointer is
    // valid for one pointer write. No "make data ready" callback is used, so the null
    // reference constant is never read.
    let status = unsafe {
        CMSampleBuffer::create_for_image_buffer(
            None,
            buffer,
            true,
            None,
            ptr::null_mut(),
            description,
            NonNull::from(timing),
            NonNull::from(&mut sample),
        )
    };
    if status != 0 {
        return Err(StudioError::Backend(format!(
            "could not create a sample buffer (OSStatus {status})"
        )));
    }
    let sample = NonNull::new(sample)
        .ok_or_else(|| StudioError::Backend("the sample buffer is missing".to_owned()))?;
    // SAFETY: The call succeeded, so we own one reference ("Create" rule).
    Ok(unsafe { CFRetained::from_raw(sample) })
}

/// Result of offering a sample buffer to the sink queue.
enum Enqueued {
    Accepted,
    QueueFull,
}

/// Puts `sample` on the queue. The queue takes over our reference when it accepts it.
fn enqueue(queue: &CMSimpleQueue, sample: CFRetained<CMSampleBuffer>) -> Result<Enqueued> {
    let raw = CFRetained::into_raw(sample);
    // SAFETY: `raw` is a valid sample buffer. The queue stores the pointer and the
    // consumer (the extension) releases it, so we hand over our reference.
    let status = unsafe { CMSimpleQueue::enqueue(queue, raw.cast::<c_void>()) };
    if status == 0 {
        return Ok(Enqueued::Accepted);
    }
    // SAFETY: The queue did not take the buffer, so the reference is still ours. Drop it.
    drop(unsafe { CFRetained::from_raw(raw) });
    if status == kCMSimpleQueueError_QueueIsFull {
        Ok(Enqueued::QueueFull)
    } else {
        Err(StudioError::Backend(format!(
            "could not add the frame to the camera queue (OSStatus {status})"
        )))
    }
}

/// A started camera: the device, its sink stream and the queue frames go into.
struct Running {
    device: CMIODeviceID,
    stream: CMIOStreamID,
    queue: CFRetained<CMSimpleQueue>,
    builder: SampleBufferBuilder,
}

// SAFETY: `CMSimpleQueue` is a lock-free queue that Apple documents as safe for one
// producer and one consumer on different threads; we are the only producer and only
// touch it through `&mut self`. The builder is `Send` (see above).
unsafe impl Send for Running {}

/// The virtual camera on macOS. It sends frames to the Webcam Studio Camera Extension.
///
/// Create it with [`MacVirtualCamera::new`], then call `start` with a 1280x720 30 fps
/// [`OutputFormat`]. See the crate documentation for the whole data path.
pub struct MacVirtualCamera {
    name: String,
    running: Option<Running>,
    sent: u64,
    dropped: u64,
}

impl MacVirtualCamera {
    /// Creates a stopped camera. The name is only used for display; the device other apps
    /// see always has the name set in the extension.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            running: None,
            sent: 0,
            dropped: 0,
        }
    }

    /// Number of frames the extension's queue has accepted since this value was created.
    pub fn frames_sent(&self) -> u64 {
        self.sent
    }

    /// Number of frames thrown away because the extension's queue was full.
    pub fn frames_dropped(&self) -> u64 {
        self.dropped
    }

    fn stop_running(&mut self) -> Result<()> {
        match self.running.take() {
            // Dropping `running` after this releases the queue and the pool.
            Some(running) => cmio::stop_stream(running.device, running.stream),
            None => Ok(()),
        }
    }
}

impl VirtualCamera for MacVirtualCamera {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self, format: OutputFormat) -> Result<()> {
        validate_format(format)?;
        self.stop_running()?;

        let device = cmio::find_device()
            .ok_or_else(|| StudioError::Backend(cmio::NOT_INSTALLED.to_owned()))?;
        let stream = cmio::find_sink_stream(device).ok_or_else(|| {
            StudioError::Backend(
                "Webcam Studio Camera is installed but has no input stream. Reinstall it from the app's camera setup."
                    .to_owned(),
            )
        })?;
        let builder = SampleBufferBuilder::new()?;
        let queue = cmio::copy_buffer_queue(stream)?;
        cmio::start_stream(device, stream)?;

        self.running = Some(Running {
            device,
            stream,
            queue,
            builder,
        });
        Ok(())
    }

    fn push_frame(&mut self, frame: &VideoFrame) -> Result<()> {
        let running = self
            .running
            .as_mut()
            .ok_or_else(|| StudioError::Backend("the virtual camera is not started".to_owned()))?;
        let sample = running.builder.build(frame)?;
        match enqueue(&running.queue, sample)? {
            Enqueued::Accepted => self.sent += 1,
            Enqueued::QueueFull => self.dropped += 1,
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.stop_running()
    }
}

impl Drop for MacVirtualCamera {
    fn drop(&mut self) {
        let _ = self.stop_running();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::is_extension_installed;

    fn test_frame() -> VideoFrame {
        let mut frame = VideoFrame::new_bgra(OUTPUT_WIDTH, OUTPUT_HEIGHT, 0);
        frame.set_pixel(0, 0, [1, 2, 3, 255]);
        frame.set_pixel(OUTPUT_WIDTH - 1, OUTPUT_HEIGHT - 1, [9, 8, 7, 255]);
        frame
    }

    #[test]
    fn pool_creates_pixel_buffers() {
        let builder = SampleBufferBuilder::new().expect("pool");
        let first = create_pixel_buffer(&builder.pool).expect("buffer");
        let second = create_pixel_buffer(&builder.pool).expect("buffer");
        assert_eq!(CVPixelBufferGetHeight(&first), OUTPUT_HEIGHT as usize);
        assert!(CVPixelBufferGetBytesPerRow(&second) >= OUTPUT_WIDTH as usize * 4);
    }

    #[test]
    fn builds_a_sample_buffer_from_a_frame() {
        let mut builder = SampleBufferBuilder::new().expect("pool");
        let frame = test_frame();
        let sample = builder.build(&frame).expect("sample buffer");
        // SAFETY: `sample` is a valid sample buffer.
        assert!(unsafe { sample.is_valid() });
        // A second frame reuses the cached format description.
        builder.build(&frame).expect("second sample buffer");
    }

    #[test]
    fn wrong_size_frame_is_rejected() {
        let mut builder = SampleBufferBuilder::new().expect("pool");
        let frame = VideoFrame::new_bgra(640, 360, 0);
        assert!(matches!(
            builder.build(&frame),
            Err(StudioError::Invalid(_))
        ));
    }

    #[test]
    fn is_extension_installed_does_not_panic() {
        let _ = is_extension_installed();
    }

    #[test]
    fn start_without_extension_reports_it() {
        if is_extension_installed() {
            return;
        }
        let mut camera = MacVirtualCamera::new("Webcam Studio Camera");
        let format = OutputFormat {
            width: OUTPUT_WIDTH,
            height: OUTPUT_HEIGHT,
            fps: OUTPUT_FPS,
        };
        match camera.start(format) {
            Err(StudioError::Backend(message)) => assert!(message.contains("not installed")),
            other => panic!("expected the not-installed error, got {other:?}"),
        }
        // Stopping a camera that never started is fine.
        camera.stop().unwrap();
    }

    #[test]
    fn start_rejects_other_formats() {
        let mut camera = MacVirtualCamera::new("Webcam Studio Camera");
        let format = OutputFormat {
            width: 1920,
            height: 1080,
            fps: 30,
        };
        assert!(matches!(camera.start(format), Err(StudioError::Invalid(_))));
    }

    #[test]
    fn push_before_start_fails() {
        let mut camera = MacVirtualCamera::new("Webcam Studio Camera");
        assert!(camera.push_frame(&test_frame()).is_err());
        assert_eq!(camera.frames_sent(), 0);
        assert_eq!(camera.frames_dropped(), 0);
    }
}
