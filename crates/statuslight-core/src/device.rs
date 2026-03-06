//! Device abstraction and HID communication for status lights.
//!
//! The [`StatusLightDevice`] trait defines the interface for controlling a
//! status light device. [`HidSlickyDevice`] provides the Slicky HID-backed
//! implementation. Unit testing the HID layer requires a physical device and
//! is done manually.

use crate::color::Color;
use crate::drivers::hid_helpers;
use crate::error::{Result, StatusLightError};
use crate::protocol::{self, BUFFER_SIZE, PRODUCT_ID, VENDOR_ID};

/// Trait for controlling a status light device. Enables mocking in tests.
///
/// # Thread Safety
///
/// This trait requires `Send` but intentionally does **not** require `Sync`.
/// `hidapi::HidDevice` is `Send` but not `Sync`, so requiring `Sync` would
/// force every driver to wrap its device handle in a `Mutex`, adding
/// unnecessary complexity when the daemon already serializes access.
pub trait StatusLightDevice: Send {
    /// Human-readable driver name (e.g. "Slicky", "Arduino RGB").
    fn driver_name(&self) -> &str {
        "unknown"
    }

    /// Device serial number, if available.
    fn serial(&self) -> Option<&str> {
        None
    }

    /// Set the device to the given color.
    fn set_color(&self, color: Color) -> Result<()>;

    /// Turn the device off (set to black).
    fn off(&self) -> Result<()> {
        self.set_color(Color::off())
    }
}

/// Info about a connected device (from enumeration).
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// OS-specific device path.
    pub path: String,
    /// Device serial number, if available.
    pub serial: Option<String>,
    /// Manufacturer string, if available.
    pub manufacturer: Option<String>,
    /// Product string, if available.
    pub product: Option<String>,
    /// USB Vendor ID.
    pub vid: u16,
    /// USB Product ID.
    pub pid: u16,
    /// Driver identifier (e.g. "slicky", "luxafor").
    pub driver_id: String,
}

/// Real HID-backed Slicky device.
pub struct HidSlickyDevice {
    device: hidapi::HidDevice,
    serial: Option<String>,
}

/// VID/PID pairs for Slicky devices.
const SLICKY_VID_PID: &[(u16, u16)] = &[(VENDOR_ID, PRODUCT_ID)];

impl HidSlickyDevice {
    /// Open the first Slicky device found on the USB bus.
    ///
    /// Returns [`StatusLightError::DeviceNotFound`] if no device is connected,
    /// or [`StatusLightError::MultipleDevices`] if more than one is found.
    pub fn open(api: &hidapi::HidApi) -> Result<Self> {
        let devices = Self::enumerate(api)?;
        match devices.len() {
            0 => Err(StatusLightError::DeviceNotFound),
            1 => {
                let (device, serial) = hid_helpers::open_first_hid(api, SLICKY_VID_PID)?;
                Ok(Self { device, serial })
            }
            count => Err(StatusLightError::MultipleDevices { count }),
        }
    }

    /// Open a Slicky device by its serial number.
    pub fn open_serial(api: &hidapi::HidApi, serial: &str) -> Result<Self> {
        let (device, serial) = hid_helpers::open_hid_by_serial(api, SLICKY_VID_PID, serial)?;
        Ok(Self { device, serial })
    }

    /// List all connected Slicky devices.
    pub fn enumerate(api: &hidapi::HidApi) -> Result<Vec<DeviceInfo>> {
        hid_helpers::enumerate_hid(api, SLICKY_VID_PID, "slicky")
    }
}

impl StatusLightDevice for HidSlickyDevice {
    fn driver_name(&self) -> &str {
        "Slicky"
    }

    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }

    fn set_color(&self, color: Color) -> Result<()> {
        let report = protocol::build_set_color_report(color);
        let written = self.device.write(&report)?;
        if written != BUFFER_SIZE {
            return Err(StatusLightError::WriteMismatch {
                expected: BUFFER_SIZE,
                actual: written,
            });
        }
        log::debug!("Set color to {color}");
        Ok(())
    }
}
