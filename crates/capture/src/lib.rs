//! Native webcam capture.
//!
//! This crate reads frames from the real cameras attached to the computer. It is the first
//! step of the path a picture takes through Webcam Studio. On macOS the whole path is:
//!
//! 1. **AVFoundation capture.** [`open_camera`] opens one webcam. A background thread owned
//!    by AVFoundation delivers every picture as BGRA. The crate copies it into a
//!    [`VideoFrame`] and keeps only the two newest frames, so a slow reader never slows the
//!    camera down. [`CameraCapture::next_frame`] hands them out.
//! 2. **Engine.** The engine applies zoom, blur and the other effects.
//! 3. **`MacVirtualCamera`** (crate `studio-vdevice-macos`) copies the finished frame into
//!    shared memory and puts it on the *sink stream* of the Webcam Studio Camera.
//! 4. **Camera Extension.** A small program that macOS runs takes the frame from the sink
//!    stream and passes it to its *source stream*. It shows a placeholder if no frames
//!    arrive.
//! 5. **Meeting apps.** Zoom, Teams, Safari and the others read the source stream like any
//!    other camera.
//!
//! # Camera access and activation
//!
//! Two things need the user's permission on macOS, and both are asked once:
//!
//! - **Camera access.** Call [`request_camera_permission`] when the app starts. macOS shows
//!   its own dialog. The app's `Info.plist` needs a camera usage description or macOS ends
//!   the process. If the user says no, [`open_camera`] fails with a message that points to
//!   System Settings.
//! - **The virtual camera.** The Camera Extension has to be installed with
//!   `studio_vdevice::macos::request_activation`. macOS then asks the user to allow it in
//!   System Settings. After that the Webcam Studio Camera exists, and `MacVirtualCamera`
//!   can send frames to it.
//!
//! [`list_cameras`] never lists the Webcam Studio Camera itself. If it did, a user could
//! pick it as the input and feed the output back into the app.
//!
//! # Other systems
//!
//! Linux and Windows return `StudioError::Unsupported` for now. V4L2 and PipeWire on Linux,
//! and Media Foundation on Windows, are planned.

use std::time::Duration;

use studio_core::error::Result;
#[cfg(not(target_os = "macos"))]
use studio_core::error::StudioError;
use studio_core::frame::VideoFrame;

mod format;
mod queue;
mod rows;

#[cfg(target_os = "macos")]
mod macos;

/// A camera the user can choose.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CameraInfo {
    /// Stable ID to pass to [`open_camera`].
    pub id: String,
    /// Name to show to the user.
    pub name: String,
}

/// A camera that is delivering frames.
pub trait CameraCapture: Send {
    /// Starts delivering frames.
    fn start(&mut self) -> Result<()>;
    /// Waits up to `timeout` for the next frame. Returns `Ok(None)` if none arrived in time.
    ///
    /// Frames come out in the size the camera was set to, which can differ from the size
    /// asked for in [`open_camera`] when the camera does not support it. They are BGRA with
    /// no row padding.
    fn next_frame(&mut self, timeout: Duration) -> Result<Option<VideoFrame>>;
    /// Stops delivering frames. The camera can be started again.
    fn stop(&mut self);
    /// Name of the camera.
    fn name(&self) -> &str;
}

/// Returns `true` for the Webcam Studio virtual camera, which must not be offered as an
/// input.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn is_own_virtual_camera(id: &str, name: &str) -> bool {
    id.eq_ignore_ascii_case(studio_vdevice_macos::DEVICE_UUID)
        || name == studio_vdevice_macos::DEVICE_NAME
}

/// Lists the cameras that are connected now, without the Webcam Studio virtual camera.
///
/// On macOS this does not need camera permission. An empty list is a normal answer.
#[cfg(target_os = "macos")]
pub fn list_cameras() -> Result<Vec<CameraInfo>> {
    macos::list_cameras()
}

/// Lists the cameras that are connected now.
///
/// Not implemented on this system yet: always returns `StudioError::Unsupported`.
#[cfg(not(target_os = "macos"))]
pub fn list_cameras() -> Result<Vec<CameraInfo>> {
    Err(unsupported())
}

/// Opens the camera with the given `id` (from [`list_cameras`]) and asks for a picture of
/// about `width` x `height` at `fps` frames per second. The closest format the camera
/// offers is used. Call `start` on the result to begin.
#[cfg(target_os = "macos")]
pub fn open_camera(id: &str, width: u32, height: u32, fps: u32) -> Result<Box<dyn CameraCapture>> {
    macos::open_camera(id, width, height, fps)
}

/// Opens a camera.
///
/// Not implemented on this system yet: always returns `StudioError::Unsupported`.
#[cfg(not(target_os = "macos"))]
pub fn open_camera(id: &str, width: u32, height: u32, fps: u32) -> Result<Box<dyn CameraCapture>> {
    let _ = (id, width, height, fps);
    Err(unsupported())
}

/// Asks the user for permission to use the camera, then calls `done` with the answer.
///
/// On macOS this shows the system dialog the first time. `done` can run on any thread. On
/// other systems there is no such step and `done(true)` runs at once.
#[cfg(target_os = "macos")]
pub fn request_camera_permission(done: Box<dyn FnOnce(bool) + Send>) {
    macos::request_camera_permission(done);
}

/// Asks the user for permission to use the camera, then calls `done` with the answer.
///
/// Other systems have no such step, so `done(true)` runs at once.
#[cfg(not(target_os = "macos"))]
pub fn request_camera_permission(done: Box<dyn FnOnce(bool) + Send>) {
    done(true);
}

#[cfg(not(target_os = "macos"))]
fn unsupported() -> StudioError {
    StudioError::Unsupported("native capture is not implemented on this platform yet")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_virtual_camera_is_recognised() {
        assert!(is_own_virtual_camera("anything", "Webcam Studio Camera"));
        assert!(is_own_virtual_camera(
            "e0a5d920-6291-412e-80c9-99704e4e5218",
            "Other name"
        ));
        assert!(!is_own_virtual_camera("0x1234", "FaceTime HD Camera"));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn capture_is_unsupported_off_macos() {
        assert!(matches!(list_cameras(), Err(StudioError::Unsupported(_))));
        assert!(matches!(
            open_camera("x", 1280, 720, 30),
            Err(StudioError::Unsupported(_))
        ));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn permission_is_granted_off_macos() {
        let (tx, rx) = std::sync::mpsc::channel();
        request_camera_permission(Box::new(move |granted| tx.send(granted).unwrap()));
        assert!(rx.recv().unwrap());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn listing_cameras_works_without_a_camera() {
        // CI machines have no camera, so an empty list is fine.
        let cameras = list_cameras().expect("listing should not fail");
        assert!(cameras.iter().all(|c| !c.id.is_empty()));
        assert!(
            cameras
                .iter()
                .all(|c| !is_own_virtual_camera(&c.id, &c.name))
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn unknown_camera_cannot_be_opened() {
        assert!(open_camera("nonexistent", 1280, 720, 30).is_err());
    }
}
