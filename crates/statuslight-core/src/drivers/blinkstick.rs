//! BlinkStick USB light device driver.
//!
//! The BlinkStick uses HID **feature reports** with **GRB** color order.
//!
//! ## Protocol (single LED, report ID 0x01)
//!
//! | Byte | Value | Description |
//! |------|-------|-------------|
//! | 0 | 0x01 | Report ID: set single LED |
//! | 1 | G | Green channel (0–255) |
//! | 2 | R | Red channel (0–255) |
//! | 3 | B | Blue channel (0–255) |
//!
//! Note: Color order is GRB (Green-Red-Blue), not RGB.

use crate::color::Color;
use crate::device::DeviceInfo;
use crate::drivers::hid_helpers;
use crate::error::Result;
use crate::{DeviceDriver, StatusLightDevice, SupportedDevice};

/// BlinkStick VID/PID.
const VID_PID: &[(u16, u16)] = &[(0x20a0, 0x41e5)];

const REPORT_SINGLE: u8 = 0x01;
const REPORT_SIZE: usize = 4;

/// Build the 4-byte single-LED report (GRB order).
pub fn build_single_color_report(color: Color) -> [u8; REPORT_SIZE] {
    [REPORT_SINGLE, color.g, color.r, color.b]
}

/// HID-backed BlinkStick device.
pub struct HidBlinkStickDevice {
    device: hidapi::HidDevice,
    serial: Option<String>,
}

impl StatusLightDevice for HidBlinkStickDevice {
    fn driver_name(&self) -> &str {
        "BlinkStick"
    }

    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }

    fn set_color(&self, color: Color) -> Result<()> {
        let report = build_single_color_report(color);
        self.device.send_feature_report(&report)?;
        log::debug!("BlinkStick: set color to {color}");
        Ok(())
    }
}

/// Driver for BlinkStick USB lights.
pub struct BlinkStickDriver;

impl DeviceDriver for BlinkStickDriver {
    fn id(&self) -> &str {
        "blinkstick"
    }

    fn display_name(&self) -> &str {
        "BlinkStick"
    }

    fn supported_hardware(&self) -> Vec<SupportedDevice> {
        vec![SupportedDevice {
            name: "BlinkStick / Pro / Square / Strip / Nano / Flex".into(),
            vid: 0x20a0,
            pid: 0x41e5,
        }]
    }

    fn enumerate(&self) -> Result<Vec<DeviceInfo>> {
        hid_helpers::enumerate_hid(VID_PID, "blinkstick")
    }

    fn open(&self) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_first_hid(VID_PID)?;
        Ok(Box::new(HidBlinkStickDevice { device, serial }))
    }

    fn open_serial(&self, serial: &str) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_hid_by_serial(VID_PID, serial)?;
        Ok(Box::new(HidBlinkStickDevice { device, serial }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_color_report_grb_order() {
        let report = build_single_color_report(Color::new(255, 128, 64));
        assert_eq!(report[0], REPORT_SINGLE);
        assert_eq!(report[1], 128, "byte 1 should be Green");
        assert_eq!(report[2], 255, "byte 2 should be Red");
        assert_eq!(report[3], 64, "byte 3 should be Blue");
    }

    #[test]
    fn report_size() {
        let report = build_single_color_report(Color::off());
        assert_eq!(report.len(), REPORT_SIZE);
    }

    #[test]
    fn off_report() {
        let report = build_single_color_report(Color::off());
        assert_eq!(report, [REPORT_SINGLE, 0, 0, 0]);
    }
}
