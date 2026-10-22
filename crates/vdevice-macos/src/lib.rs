//! Virtual camera and microphone for macOS.
//!
//! The camera is real on macOS. The microphone is not written yet:
//! [`create_microphone`] returns `StudioError::Unsupported`. This crate compiles on every
//! operating system so the workspace builds everywhere. On other systems only the
//! constants and [`create_microphone`] / [`create_camera`] (which return `Unsupported`)
//! exist.
//!
//! # How a frame gets from your webcam to a meeting app
//!
//! macOS does not let an ordinary app publish a camera. It has to come from a *Camera
//! Extension*, a small program that macOS itself runs. Webcam Studio ships one, written in
//! Swift, in `platform/macos`. Here is the whole path:
//!
//! 1. The `studio-capture` crate reads the real webcam through AVFoundation.
//! 2. The engine applies zoom, blur and the other effects.
//! 3. [`MacVirtualCamera`] copies each finished frame into a pixel buffer that lives in
//!    shared memory (an IOSurface) and puts it on the *sink stream* of the extension's
//!    device. A sink stream is an input: it is the door the app uses to hand frames to the
//!    extension.
//! 4. The extension takes the frame from the sink stream and passes it on to its *source
//!    stream*. If no frames arrive, the extension shows a placeholder picture instead.
//! 5. Zoom, Teams, Safari and the other meeting apps read the source stream like any other
//!    camera. They see a camera called "Webcam Studio Camera".
//!
//! The format is fixed at 1280x720, 30 frames per second, 32-bit BGRA. The extension and
//! this crate agree on the constants in this file ([`DEVICE_UUID`] and the others). If you
//! change one, change the Swift side too.
//!
//! # Installing the extension
//!
//! The extension is installed with a system extension request. Call
//! [`request_activation`] (macOS only) from the app:
//!
//! 1. The request is sent to macOS. The app must be signed with a Developer ID, carry the
//!    system extension entitlement and live in `/Applications`, or the request fails.
//! 2. The first time, macOS asks the user to allow the extension in System Settings
//!    (Privacy & Security). The callback is told [`ActivationOutcome::NeedsUserApproval`]
//!    at that moment. This is not the end: when the user approves, the callback is called
//!    **again** with [`ActivationOutcome::Activated`] (or an error if they refuse).
//! 3. After that, [`is_extension_installed`] returns `true`, and
//!    [`MacVirtualCamera::start`] can find the device.
//!
//! [`request_deactivation`] removes the extension again.
//!
//! # Plan for the microphone
//!
//! A Core Audio AudioServerPlugIn (a HAL driver), derived from BlackHole. BlackHole is
//! GPL-3.0, which is compatible with this project. It is installed into
//! `/Library/Audio/Plug-Ins/HAL` and needs a `coreaudiod` restart.

use studio_core::device::{VirtualCamera, VirtualMicrophone};
use studio_core::error::{Result, StudioError};

mod pixels;

#[cfg(target_os = "macos")]
mod activation;
#[cfg(target_os = "macos")]
mod camera;
#[cfg(target_os = "macos")]
mod cmio;

#[cfg(target_os = "macos")]
pub use activation::{request_activation, request_deactivation};
#[cfg(target_os = "macos")]
pub use camera::MacVirtualCamera;

/// Bundle identifier of the Webcam Studio app.
pub const HOST_BUNDLE_ID: &str = "dev.pankajyadav.webcamstudio";
/// Bundle identifier of the Camera Extension. It sits inside the app at
/// `Contents/Library/SystemExtensions/`.
pub const EXTENSION_BUNDLE_ID: &str = "dev.pankajyadav.webcamstudio.CameraExtension";
/// Name other apps show for the camera.
pub const DEVICE_NAME: &str = "Webcam Studio Camera";
/// Unique ID of the camera device. macOS reports it as the device UID.
pub const DEVICE_UUID: &str = "E0A5D920-6291-412E-80C9-99704E4E5218";
/// ID of the output stream that meeting apps read.
pub const SOURCE_STREAM_UUID: &str = "83148DC8-A158-4E84-9166-75E50BB46FB1";
/// ID of the input stream the app writes frames to.
pub const SINK_STREAM_UUID: &str = "01C4DA86-551A-49BD-882D-170FABF06336";
/// Name of the input stream.
pub const SINK_STREAM_NAME: &str = "Webcam Studio Camera Input";
/// Width of every frame in pixels.
pub const OUTPUT_WIDTH: u32 = 1280;
/// Height of every frame in pixels.
pub const OUTPUT_HEIGHT: u32 = 720;
/// Frames per second.
pub const OUTPUT_FPS: u32 = 30;

/// What happened to an activation request. See [`request_activation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationOutcome {
    /// The extension is installed and running.
    Activated,
    /// The user has to allow the extension in System Settings. Another result follows once
    /// they have decided.
    NeedsUserApproval,
    /// The extension will start working after the next restart.
    WillCompleteAfterReboot,
}

/// Short description of the backend this crate uses.
pub fn backend_name() -> &'static str {
    "CoreMediaIO Camera Extension"
}

/// Creates a virtual camera called `name`.
///
/// On macOS this returns a [`MacVirtualCamera`]. Creating it does not touch the system;
/// the extension is looked up when you call `start`. On other systems it returns
/// `StudioError::Unsupported`.
#[cfg(target_os = "macos")]
pub fn create_camera(name: &str) -> Result<Box<dyn VirtualCamera>> {
    Ok(Box::new(MacVirtualCamera::new(name)))
}

/// Creates a virtual camera called `name`.
///
/// On macOS this returns a `MacVirtualCamera`. On other systems it returns
/// `StudioError::Unsupported`.
#[cfg(not(target_os = "macos"))]
pub fn create_camera(name: &str) -> Result<Box<dyn VirtualCamera>> {
    let _ = name;
    Err(StudioError::Unsupported(
        "the macOS virtual camera only exists on macOS",
    ))
}

/// Creates a virtual microphone called `name`.
///
/// Not implemented yet: always returns `StudioError::Unsupported`.
pub fn create_microphone(name: &str) -> Result<Box<dyn VirtualMicrophone>> {
    let _ = name;
    Err(StudioError::Unsupported(
        "macOS virtual microphone is not implemented yet (planned: Core Audio HAL driver)",
    ))
}

/// Returns `true` if the Camera Extension is installed and macOS lists its device.
///
/// This is quick and does not block. It only reads the list of camera devices.
#[cfg(target_os = "macos")]
pub fn is_extension_installed() -> bool {
    cmio::find_device().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks the 8-4-4-4-12 hexadecimal layout of a UUID.
    fn is_uuid(text: &str) -> bool {
        let parts: Vec<&str> = text.split('-').collect();
        let lengths = [8, 4, 4, 4, 12];
        parts.len() == lengths.len()
            && parts
                .iter()
                .zip(lengths)
                .all(|(part, len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
    }

    #[test]
    fn uuid_constants_are_valid_and_distinct() {
        let all = [DEVICE_UUID, SOURCE_STREAM_UUID, SINK_STREAM_UUID];
        for uuid in all {
            assert!(is_uuid(uuid), "{uuid} is not a UUID");
        }
        assert_ne!(DEVICE_UUID, SOURCE_STREAM_UUID);
        assert_ne!(DEVICE_UUID, SINK_STREAM_UUID);
        assert_ne!(SOURCE_STREAM_UUID, SINK_STREAM_UUID);
    }

    #[test]
    fn bundle_ids_belong_together() {
        assert!(EXTENSION_BUNDLE_ID.starts_with(HOST_BUNDLE_ID));
    }

    #[test]
    fn microphone_is_not_available() {
        assert!(matches!(
            create_microphone("Studio"),
            Err(StudioError::Unsupported(_))
        ));
        assert_eq!(backend_name(), "CoreMediaIO Camera Extension");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn camera_is_unsupported_off_macos() {
        assert!(matches!(
            create_camera("Studio"),
            Err(StudioError::Unsupported(_))
        ));
    }
}
