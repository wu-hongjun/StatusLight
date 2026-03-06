//! HID protocol encoding for Slicky devices.
//!
//! The Slicky-1.0 uses 64-byte vendor-specific HID output reports.
//! The buffer sent via the HID API is 65 bytes: a report ID byte (0x00)
//! followed by the 64-byte payload.
//!
//! ## Wire format (65 bytes)
//!
//! ```text
//! Index: [0]   [1]   [2]   [3]   [4]   [5]   [6]   [7]   [8]   [9..64]
//! Value: 0x00  0x0A  0x04  0x00  0x00  0x00  BLUE  GRN   RED   0x00...
//!        ^^^^  ^^^^  ^^^^                    ^^^^  ^^^^  ^^^^
//!        rpt   cmd   sub                     B     G     R
//!        ID
//! ```

use crate::color::Color;

/// USB Vendor ID for Lexcelon Slicky devices.
pub const VENDOR_ID: u16 = 0x04D8;

/// USB Product ID for Slicky-1.0.
pub const PRODUCT_ID: u16 = 0xEC24;

/// HID report payload size (bytes).
pub const REPORT_SIZE: usize = 64;

/// Total buffer size: report ID (1 byte) + payload (64 bytes).
pub const BUFFER_SIZE: usize = 65;

// Byte offsets within the 65-byte buffer.
const IDX_REPORT_ID: usize = 0; // always 0x00
const IDX_COMMAND: usize = 1; // 0x0A = set color
const IDX_SUBCOMMAND: usize = 2; // 0x04
const IDX_BLUE: usize = 6;
const IDX_GREEN: usize = 7;
const IDX_RED: usize = 8;

// Command bytes.
const CMD_SET_COLOR: u8 = 0x0A;
const SUBCMD_SET_COLOR: u8 = 0x04;

/// Build the 65-byte HID output report for setting a color.
///
/// The returned buffer includes the report ID at index 0, the set-color
/// command bytes, and the BGR color values at their protocol-defined offsets.
pub fn build_set_color_report(color: Color) -> [u8; BUFFER_SIZE] {
    let mut buf = [0u8; BUFFER_SIZE];
    buf[IDX_REPORT_ID] = 0x00;
    buf[IDX_COMMAND] = CMD_SET_COLOR;
    buf[IDX_SUBCOMMAND] = SUBCMD_SET_COLOR;
    buf[IDX_BLUE] = color.b;
    buf[IDX_GREEN] = color.g;
    buf[IDX_RED] = color.r;
    buf
}

/// Build the off report (all color bytes zero).
pub fn build_off_report() -> [u8; BUFFER_SIZE] {
    build_set_color_report(Color::off())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_color_report_red() {
        let report = build_set_color_report(Color::new(255, 0, 0));
        assert_eq!(report[0], 0x00, "report ID should be 0x00");
        assert_eq!(report[1], 0x0A, "command byte should be 0x0A");
        assert_eq!(report[2], 0x04, "subcommand byte should be 0x04");
        assert_eq!(report[IDX_RED], 255, "red channel");
        assert_eq!(report[IDX_GREEN], 0, "green channel");
        assert_eq!(report[IDX_BLUE], 0, "blue channel");
    }

    #[test]
    fn set_color_report_bgr_order() {
        let report = build_set_color_report(Color::new(0x11, 0x22, 0x33));
        assert_eq!(report[6], 0x33, "index 6 should be blue");
        assert_eq!(report[7], 0x22, "index 7 should be green");
        assert_eq!(report[8], 0x11, "index 8 should be red");
    }

    #[test]
    fn set_color_report_size() {
        let report = build_set_color_report(Color::new(0, 0, 0));
        assert_eq!(report.len(), BUFFER_SIZE, "report should be 65 bytes");
    }

    #[test]
    fn set_color_report_padding_zeros() {
        let report = build_set_color_report(Color::new(255, 255, 255));
        for i in 9..BUFFER_SIZE {
            assert_eq!(report[i], 0x00, "padding byte at index {i} should be 0x00");
        }
    }

    #[test]
    fn off_report_has_zero_color_bytes() {
        let report = build_off_report();
        assert_eq!(report[IDX_RED], 0, "off report red should be 0");
        assert_eq!(report[IDX_GREEN], 0, "off report green should be 0");
        assert_eq!(report[IDX_BLUE], 0, "off report blue should be 0");
    }

    #[test]
    fn off_report_has_command_bytes() {
        let report = build_off_report();
        assert_eq!(report[1], 0x0A, "off report should still have command byte");
        assert_eq!(
            report[2], 0x04,
            "off report should still have subcommand byte"
        );
    }
}
