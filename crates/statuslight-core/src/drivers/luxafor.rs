//! Luxafor Flag USB light device driver.
//!
//! The Luxafor Flag is an HID device that accepts 8-byte output reports.
//!
//! ## Protocol
//!
//! | Byte | Value | Description |
//! |------|-------|-------------|
//! | 0 | 0x01 | Command: steady color |
//! | 1 | 0xFF | LED selection: all LEDs |
//! | 2 | R | Red channel (0–255) |
//! | 3 | G | Green channel (0–255) |
//! | 4 | B | Blue channel (0–255) |
//! | 5–7 | 0x00 | Padding |

use crate::color::Color;
use crate::device::DeviceInfo;
use crate::drivers::hid_helpers;
use crate::error::{Result, StatusLightError};
use crate::{DeviceDriver, StatusLightDevice, SupportedDevice};

/// Luxafor VID/PID.
const VID_PID: &[(u16, u16)] = &[(0x04d8, 0xf372)];

const CMD_STEADY: u8 = 0x01;
const LED_ALL: u8 = 0xFF;
const REPORT_SIZE: usize = 8;

/// Build the 8-byte steady color report.
pub fn build_steady_color_report(color: Color) -> [u8; REPORT_SIZE] {
    [CMD_STEADY, LED_ALL, color.r, color.g, color.b, 0, 0, 0]
}

/// HID-backed Luxafor device.
pub struct HidLuxaforDevice {
    device: hidapi::HidDevice,
    serial: Option<String>,
}

impl StatusLightDevice for HidLuxaforDevice {
    fn driver_name(&self) -> &str {
        "Luxafor"
    }

    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }

    fn set_color(&self, color: Color) -> Result<()> {
        let report = build_steady_color_report(color);
        let written = self.device.write(&report)?;
        if written != REPORT_SIZE {
            return Err(StatusLightError::WriteMismatch {
                expected: REPORT_SIZE,
                actual: written,
            });
        }
        log::debug!("Luxafor: set color to {color}");
        Ok(())
    }
}

/// Driver for Luxafor Flag USB lights.
pub struct LuxaforDriver;

impl DeviceDriver for LuxaforDriver {
    fn id(&self) -> &str {
        "luxafor"
    }

    fn display_name(&self) -> &str {
        "Luxafor Flag"
    }

    fn supported_hardware(&self) -> Vec<SupportedDevice> {
        vec![SupportedDevice {
            name: "Luxafor Flag / Orb / Mute / Bluetooth".into(),
            vid: 0x04d8,
            pid: 0xf372,
        }]
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
        hid_helpers::enumerate_hid(VID_PID, "luxafor")
    }

    fn open(&self) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_first_hid(VID_PID)?;
        Ok(Box::new(HidLuxaforDevice { device, serial }))
    }

    fn open_serial(&self, serial: &str) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_hid_by_serial(VID_PID, serial)?;
        Ok(Box::new(HidLuxaforDevice { device, serial }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steady_color_report_format() {
        let report = build_steady_color_report(Color::new(255, 128, 0));
        assert_eq!(report[0], CMD_STEADY);
        assert_eq!(report[1], LED_ALL);
        assert_eq!(report[2], 255); // R
        assert_eq!(report[3], 128); // G
        assert_eq!(report[4], 0); // B
        assert_eq!(report[5], 0);
        assert_eq!(report[6], 0);
        assert_eq!(report[7], 0);
    }

    #[test]
    fn steady_color_report_size() {
        let report = build_steady_color_report(Color::off());
        assert_eq!(report.len(), REPORT_SIZE);
    }

    #[test]
    fn off_report_has_zero_colors() {
        let report = build_steady_color_report(Color::off());
        assert_eq!(report[2], 0);
        assert_eq!(report[3], 0);
        assert_eq!(report[4], 0);
    }
}
