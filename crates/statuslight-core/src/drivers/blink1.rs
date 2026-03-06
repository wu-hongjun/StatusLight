//! Blink(1) USB light device driver.
//!
//! The Blink(1) uses HID **feature reports** (not output reports).
//!
//! ## Protocol
//!
//! | Byte | Value | Description |
//! |------|-------|-------------|
//! | 0 | 0x01 | Report ID |
//! | 1 | 0x6E | Command: set color now ('n') |
//! | 2 | R | Red channel (0–255) |
//! | 3 | G | Green channel (0–255) |
//! | 4 | B | Blue channel (0–255) |
//! | 5–6 | 0x00 | Reserved |
//! | 7 | 0x00 | LED index: 0=both, 1=top, 2=bottom |
//! | 8 | 0x00 | Reserved |

use crate::color::Color;
use crate::device::DeviceInfo;
use crate::drivers::hid_helpers;
use crate::error::Result;
use crate::{DeviceDriver, StatusLightDevice, SupportedDevice};

/// Blink(1) VID/PID.
const VID_PID: &[(u16, u16)] = &[(0x27b8, 0x01ed)];

const REPORT_ID: u8 = 0x01;
const CMD_SET_COLOR: u8 = 0x6E; // 'n' — set color now
const LED_BOTH: u8 = 0x00;
const REPORT_SIZE: usize = 9;

/// Build the 9-byte feature report for setting a color.
pub fn build_set_color_report(color: Color) -> [u8; REPORT_SIZE] {
    [
        REPORT_ID,
        CMD_SET_COLOR,
        color.r,
        color.g,
        color.b,
        0,
        0,
        LED_BOTH,
        0,
    ]
}

/// HID-backed Blink(1) device.
pub struct HidBlink1Device {
    device: hidapi::HidDevice,
    serial: Option<String>,
}

impl StatusLightDevice for HidBlink1Device {
    fn driver_name(&self) -> &str {
        "Blink(1)"
    }

    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }

    fn set_color(&self, color: Color) -> Result<()> {
        let report = build_set_color_report(color);
        // Blink(1) uses feature reports, not output reports.
        self.device.send_feature_report(&report)?;
        log::debug!("Blink(1): set color to {color}");
        Ok(())
    }
}

/// Driver for Blink(1) USB lights.
pub struct Blink1Driver;

impl DeviceDriver for Blink1Driver {
    fn id(&self) -> &str {
        "blink1"
    }

    fn display_name(&self) -> &str {
        "Blink(1)"
    }

    fn supported_hardware(&self) -> Vec<SupportedDevice> {
        vec![SupportedDevice {
            name: "Blink(1) mk1/mk2/mk3".into(),
            vid: 0x27b8,
            pid: 0x01ed,
        }]
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
        hid_helpers::enumerate_hid(VID_PID, "blink1")
    }

    fn open(&self) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_first_hid(VID_PID)?;
        Ok(Box::new(HidBlink1Device { device, serial }))
    }

    fn open_serial(&self, serial: &str) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_hid_by_serial(VID_PID, serial)?;
        Ok(Box::new(HidBlink1Device { device, serial }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_color_report_format() {
        let report = build_set_color_report(Color::new(255, 128, 0));
        assert_eq!(report[0], REPORT_ID);
        assert_eq!(report[1], CMD_SET_COLOR);
        assert_eq!(report[2], 255); // R
        assert_eq!(report[3], 128); // G
        assert_eq!(report[4], 0); // B
        assert_eq!(report[5], 0);
        assert_eq!(report[6], 0);
        assert_eq!(report[7], LED_BOTH);
        assert_eq!(report[8], 0);
    }

    #[test]
    fn report_size() {
        let report = build_set_color_report(Color::off());
        assert_eq!(report.len(), REPORT_SIZE);
    }

    #[test]
    fn report_uses_feature_report_cmd() {
        let report = build_set_color_report(Color::new(0, 255, 0));
        assert_eq!(report[1], 0x6E, "command should be 'n' (0x6E)");
    }
}
