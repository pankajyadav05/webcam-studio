//! Virtual camera and microphone for Linux.
//!
//! Nothing is implemented yet: [`create_camera`] and [`create_microphone`] return
//! `StudioError::Unsupported`. This crate compiles on every operating system so the
//! workspace builds everywhere; only the `studio-vdevice` facade picks it on Linux.
//!
//! # Plan for the camera
//!
//! The main path is a PipeWire video source node (`media.class=Video/Source`,
//! `media.role=Camera`). It needs no kernel module. The catch is that sandboxed apps
//! that reach cameras through the desktop portal filter out software nodes, so some
//! browsers and Flatpak apps will not list it.
//!
//! The fallback is v4l2loopback, which creates a `/dev/videoN` device (YUYV or NV12)
//! that every app can see. It needs the kernel module installed, so it stays optional.
//!
//! # Plan for the microphone
//!
//! A PipeWire node with `media.class=Audio/Source`, fed with our denoised audio in the
//! style of a PipeWire filter-chain. No kernel module is needed.

use studio_core::device::{VirtualCamera, VirtualMicrophone};
use studio_core::error::{Result, StudioError};

/// Short description of the backend this crate will use.
pub fn backend_name() -> &'static str {
    "PipeWire / v4l2loopback (planned)"
}

/// Creates a virtual camera called `name`.
///
/// Not implemented yet: always returns `StudioError::Unsupported`.
pub fn create_camera(name: &str) -> Result<Box<dyn VirtualCamera>> {
    let _ = name;
    Err(StudioError::Unsupported(
        "Linux virtual camera is not implemented yet (PipeWire Video/Source node and v4l2loopback fallback are planned)",
    ))
}

/// Creates a virtual microphone called `name`.
///
/// Not implemented yet: always returns `StudioError::Unsupported`.
pub fn create_microphone(name: &str) -> Result<Box<dyn VirtualMicrophone>> {
    let _ = name;
    Err(StudioError::Unsupported(
        "Linux virtual microphone is not implemented yet (PipeWire Audio/Source node is planned)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_implemented_yet() {
        assert!(matches!(
            create_camera("Studio"),
            Err(StudioError::Unsupported(_))
        ));
        assert!(matches!(
            create_microphone("Studio"),
            Err(StudioError::Unsupported(_))
        ));
        assert!(backend_name().contains("PipeWire"));
    }
}
