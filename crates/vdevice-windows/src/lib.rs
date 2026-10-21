//! Virtual camera and microphone for Windows.
//!
//! Nothing is implemented yet: [`create_camera`] and [`create_microphone`] return
//! `StudioError::Unsupported`. This crate compiles on every operating system so the
//! workspace builds everywhere; only the `studio-vdevice` facade picks it on Windows.
//!
//! # Plan for the camera
//!
//! Use the Media Foundation virtual camera API (`MFCreateVirtualCamera`), available on
//! Windows 11 build 22000 and later. The camera itself is a user-mode COM media source
//! DLL, built later as a separate `cdylib` crate. The app sends it frames over shared
//! memory. No kernel driver and no driver signing are needed.
//!
//! Open point to test early: whether apps that only know DirectShow can see a Media
//! Foundation virtual camera. Zoom, Teams and Chrome must be tried before we rely on it.
//! Windows 10 would need a separate DirectShow filter.
//!
//! # Plan for the microphone
//!
//! A virtual microphone on Windows needs a signed WaveRT kernel driver. Version 1 will
//! not ship one, so this is an open question (options: build and sign a driver, license
//! a commercial one, or process the real microphone instead).

use studio_core::device::{VirtualCamera, VirtualMicrophone};
use studio_core::error::{Result, StudioError};

/// Short description of the backend this crate will use.
pub fn backend_name() -> &'static str {
    "Media Foundation virtual camera (planned)"
}

/// Creates a virtual camera called `name`.
///
/// Not implemented yet: always returns `StudioError::Unsupported`.
pub fn create_camera(name: &str) -> Result<Box<dyn VirtualCamera>> {
    let _ = name;
    Err(StudioError::Unsupported(
        "Windows virtual camera is not implemented yet (MFCreateVirtualCamera with a COM media source DLL is planned)",
    ))
}

/// Creates a virtual microphone called `name`.
///
/// Not implemented yet: always returns `StudioError::Unsupported`.
pub fn create_microphone(name: &str) -> Result<Box<dyn VirtualMicrophone>> {
    let _ = name;
    Err(StudioError::Unsupported(
        "Windows virtual microphone is not planned for v1 (it needs a signed WaveRT kernel driver)",
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
        assert!(backend_name().contains("Media Foundation"));
    }
}
