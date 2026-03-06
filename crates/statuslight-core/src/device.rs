//! Device abstraction and HID communication for status lights.
//!
//! The [`StatusLightDevice`] trait defines the interface for controlling a
//! status light device. [`HidSlickyDevice`] provides the Slicky HID-backed
//! implementation. Unit testing the HID layer requires a physical device and
//! is done manually.

use crate::color::Color;
use crate::error::{Result, StatusLightError};
use crate::protocol::{self, BUFFER_SIZE, PRODUCT_ID, VENDOR_ID};

/// Trait for controlling a status light device. Enables mocking in tests.
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
}

/// Real HID-backed Slicky device.
pub struct HidSlickyDevice {
    device: hidapi::HidDevice,
    serial: Option<String>,
}

impl HidSlickyDevice {
    /// Open the first Slicky device found on the USB bus.
    ///
    /// Returns [`StatusLightError::DeviceNotFound`] if no device is connected,
    /// or [`StatusLightError::MultipleDevices`] if more than one is found.
    pub fn open() -> Result<Self> {
        let api = hidapi::HidApi::new()?;
        let devices: Vec<_> = api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
            .collect();

        match devices.len() {
            0 => Err(StatusLightError::DeviceNotFound),
            1 => {
                let serial = devices[0].serial_number().map(|s| s.to_string());
                let device = devices[0].open_device(&api)?;
                Ok(Self { device, serial })
            }
            count => Err(StatusLightError::MultipleDevices { count }),
        }
    }

    /// Open a Slicky device by its serial number.
    pub fn open_serial(serial: &str) -> Result<Self> {
        let api = hidapi::HidApi::new()?;
        let info = api
            .device_list()
            .find(|d| {
                d.vendor_id() == VENDOR_ID
                    && d.product_id() == PRODUCT_ID
                    && d.serial_number().is_some_and(|s| s == serial)
            })
            .ok_or(StatusLightError::DeviceNotFound)?;

        let serial = info.serial_number().map(|s| s.to_string());
        let device = info.open_device(&api)?;
        Ok(Self { device, serial })
    }

    /// List all connected Slicky devices.
    pub fn enumerate() -> Result<Vec<DeviceInfo>> {
        let api = hidapi::HidApi::new()?;
        let devices = api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
            .map(|d| DeviceInfo {
                path: d.path().to_string_lossy().to_string(),
                serial: d.serial_number().map(|s| s.to_string()),
                manufacturer: d.manufacturer_string().map(|s| s.to_string()),
                product: d.product_string().map(|s| s.to_string()),
            })
            .collect();
        Ok(devices)
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
