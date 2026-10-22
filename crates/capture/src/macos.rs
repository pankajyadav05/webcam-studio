//! Camera capture on macOS with AVFoundation.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVAuthorizationStatus, AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession,
    AVCaptureDeviceInput, AVCaptureDevicePosition, AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureDeviceTypeExternal, AVCaptureOutput, AVCaptureSession, AVCaptureSessionPreset640x480,
    AVCaptureSessionPreset1280x720, AVCaptureSessionPreset1920x1080, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeVideo,
};
use objc2_core_media::{
    CMSampleBuffer, CMTime, CMTimeFlags, CMVideoFormatDescriptionGetDimensions,
};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_32BGRA, kCVReturnSuccess,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSObject, NSObjectProtocol, NSString};
use studio_core::error::{Result, StudioError};
use studio_core::frame::VideoFrame;

use crate::format::{self, Candidate};
use crate::queue::FrameQueue;
use crate::rows::frame_from_bgra_rows;
use crate::{CameraCapture, CameraInfo, is_own_virtual_camera};

const DENIED: &str = "Camera access is denied. Allow it in System Settings \u{2192} Privacy & Security \u{2192} Camera.";

// ---------------------------------------------------------------------------------------
// Permission
// ---------------------------------------------------------------------------------------

fn authorization_status() -> AVAuthorizationStatus {
    // SAFETY: `AVMediaTypeVideo` is a constant exported by AVFoundation.
    match unsafe { AVMediaTypeVideo } {
        // SAFETY: A valid media type is passed (video is one of the two allowed values).
        Some(media) => unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) },
        None => AVAuthorizationStatus::NotDetermined,
    }
}

pub(crate) fn request_camera_permission(done: Box<dyn FnOnce(bool) + Send>) {
    match authorization_status() {
        AVAuthorizationStatus::Authorized => return done(true),
        AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted => return done(false),
        _ => {}
    }
    // SAFETY: `AVMediaTypeVideo` is a constant exported by AVFoundation.
    let Some(media) = (unsafe { AVMediaTypeVideo }) else {
        return done(false);
    };
    // The block may be called only once, but blocks are `Fn`, so the callback waits in an
    // `Option` that the first call empties.
    let done = Mutex::new(Some(done));
    let handler = RcBlock::new(move |granted: Bool| {
        let callback = done.lock().ok().and_then(|mut slot| slot.take());
        if let Some(callback) = callback {
            callback(granted.as_bool());
        }
    });
    // SAFETY: Video is a valid media type. AVFoundation copies the block, so it lives
    // until it has been called.
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &handler) };
}

// ---------------------------------------------------------------------------------------
// Finding cameras
// ---------------------------------------------------------------------------------------

fn connected_devices() -> Retained<NSArray<AVCaptureDevice>> {
    // SAFETY: These are constants exported by AVFoundation.
    let device_types = unsafe {
        NSArray::from_slice(&[
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeExternal,
        ])
    };
    // SAFETY: Valid arguments; the discovery session is only used to read its device list.
    unsafe {
        AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &device_types,
            AVMediaTypeVideo,
            AVCaptureDevicePosition::Unspecified,
        )
        .devices()
    }
}

fn device_id(device: &AVCaptureDevice) -> String {
    // SAFETY: Plain property read on a valid device.
    unsafe { device.uniqueID() }.to_string()
}

fn device_name(device: &AVCaptureDevice) -> String {
    // SAFETY: Plain property read on a valid device.
    unsafe { device.localizedName() }.to_string()
}

pub(crate) fn list_cameras() -> Result<Vec<CameraInfo>> {
    Ok(connected_devices()
        .iter()
        .map(|device| CameraInfo {
            id: device_id(&device),
            name: device_name(&device),
        })
        .filter(|camera| !is_own_virtual_camera(&camera.id, &camera.name))
        .collect())
}

// ---------------------------------------------------------------------------------------
// Receiving frames
// ---------------------------------------------------------------------------------------

struct FrameDelegateIvars {
    frames: Arc<FrameQueue>,
}

define_class!(
    // SAFETY:
    // - The superclass NSObject has no subclassing requirements.
    // - `FrameDelegate` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "WebcamStudioFrameDelegate"]
    #[ivars = FrameDelegateIvars]
    struct FrameDelegate;

    // SAFETY: `NSObjectProtocol` has no safety requirements.
    unsafe impl NSObjectProtocol for FrameDelegate {}

    // SAFETY: The method signature matches the protocol.
    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for FrameDelegate {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn capture_output_did_output_sample_buffer(
            &self,
            _output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            // This runs on the capture queue. It must be quick and must never wait for the
            // reader, so the queue drops its oldest frame when full.
            if let Some(frame) = frame_from_sample(sample_buffer) {
                self.ivars().frames.push(frame);
            }
        }
    }
);

impl FrameDelegate {
    fn new(frames: Arc<FrameQueue>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(FrameDelegateIvars { frames });
        // SAFETY: The signature of `NSObject`'s `init` method is correct.
        unsafe { msg_send![super(this), init] }
    }
}

/// Capture time of a sample in microseconds, or 0 if the sample has no valid time.
fn timestamp_us(sample: &CMSampleBuffer) -> u64 {
    // SAFETY: Plain property read on a valid sample buffer.
    let time: CMTime = unsafe { sample.presentation_time_stamp() };
    if !time.flags.contains(CMTimeFlags::Valid) || time.timescale <= 0 || time.value < 0 {
        return 0;
    }
    let micros = i128::from(time.value) * 1_000_000 / i128::from(time.timescale);
    u64::try_from(micros).unwrap_or(0)
}

fn frame_from_sample(sample: &CMSampleBuffer) -> Option<VideoFrame> {
    // SAFETY: Plain property read on a valid sample buffer. The buffer stays alive while
    // we hold the returned reference.
    let image = unsafe { sample.image_buffer() }?;
    copy_pixels(&image, timestamp_us(sample))
}

/// Copies a BGRA pixel buffer into a frame without row padding.
fn copy_pixels(buffer: &CVPixelBuffer, timestamp_us: u64) -> Option<VideoFrame> {
    if CVPixelBufferGetPixelFormatType(buffer) != kCVPixelFormatType_32BGRA {
        return None;
    }
    // SAFETY: `buffer` is valid. A successful lock is paired with the unlock below.
    let status = unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
    if status != kCVReturnSuccess {
        return None;
    }
    let width = CVPixelBufferGetWidth(buffer);
    let height = CVPixelBufferGetHeight(buffer);
    let stride = CVPixelBufferGetBytesPerRow(buffer);
    let base = CVPixelBufferGetBaseAddress(buffer)
        .cast::<u8>()
        .cast_const();
    let frame = if base.is_null() {
        None
    } else {
        // SAFETY: The buffer is locked, so `base` points to `stride * height` readable
        // bytes that nobody changes until the unlock below. The slice ends before it.
        let rows = unsafe { std::slice::from_raw_parts(base, stride * height) };
        frame_from_bgra_rows(
            rows,
            stride,
            u32::try_from(width).ok()?,
            u32::try_from(height).ok()?,
            timestamp_us,
        )
    };
    // SAFETY: The buffer was locked above with the same flags.
    unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags::ReadOnly) };
    frame
}

// ---------------------------------------------------------------------------------------
// Opening a camera
// ---------------------------------------------------------------------------------------

/// A camera session. AVFoundation objects are not marked thread-safe by the bindings.
struct AvCapture {
    name: String,
    session: Retained<AVCaptureSession>,
    output: Retained<AVCaptureVideoDataOutput>,
    frames: Arc<FrameQueue>,
    running: bool,
    // Kept alive for as long as the session runs.
    _input: Retained<AVCaptureDeviceInput>,
    _delegate: Retained<FrameDelegate>,
    _queue: DispatchRetained<DispatchQueue>,
}

// SAFETY: `AVCaptureSession` and the objects attached to it may be started and stopped from
// any thread (Apple recommends a background thread). Here they are only used through
// `&mut self`, so one thread uses them at a time. The frame delegate runs on its own
// dispatch queue and shares only the `Arc<FrameQueue>`, which is thread-safe.
unsafe impl Send for AvCapture {}

fn nserror_message(error: &objc2_foundation::NSError) -> String {
    error.localizedDescription().to_string()
}

pub(crate) fn open_camera(
    id: &str,
    width: u32,
    height: u32,
    fps: u32,
) -> Result<Box<dyn CameraCapture>> {
    if width == 0 || height == 0 || fps == 0 {
        return Err(StudioError::Invalid(
            "width, height and frame rate must not be zero".to_owned(),
        ));
    }
    if is_own_virtual_camera(id, "") {
        return Err(StudioError::Invalid(
            "the Webcam Studio Camera is the output of this app and cannot be used as an input"
                .to_owned(),
        ));
    }

    // SAFETY: Plain lookup by ID. An unknown ID gives `None`.
    let device = unsafe { AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(id)) }
        .ok_or_else(|| StudioError::Invalid(format!("camera \"{id}\" was not found")))?;
    let name = device_name(&device);
    if is_own_virtual_camera(&device_id(&device), &name) {
        return Err(StudioError::Invalid(
            "the Webcam Studio Camera is the output of this app and cannot be used as an input"
                .to_owned(),
        ));
    }

    if matches!(
        authorization_status(),
        AVAuthorizationStatus::Denied | AVAuthorizationStatus::Restricted
    ) {
        return Err(StudioError::Backend(DENIED.to_owned()));
    }

    // If permission has not been asked yet, macOS asks now, while the input is created.
    // SAFETY: `device` is a valid capture device.
    let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }
        .map_err(|error| StudioError::Backend(nserror_message(&error)))?;

    let frames = Arc::new(FrameQueue::default());
    let delegate = FrameDelegate::new(Arc::clone(&frames));
    let queue = DispatchQueue::new("dev.pankajyadav.webcamstudio.capture", None);

    // SAFETY: Creating plain AVFoundation objects.
    let (session, output) = unsafe { (AVCaptureSession::new(), AVCaptureVideoDataOutput::new()) };
    // SAFETY: The settings dictionary is valid; the delegate and queue outlive the output
    // because `AvCapture` keeps them and clears the delegate when it is dropped.
    unsafe {
        output.setVideoSettings(Some(&bgra_video_settings()));
        output.setAlwaysDiscardsLateVideoFrames(true);
        output.setSampleBufferDelegate_queue(
            Some(ProtocolObject::from_ref(&*delegate)),
            Some(&queue),
        );
    }

    // SAFETY: Session configuration calls on valid objects, wrapped in begin/commit.
    let added = unsafe {
        session.beginConfiguration();
        let added = session.canAddInput(&input) && session.canAddOutput(&output);
        if added {
            session.addInput(&input);
            session.addOutput(&output);
        }
        session.commitConfiguration();
        added
    };
    if !added {
        return Err(StudioError::Backend(format!(
            "could not connect the camera \"{name}\" to a capture session"
        )));
    }

    if !select_format(&device, width, height, fps) {
        use_preset(&session, width);
    }

    Ok(Box::new(AvCapture {
        name,
        session,
        output,
        frames,
        running: false,
        _input: input,
        _delegate: delegate,
        _queue: queue,
    }))
}

/// Asks the camera output for BGRA pictures.
fn bgra_video_settings() -> Retained<NSDictionary<NSString, AnyObject>> {
    // SAFETY: A constant exported by Core Video.
    let key: &NSString = unsafe { kCVPixelBufferPixelFormatTypeKey }.as_ref();
    let value = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
    let value: &AnyObject = &value;
    NSDictionary::from_slices(&[key], &[value])
}

/// Where a candidate came from: format number and frame rate range number.
type Origin = (usize, usize);

/// Chooses the camera format closest to the request and makes it the active one. Returns
/// `false` if nothing could be set, in which case the caller falls back to a preset.
fn select_format(device: &AVCaptureDevice, width: u32, height: u32, fps: u32) -> bool {
    // SAFETY: Plain property reads on a valid device.
    let formats = unsafe { device.formats() };
    let mut candidates = Vec::new();
    let mut origins: Vec<Origin> = Vec::new();
    for (format_index, format) in formats.iter().enumerate() {
        // SAFETY: Plain property reads on a valid format.
        let (description, ranges) = unsafe {
            (
                format.formatDescription(),
                format.videoSupportedFrameRateRanges(),
            )
        };
        // SAFETY: Formats of a video camera always carry a video format description.
        let size = unsafe { CMVideoFormatDescriptionGetDimensions(&description) };
        for (range_index, range) in ranges.iter().enumerate() {
            // SAFETY: Plain property reads on a valid range.
            let (min_fps, max_fps) = unsafe { (range.minFrameRate(), range.maxFrameRate()) };
            candidates.push(Candidate {
                width: u32::try_from(size.width).unwrap_or(0),
                height: u32::try_from(size.height).unwrap_or(0),
                min_fps,
                max_fps,
            });
            origins.push((format_index, range_index));
        }
    }
    let Some(pick) = format::pick(&candidates, width, height, fps) else {
        return false;
    };
    let (format_index, range_index) = origins[pick.index];
    let best = candidates[pick.index];
    let format = formats.objectAtIndex(format_index);
    // SAFETY: Plain property read on a valid format.
    let ranges = unsafe { format.videoSupportedFrameRateRanges() };
    let range = ranges.objectAtIndex(range_index);

    // SAFETY: Plain property reads on a valid range.
    let duration = unsafe {
        if (pick.fps - best.max_fps).abs() < 1e-6 {
            range.minFrameDuration()
        } else if (pick.fps - best.min_fps).abs() < 1e-6 {
            range.maxFrameDuration()
        } else {
            CMTime {
                value: 1000,
                timescale: (pick.fps * 1000.0).round() as i32,
                flags: CMTimeFlags::Valid,
                epoch: 0,
            }
        }
    };

    // SAFETY: The device is locked for configuration while its format is changed, and
    // unlocked again afterwards, even when a step fails.
    unsafe {
        if device.lockForConfiguration().is_err() {
            return false;
        }
        device.setActiveFormat(&format);
        device.setActiveVideoMinFrameDuration(duration);
        device.setActiveVideoMaxFrameDuration(duration);
        device.unlockForConfiguration();
    }
    true
}

/// Fallback when the format cannot be chosen directly: let the session pick a size.
fn use_preset(session: &AVCaptureSession, width: u32) {
    // SAFETY: Presets are constants exported by AVFoundation; `canSetSessionPreset` is
    // checked first.
    unsafe {
        let preset = if width >= 1900 {
            AVCaptureSessionPreset1920x1080
        } else if width >= 1200 {
            AVCaptureSessionPreset1280x720
        } else {
            AVCaptureSessionPreset640x480
        };
        if session.canSetSessionPreset(preset) {
            session.setSessionPreset(preset);
        }
    }
}

impl CameraCapture for AvCapture {
    fn start(&mut self) -> Result<()> {
        if self.running {
            return Ok(());
        }
        // SAFETY: Plain calls on a valid session. `startRunning` blocks until the camera
        // is on, so call this from a worker thread.
        let is_running = unsafe {
            self.session.startRunning();
            self.session.isRunning()
        };
        if !is_running {
            return Err(StudioError::Backend(
                "the camera did not start. Check that no other app is using it and that camera access is allowed."
                    .to_owned(),
            ));
        }
        self.running = true;
        Ok(())
    }

    fn next_frame(&mut self, timeout: Duration) -> Result<Option<VideoFrame>> {
        if !self.running {
            return Err(StudioError::Backend("the camera is not started".to_owned()));
        }
        Ok(self.frames.pop(timeout))
    }

    fn stop(&mut self) {
        if self.running {
            // SAFETY: Plain call on a valid session.
            unsafe { self.session.stopRunning() };
            self.running = false;
        }
        self.frames.clear();
    }

    fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for AvCapture {
    fn drop(&mut self) {
        self.stop();
        // SAFETY: Removing the delegate makes sure no callback runs after the delegate is
        // gone.
        unsafe { self.output.setSampleBufferDelegate_queue(None, None) };
    }
}
