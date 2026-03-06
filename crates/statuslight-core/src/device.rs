//! Device abstraction and HID communication for Slicky lights.
//!
//! The [`SlickyDevice`] trait defines the interface for controlling a Slicky
//! device. [`HidSlickyDevice`] provides the real HID-backed implementation.
//! Unit testing the HID layer requires a physical device and is done manually.

use crate::color::Color;
use crate::error::{Result, SlickyError};
use crate::protocol::{self, BUFFER_SIZE, PRODUCT_ID, VENDOR_ID};

/// Trait for controlling a Slicky device. Enables mocking in tests.
pub trait SlickyDevice {
    /// Set the device to the given color.
    fn set_color(&self, color: Color) -> Result<()>;

    /// Turn the device off (set to black).
    fn off(&self) -> Result<()> {
        self.set_color(Color::off())
    }
}

/// Info about a connected Slicky device (from enumeration).
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
}

impl HidSlickyDevice {
    /// Open the first Slicky device found on the USB bus.
    ///
    /// Returns [`SlickyError::DeviceNotFound`] if no device is connected,
    /// or [`SlickyError::MultipleDevices`] if more than one is found.
    pub fn open() -> Result<Self> {
        let api = hidapi::HidApi::new()?;
        let devices: Vec<_> = api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID && d.product_id() == PRODUCT_ID)
            .collect();

        match devices.len() {
            0 => Err(SlickyError::DeviceNotFound),
            1 => {
                let device = devices[0].open_device(&api)?;
                Ok(Self { device })
            }
            count => Err(SlickyError::MultipleDevices { count }),
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
            .ok_or(SlickyError::DeviceNotFound)?;

        let device = info.open_device(&api)?;
        Ok(Self { device })
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

impl SlickyDevice for HidSlickyDevice {
    fn set_color(&self, color: Color) -> Result<()> {
        let report = protocol::build_set_color_report(color);
        let written = self.device.write(&report)?;
        if written != BUFFER_SIZE {
            return Err(SlickyError::WriteMismatch {
                expected: BUFFER_SIZE,
                actual: written,
            });
        }
        log::debug!("Set color to {color}");
        Ok(())
    }
}
