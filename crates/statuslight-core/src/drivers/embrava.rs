//! Embrava Blynclight USB light device driver.
//!
//! The Embrava Blynclight uses HID output reports with **RBG** color order
//! (Red-Blue-Green — blue and green are swapped compared to RGB).
//!
//! ## Protocol (9 bytes)
//!
//! | Byte | Value | Description |
//! |------|-------|-------------|
//! | 0 | 0x00 | Report ID |
//! | 1 | R | Red channel (0–255) |
//! | 2 | B | Blue channel (0–255) |
//! | 3 | G | Green channel (0–255) |
//! | 4 | flags | Bit 7: off (1=off, 0=on) |
//! | 5 | 0x00 | Music (not used) |
//! | 6 | 0x00 | Volume (not used) |
//! | 7 | 0xFF | Footer byte 1 |
//! | 8 | 0x22 | Footer byte 2 |

use crate::color::Color;
use crate::device::DeviceInfo;
use crate::drivers::hid_helpers;
use crate::error::{Result, StatusLightError};
use crate::{DeviceDriver, StatusLightDevice, SupportedDevice};

/// Embrava VID/PID pairs (multiple hardware revisions).
const VID_PID: &[(u16, u16)] = &[
    (0x2c0d, 0x0001), // Blynclight
    (0x2c0d, 0x0002), // Blynclight Plus
    (0x2c0d, 0x000a), // Blynclight Mini
    (0x2c0d, 0x000c), // Blynclight (variant)
    (0x2c0d, 0x0010), // Blynclight Plus (variant)
    (0x0e53, 0x2516), // Embrava Connect
    (0x0e53, 0x2517), // Embrava Connect Mini
    (0x047f, 0xd005), // Plantronics Status Indicator (Embrava OEM)
];

const REPORT_SIZE: usize = 9;
const FOOTER_1: u8 = 0xFF;
const FOOTER_2: u8 = 0x22;

/// Build the 9-byte color report (RBG order).
///
/// When `color` is off (all zeros), the "off" flag (bit 7 of byte 4) is set.
pub fn build_color_report(color: Color) -> [u8; REPORT_SIZE] {
    let flags = if color.is_off() { 0x80 } else { 0x00 };
    [
        0x00,     // report ID
        color.r,  // Red
        color.b,  // Blue (swapped)
        color.g,  // Green (swapped)
        flags,    // flags: bit 7 = off
        0x00,     // music
        0x00,     // volume
        FOOTER_1, // always 0xFF
        FOOTER_2, // always 0x22
    ]
}

/// HID-backed Embrava Blynclight device.
pub struct HidEmbravaDevice {
    device: hidapi::HidDevice,
    serial: Option<String>,
}

impl StatusLightDevice for HidEmbravaDevice {
    fn driver_name(&self) -> &str {
        "Embrava"
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
        log::debug!("Embrava: set color to {color}");
        Ok(())
    }
}

/// Driver for Embrava Blynclight USB lights.
pub struct EmbravaDriver;

impl DeviceDriver for EmbravaDriver {
    fn id(&self) -> &str {
        "embrava"
    }

    fn display_name(&self) -> &str {
        "Embrava Blynclight"
    }

    fn supported_hardware(&self) -> Vec<SupportedDevice> {
        VID_PID
            .iter()
            .map(|&(vid, pid)| {
                let name = match (vid, pid) {
                    (0x2c0d, 0x0001) => "Blynclight",
                    (0x2c0d, 0x0002) => "Blynclight Plus",
                    (0x2c0d, 0x000a) => "Blynclight Mini",
                    (0x2c0d, 0x000c) => "Blynclight",
                    (0x2c0d, 0x0010) => "Blynclight Plus",
                    (0x0e53, 0x2516) => "Embrava Connect",
                    (0x0e53, 0x2517) => "Embrava Connect Mini",
                    (0x047f, 0xd005) => "Plantronics Status Indicator",
                    _ => "Blynclight",
                };
                SupportedDevice {
                    name: name.into(),
                    vid,
                    pid,
                }
            })
            .collect()
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
        hid_helpers::enumerate_hid(VID_PID, "embrava")
    }

    fn open(&self) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_first_hid(VID_PID)?;
        Ok(Box::new(HidEmbravaDevice { device, serial }))
    }

    fn open_serial(&self, serial: &str) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_hid_by_serial(VID_PID, serial)?;
        Ok(Box::new(HidEmbravaDevice { device, serial }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_report_rbg_order() {
        let report = build_color_report(Color::new(255, 128, 64));
        assert_eq!(report[0], 0x00, "report ID");
        assert_eq!(report[1], 255, "byte 1 should be Red");
        assert_eq!(report[2], 64, "byte 2 should be Blue");
        assert_eq!(report[3], 128, "byte 3 should be Green");
    }

    #[test]
    fn color_report_on_flag() {
        let report = build_color_report(Color::new(255, 0, 0));
        assert_eq!(
            report[4] & 0x80,
            0,
            "off bit should be clear when color is set"
        );
    }

    #[test]
    fn off_report_flag() {
        let report = build_color_report(Color::off());
        assert_ne!(
            report[4] & 0x80,
            0,
            "off bit should be set when color is off"
        );
    }

    #[test]
    fn footer_bytes() {
        let report = build_color_report(Color::new(0, 255, 0));
        assert_eq!(report[7], 0xFF, "footer byte 1");
        assert_eq!(report[8], 0x22, "footer byte 2");
    }

    #[test]
    fn report_size() {
        let report = build_color_report(Color::off());
        assert_eq!(report.len(), REPORT_SIZE);
    }
}
