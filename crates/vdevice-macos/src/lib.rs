//! Virtual camera and microphone for macOS.
//!
//! Nothing is implemented yet: [`create_camera`] and [`create_microphone`] return
//! `StudioError::Unsupported`. This crate compiles on every operating system so the
//! workspace builds everywhere; only the `studio-vdevice` facade picks it on macOS.
//!
//! # Plan for the camera
//!
//! A CoreMediaIO Camera Extension: a system extension written in Swift and built by
//! Xcode, kept in `platform/macos/CameraExtension`. The app sends it frames through an
//! IOSurface-backed sink stream. Shipping it needs Developer ID signing, notarization,
//! approval by the user in System Settings, and the app installed in `/Applications`.
//!
//! # Plan for the microphone
//!
//! A Core Audio AudioServerPlugIn (a HAL driver), derived from BlackHole. BlackHole is
//! GPL-3.0, which is compatible with this project. It is installed into
//! `/Library/Audio/Plug-Ins/HAL` and needs a `coreaudiod` restart.

use studio_core::device::{VirtualCamera, VirtualMicrophone};
use studio_core::error::{Result, StudioError};

/// Short description of the backend this crate will use.
pub fn backend_name() -> &'static str {
    "CoreMediaIO Camera Extension (planned)"
}

/// Creates a virtual camera called `name`.
///
/// Not implemented yet: always returns `StudioError::Unsupported`.
pub fn create_camera(name: &str) -> Result<Box<dyn VirtualCamera>> {
    let _ = name;
    Err(StudioError::Unsupported(
        "macOS virtual camera is not implemented yet (CoreMediaIO Camera Extension with an IOSurface sink stream is planned)",
    ))
}

/// Creates a virtual microphone called `name`.
///
/// Not implemented yet: always returns `StudioError::Unsupported`.
pub fn create_microphone(name: &str) -> Result<Box<dyn VirtualMicrophone>> {
    let _ = name;
    Err(StudioError::Unsupported(
        "macOS virtual microphone is not implemented yet (Core Audio AudioServerPlugIn HAL driver is planned)",
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
        assert!(backend_name().contains("CoreMediaIO"));
    }
}
