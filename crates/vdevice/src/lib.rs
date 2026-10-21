//! One entry point for virtual cameras and microphones on every operating system.
//!
//! This crate picks the backend for the operating system it is built for: Linux,
//! Windows or macOS. On any other system the `create_*` functions return
//! `StudioError::Unsupported`. The platform backends are not implemented yet, so for
//! now they return `Unsupported` as well. Use [`null`] for devices that work in memory
//! only, for example in tests.

pub mod null;

pub use studio_core::device::{OutputFormat, VirtualCamera, VirtualMicrophone};

#[cfg(target_os = "linux")]
use studio_vdevice_linux as platform;
#[cfg(target_os = "macos")]
use studio_vdevice_macos as platform;
#[cfg(target_os = "windows")]
use studio_vdevice_windows as platform;

/// Short description of the backend used on this operating system.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn backend_name() -> &'static str {
    platform::backend_name()
}

/// Short description of the backend used on this operating system.
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn backend_name() -> &'static str {
    "unsupported platform"
}

/// Creates a virtual camera called `name` using this operating system's backend.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn create_camera(name: &str) -> studio_core::error::Result<Box<dyn VirtualCamera>> {
    platform::create_camera(name)
}

/// Creates a virtual camera called `name` using this operating system's backend.
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn create_camera(name: &str) -> studio_core::error::Result<Box<dyn VirtualCamera>> {
    let _ = name;
    Err(studio_core::error::StudioError::Unsupported(
        "virtual cameras are not available on this operating system",
    ))
}

/// Creates a virtual microphone called `name` using this operating system's backend.
#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub fn create_microphone(name: &str) -> studio_core::error::Result<Box<dyn VirtualMicrophone>> {
    platform::create_microphone(name)
}

/// Creates a virtual microphone called `name` using this operating system's backend.
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub fn create_microphone(name: &str) -> studio_core::error::Result<Box<dyn VirtualMicrophone>> {
    let _ = name;
    Err(studio_core::error::StudioError::Unsupported(
        "virtual microphones are not available on this operating system",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use studio_core::error::StudioError;

    #[test]
    fn backend_name_is_not_empty() {
        assert!(!backend_name().is_empty());
    }

    #[test]
    fn real_devices_are_unsupported_for_now() {
        assert!(matches!(
            create_camera("Studio"),
            Err(StudioError::Unsupported(_))
        ));
        assert!(matches!(
            create_microphone("Studio"),
            Err(StudioError::Unsupported(_))
        ));
    }
}
