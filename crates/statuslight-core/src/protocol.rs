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

const CMD_DEVICE_INFO: u8 = 0x00;
const CMD_SERIAL: u8 = 0x01;
const CMD_GET_COLOR: u8 = 0x0B;

/// Timeout in milliseconds for reading HID input reports.
pub const READ_TIMEOUT_MS: i32 = 200;

// Response byte offsets for CMD 0x0B get color.
const RESP_COLOR_BLUE: usize = 5;
const RESP_COLOR_GREEN: usize = 6;
const RESP_COLOR_RED: usize = 7;

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

/// Build the 65-byte HID output report to request the current color (CMD 0x0B).
pub fn build_get_color_request() -> [u8; BUFFER_SIZE] {
    let mut buf = [0u8; BUFFER_SIZE];
    buf[IDX_REPORT_ID] = 0x00;
    buf[IDX_COMMAND] = CMD_GET_COLOR;
    buf
}

/// Parse the device response from a CMD 0x0B get-color request.
///
/// Returns `Some(Color)` if the response is valid (starts with `0x0B`
/// and has at least 8 bytes), or `None` otherwise.
pub fn parse_get_color_response(resp: &[u8]) -> Option<Color> {
    if resp.len() < 8 || resp[0] != CMD_GET_COLOR {
        return None;
    }
    Some(Color::new(
        resp[RESP_COLOR_RED],
        resp[RESP_COLOR_GREEN],
        resp[RESP_COLOR_BLUE],
    ))
}

/// Build the 65-byte HID output report for a device info query (CMD 0x00).
pub fn build_device_info_request() -> [u8; BUFFER_SIZE] {
    let mut buf = [0u8; BUFFER_SIZE];
    buf[IDX_REPORT_ID] = 0x00;
    buf[IDX_COMMAND] = CMD_DEVICE_INFO;
    buf
}

/// Build the 65-byte HID output report for a serial number query (CMD 0x01).
pub fn build_serial_request() -> [u8; BUFFER_SIZE] {
    let mut buf = [0u8; BUFFER_SIZE];
    buf[IDX_REPORT_ID] = 0x00;
    buf[IDX_COMMAND] = CMD_SERIAL;
    buf
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

    #[test]
    fn get_color_request_format() {
        let req = build_get_color_request();
        assert_eq!(req[0], 0x00, "report ID");
        assert_eq!(req[1], 0x0B, "command byte should be 0x0B");
        assert_eq!(req.len(), BUFFER_SIZE);
    }

    #[test]
    fn parse_get_color_response_valid() {
        let mut resp = [0u8; 64];
        resp[0] = 0x0B;
        resp[RESP_COLOR_BLUE] = 0x33;
        resp[RESP_COLOR_GREEN] = 0x22;
        resp[RESP_COLOR_RED] = 0x11;
        let color = parse_get_color_response(&resp).unwrap();
        assert_eq!(color.r, 0x11);
        assert_eq!(color.g, 0x22);
        assert_eq!(color.b, 0x33);
    }

    #[test]
    fn parse_get_color_response_wrong_command() {
        let mut resp = [0u8; 64];
        resp[0] = 0x0A; // wrong command
        assert!(parse_get_color_response(&resp).is_none());
    }

    #[test]
    fn parse_get_color_response_too_short() {
        let resp = [0x0B, 0x04, 0x00]; // only 3 bytes
        assert!(parse_get_color_response(&resp).is_none());
    }

    #[test]
    fn device_info_request_format() {
        let req = build_device_info_request();
        assert_eq!(req[0], 0x00, "report ID");
        assert_eq!(req[1], 0x00, "command byte should be 0x00");
        assert_eq!(req.len(), BUFFER_SIZE);
    }

    #[test]
    fn serial_request_format() {
        let req = build_serial_request();
        assert_eq!(req[0], 0x00, "report ID");
        assert_eq!(req[1], 0x01, "command byte should be 0x01");
        assert_eq!(req.len(), BUFFER_SIZE);
    }
}
