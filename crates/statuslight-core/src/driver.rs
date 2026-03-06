//! Pluggable device driver abstraction.
//!
//! The [`DeviceDriver`] trait allows different USB status light hardware
//! to be discovered and opened through a uniform interface.

use crate::{DeviceInfo, Result, StatusLightDevice};

/// Describes a supported hardware model for user-facing listings.
#[derive(Debug, Clone)]
pub struct SupportedDevice {
    /// Marketing/common name (e.g. "Busylight UC Omega").
    pub name: String,
    /// USB Vendor ID.
    pub vid: u16,
    /// USB Product ID.
    pub pid: u16,
}

/// A driver that can discover and open devices of a specific type.
pub trait DeviceDriver: Send + Sync {
    /// Unique driver identifier (e.g. "slicky", "arduino-rgb").
    fn id(&self) -> &str;

    /// Human-readable name (e.g. "Slicky USB Light").
    fn display_name(&self) -> &str;

    /// List all hardware models this driver supports (for `supported` command).
    fn supported_hardware(&self) -> Vec<SupportedDevice>;

    /// Enumerate all connected devices this driver supports.
    fn enumerate(&self) -> Result<Vec<DeviceInfo>>;

    /// Open the first available device.
    fn open(&self) -> Result<Box<dyn StatusLightDevice>>;

    /// Open a device by serial number.
    fn open_serial(&self, serial: &str) -> Result<Box<dyn StatusLightDevice>>;
}
