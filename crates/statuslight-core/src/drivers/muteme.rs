//! MuteMe USB light device driver.
//!
//! The MuteMe (Original and Mini) uses HID output reports with **1-bit
//! color fields** — each RGB channel is either fully on or fully off.
//! Only 7 colors (plus off) are possible.
//!
//! ## Protocol (2 bytes)
//!
//! | Byte | Value | Description |
//! |------|-------|-------------|
//! | 0 | 0x00 | Padding / Report ID |
//! | 1 | cmd | Command byte (see below) |
//!
//! ### Command byte bit layout
//!
//! | Bit | Field | Description |
//! |-----|-------|-------------|
//! | 0 | Red | 1 = on, 0 = off |
//! | 1 | Green | 1 = on, 0 = off |
//! | 2 | Blue | 1 = on, 0 = off |
//! | 3 | — | Reserved |
//! | 4 | Dim | 1 = half brightness |
//! | 5 | Blink | 1 = hardware blink |
//! | 6 | Sleep | 1 = breathing effect |
//! | 7 | — | Reserved |

use crate::color::Color;
use crate::device::DeviceInfo;
use crate::drivers::hid_helpers;
use crate::error::{Result, StatusLightError};
use crate::{DeviceDriver, StatusLightDevice, SupportedDevice};

/// MuteMe VID/PID pairs.
const VID_PID: &[(u16, u16)] = &[
    (0x16c0, 0x27db), // MuteMe Original
    (0x20a0, 0x42da), // MuteMe Original (variant)
    (0x20a0, 0x42db), // MuteMe Mini
];

const REPORT_SIZE: usize = 2;

/// Quantize an 8-bit RGB channel to a 1-bit value.
/// Threshold at 128: values >= 128 are "on", below are "off".
pub fn quantize_channel(value: u8) -> bool {
    value >= 128
}

/// Build the command byte from a color (1-bit per channel, no effects).
pub fn build_command_byte(color: Color) -> u8 {
    let mut cmd: u8 = 0;
    if quantize_channel(color.r) {
        cmd |= 0x01;
    }
    if quantize_channel(color.g) {
        cmd |= 0x02;
    }
    if quantize_channel(color.b) {
        cmd |= 0x04;
    }
    cmd
}

/// Build the 2-byte HID report.
pub fn build_color_report(color: Color) -> [u8; REPORT_SIZE] {
    [0x00, build_command_byte(color)]
}

/// HID-backed MuteMe device.
pub struct HidMuteMeDevice {
    device: hidapi::HidDevice,
    serial: Option<String>,
}

impl StatusLightDevice for HidMuteMeDevice {
    fn driver_name(&self) -> &str {
        "MuteMe"
    }

    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }

    fn set_color(&self, color: Color) -> Result<()> {
        let report = build_color_report(color);
        let written = self.device.write(&report)?;
        if written != REPORT_SIZE {
            return Err(StatusLightError::WriteMismatch {
                expected: REPORT_SIZE,
                actual: written,
            });
        }
        log::debug!("MuteMe: set color to {color}");
        Ok(())
    }
}

/// Driver for MuteMe USB lights.
pub struct MuteMeDriver;

impl DeviceDriver for MuteMeDriver {
    fn id(&self) -> &str {
        "muteme"
    }

    fn display_name(&self) -> &str {
        "MuteMe"
    }

    fn supported_hardware(&self) -> Vec<SupportedDevice> {
        vec![
            SupportedDevice {
                name: "MuteMe Original".into(),
                vid: 0x16c0,
                pid: 0x27db,
            },
            SupportedDevice {
                name: "MuteMe Original".into(),
                vid: 0x20a0,
                pid: 0x42da,
            },
            SupportedDevice {
                name: "MuteMe Mini".into(),
                vid: 0x20a0,
                pid: 0x42db,
            },
        ]
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
        hid_helpers::enumerate_hid(VID_PID, "muteme")
    }

    fn open(&self) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_first_hid(VID_PID)?;
        Ok(Box::new(HidMuteMeDevice { device, serial }))
    }

    fn open_serial(&self, serial: &str) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_hid_by_serial(VID_PID, serial)?;
        Ok(Box::new(HidMuteMeDevice { device, serial }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantize_below_threshold() {
        assert!(!quantize_channel(0));
        assert!(!quantize_channel(127));
    }

    #[test]
    fn quantize_at_threshold() {
        assert!(quantize_channel(128));
    }

    #[test]
    fn quantize_above_threshold() {
        assert!(quantize_channel(255));
    }

    #[test]
    fn command_byte_red() {
        let cmd = build_command_byte(Color::new(255, 0, 0));
        assert_eq!(cmd, 0x01, "red only");
    }

    #[test]
    fn command_byte_green() {
        let cmd = build_command_byte(Color::new(0, 255, 0));
        assert_eq!(cmd, 0x02, "green only");
    }

    #[test]
    fn command_byte_blue() {
        let cmd = build_command_byte(Color::new(0, 0, 255));
        assert_eq!(cmd, 0x04, "blue only");
    }

    #[test]
    fn command_byte_white() {
        let cmd = build_command_byte(Color::new(255, 255, 255));
        assert_eq!(cmd, 0x07, "all channels on");
    }

    #[test]
    fn command_byte_off() {
        let cmd = build_command_byte(Color::off());
        assert_eq!(cmd, 0x00, "all channels off");
    }

    #[test]
    fn command_byte_yellow() {
        // Red + Green = yellow
        let cmd = build_command_byte(Color::new(200, 200, 50));
        assert_eq!(cmd, 0x03, "red + green = yellow");
    }

    #[test]
    fn report_format() {
        let report = build_color_report(Color::new(255, 0, 255));
        assert_eq!(report[0], 0x00, "padding byte");
        assert_eq!(report[1], 0x05, "red + blue = magenta");
    }

    #[test]
    fn report_size() {
        let report = build_color_report(Color::off());
        assert_eq!(report.len(), REPORT_SIZE);
    }
}
