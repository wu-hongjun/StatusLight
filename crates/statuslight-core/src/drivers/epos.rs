//! EPOS (formerly Sennheiser) Busylight USB device driver.
//!
//! The EPOS Busylight uses HID output reports with a 10-byte packet.
//! It has **2 independently addressable LEDs** but we set both to the
//! same color for uniform status indication.
//!
//! ## Protocol (10 bytes)
//!
//! | Byte | Value | Description |
//! |------|-------|-------------|
//! | 0 | 0x01 | Report ID |
//! | 1 | 0x12 | Action high byte (SetColor) |
//! | 2 | 0x02 | Action low byte (SetColor) |
//! | 3 | R | LED 0 Red (0-255) |
//! | 4 | G | LED 0 Green (0-255) |
//! | 5 | B | LED 0 Blue (0-255) |
//! | 6 | R | LED 1 Red (0-255) |
//! | 7 | G | LED 1 Green (0-255) |
//! | 8 | B | LED 1 Blue (0-255) |
//! | 9 | 0x00 | Reserved |

use crate::color::Color;
use crate::device::DeviceInfo;
use crate::drivers::hid_helpers;
use crate::error::{Result, StatusLightError};
use crate::{DeviceDriver, StatusLightDevice, SupportedDevice};

/// EPOS VID/PID.
const VID_PID: &[(u16, u16)] = &[(0x1395, 0x0074)];

const REPORT_ID: u8 = 0x01;
const ACTION_SET_COLOR_HI: u8 = 0x12;
const ACTION_SET_COLOR_LO: u8 = 0x02;
const REPORT_SIZE: usize = 10;

/// Build the 10-byte color report (both LEDs set to same color).
pub fn build_color_report(color: Color) -> [u8; REPORT_SIZE] {
    [
        REPORT_ID,
        ACTION_SET_COLOR_HI,
        ACTION_SET_COLOR_LO,
        color.r,
        color.g,
        color.b,
        color.r,
        color.g,
        color.b,
        0x00,
    ]
}

/// Build a 10-byte off report (all zeros).
pub fn build_off_report() -> [u8; REPORT_SIZE] {
    [0u8; REPORT_SIZE]
}

/// HID-backed EPOS Busylight device.
pub struct HidEposDevice {
    device: hidapi::HidDevice,
    serial: Option<String>,
}

impl StatusLightDevice for HidEposDevice {
    fn driver_name(&self) -> &str {
        "EPOS"
    }

    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }

    fn set_color(&self, color: Color) -> Result<()> {
        let report = if color.is_off() {
            build_off_report()
        } else {
            build_color_report(color)
        };
        let written = self.device.write(&report)?;
        if written != REPORT_SIZE {
            return Err(StatusLightError::WriteMismatch {
                expected: REPORT_SIZE,
                actual: written,
            });
        }
        log::debug!("EPOS: set color to {color}");
        Ok(())
    }
}

/// Driver for EPOS Busylight USB lights.
pub struct EposDriver;

impl DeviceDriver for EposDriver {
    fn id(&self) -> &str {
        "epos"
    }

    fn display_name(&self) -> &str {
        "EPOS Busylight"
    }

    fn supported_hardware(&self) -> Vec<SupportedDevice> {
        vec![SupportedDevice {
            name: "EPOS Busylight".into(),
            vid: 0x1395,
            pid: 0x0074,
        }]
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
        hid_helpers::enumerate_hid(VID_PID, "epos")
    }

    fn open(&self) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_first_hid(VID_PID)?;
        Ok(Box::new(HidEposDevice { device, serial }))
    }

    fn open_serial(&self, serial: &str) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_hid_by_serial(VID_PID, serial)?;
        Ok(Box::new(HidEposDevice { device, serial }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_report_format() {
        let report = build_color_report(Color::new(255, 128, 64));
        assert_eq!(report[0], REPORT_ID);
        assert_eq!(report[1], ACTION_SET_COLOR_HI);
        assert_eq!(report[2], ACTION_SET_COLOR_LO);
        // LED 0
        assert_eq!(report[3], 255, "LED0 Red");
        assert_eq!(report[4], 128, "LED0 Green");
        assert_eq!(report[5], 64, "LED0 Blue");
        // LED 1 (same color)
        assert_eq!(report[6], 255, "LED1 Red");
        assert_eq!(report[7], 128, "LED1 Green");
        assert_eq!(report[8], 64, "LED1 Blue");
        assert_eq!(report[9], 0x00, "reserved");
    }

    #[test]
    fn both_leds_match() {
        let report = build_color_report(Color::new(10, 20, 30));
        assert_eq!(report[3], report[6], "both LEDs red should match");
        assert_eq!(report[4], report[7], "both LEDs green should match");
        assert_eq!(report[5], report[8], "both LEDs blue should match");
    }

    #[test]
    fn off_report_is_all_zeros() {
        let report = build_off_report();
        assert_eq!(report, [0u8; REPORT_SIZE]);
    }

    #[test]
    fn report_size() {
        let report = build_color_report(Color::new(0, 255, 0));
        assert_eq!(report.len(), REPORT_SIZE);
    }

    #[test]
    fn action_code_is_set_color() {
        let report = build_color_report(Color::new(0, 0, 255));
        let action = ((report[1] as u16) << 8) | (report[2] as u16);
        assert_eq!(action, 0x1202, "action should be SetColor (0x1202)");
    }
}
