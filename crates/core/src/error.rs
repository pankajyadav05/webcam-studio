//! Error type shared by all Webcam Studio crates.

/// Errors that can come out of Webcam Studio.
#[derive(Debug, thiserror::Error)]
pub enum StudioError {
    /// The feature does not exist on this operating system.
    #[error("not supported on this platform: {0}")]
    Unsupported(&'static str),
    /// A device driver or system call failed.
    #[error("device backend error: {0}")]
    Backend(String),
    /// The caller passed a value that cannot be used.
    #[error("invalid input: {0}")]
    Invalid(String),
}

/// Result type that uses [`StudioError`].
pub type Result<T> = std::result::Result<T, StudioError>;
