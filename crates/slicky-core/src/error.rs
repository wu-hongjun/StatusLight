//! Error types for slicky-core.

/// All errors that can occur in slicky-core operations.
#[derive(Debug, thiserror::Error)]
pub enum SlickyError {
    /// No Slicky device was found on the USB bus.
    #[error("no Slicky device found (VID=0x04D8, PID=0xEC24)")]
    DeviceNotFound,

    /// Multiple Slicky devices found; a serial number is required to disambiguate.
    #[error("multiple Slicky devices found ({count}); specify a serial number")]
    MultipleDevices { count: usize },

    /// An error from the underlying HID library.
    #[error("USB HID error: {0}")]
    Hid(#[from] hidapi::HidError),

    /// The provided string is not a valid hex color.
    #[error("invalid hex color: {0}")]
    InvalidHexColor(String),

    /// The provided name does not match any known preset.
    #[error("unknown preset: {0}")]
    UnknownPreset(String),

    /// The HID write did not send the expected number of bytes.
    #[error("device write failed: expected {expected} bytes, got {actual}")]
    WriteMismatch { expected: usize, actual: usize },

    /// A custom preset with the same name already exists.
    #[error("duplicate preset: {0}")]
    DuplicatePreset(String),

    /// The requested preset was not found.
    #[error("preset not found: {0}")]
    PresetNotFound(String),
}

/// A type alias for `Result<T, SlickyError>`.
pub type Result<T> = std::result::Result<T, SlickyError>;
