//! Kuando Busylight USB light device driver.
//!
//! The Kuando Busylight uses 64-byte HID output reports with a complex
//! multi-step protocol. Colors are encoded as **PWM values (0–100)** rather
//! than the standard 0–255 range.
//!
//! ## Key protocol details
//!
//! - **Keepalive required:** The device turns off after ~7 seconds without
//!   a command. A background thread sends keepalive packets every 5 seconds.
//! - **Color encoding:** PWM 0–100, converted from RGB 0–255 via
//!   `(value * 100) / 255`.
//! - **Packet format:** 64 bytes = 7×8-byte steps + housekeeping + checksum.
//! - **Checksum:** 16-bit sum of bytes 0..62, stored big-endian at bytes 62–63.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::color::Color;
use crate::device::DeviceInfo;
use crate::drivers::hid_helpers;
use crate::error::{Result, StatusLightError};
use crate::{DeviceDriver, StatusLightDevice, SupportedDevice};

/// Kuando VID/PID pairs (multiple hardware revisions).
const VID_PID: &[(u16, u16)] = &[
    (0x27bb, 0x3bca), // Busylight UC Alpha
    (0x27bb, 0x3bcb), // Busylight Alpha (variant)
    (0x27bb, 0x3bcd), // Busylight UC Omega
    (0x27bb, 0x3bce), // Busylight Omega (variant)
    (0x27bb, 0x3bcf), // Busylight (variant)
    (0x04d8, 0xf848), // Busylight Alpha (Microchip VID)
];

const PACKET_SIZE: usize = 64;
const KEEPALIVE_TIMEOUT_SECS: u8 = 15;

/// Convert an RGB channel value (0–255) to Kuando PWM (0–100).
pub fn rgb_to_pwm(value: u8) -> u8 {
    ((value as u16 * 100) / 255) as u8
}

/// Compute the 16-bit checksum of bytes 0..62 and write it big-endian at [62..64].
fn set_checksum(buf: &mut [u8; PACKET_SIZE]) {
    let sum: u16 = buf[..62].iter().map(|&b| b as u16).sum();
    buf[62] = (sum >> 8) as u8;
    buf[63] = (sum & 0xFF) as u8;
}

/// Build a 64-byte color packet.
///
/// Step 0 is set to the given color with "keep alive" flag and repeat=1.
/// Steps 1–6 are unused. Bytes 59–61 are padding (0xFF).
pub fn build_color_packet(color: Color) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];

    let r = rgb_to_pwm(color.r);
    let g = rgb_to_pwm(color.g);
    let b = rgb_to_pwm(color.b);

    // Step 0 (bytes 0–7): set color
    // [0] = repeat count (1)
    // [1] = red PWM
    // [2] = green PWM
    // [3] = blue PWM
    // [4] = on-time (in 0.1s units, 0x0A = 1s)
    // [5] = off-time
    // [6] = audio (0 = none)
    // [7] = next step (0)
    buf[0] = 0x01; // repeat = 1
    buf[1] = r;
    buf[2] = g;
    buf[3] = b;
    buf[4] = 0x0A; // on-time: 1 second
    buf[5] = 0x00; // off-time: 0
    buf[6] = 0x00; // no audio
    buf[7] = 0x00; // next step = 0 (loop)

    // Steps 1–6 (bytes 8–55) are all zeros (unused).

    // Housekeeping (bytes 56–61)
    buf[56] = 0x00; // sensitivity
    buf[57] = KEEPALIVE_TIMEOUT_SECS; // timeout
    buf[58] = 0x00; // trigger

    // Padding
    buf[59] = 0xFF;
    buf[60] = 0xFF;
    buf[61] = 0xFF;

    set_checksum(&mut buf);
    buf
}

/// Build a 64-byte keepalive packet (no color change, just reset timeout).
pub fn build_keepalive_packet(timeout_secs: u8) -> [u8; PACKET_SIZE] {
    let mut buf = [0u8; PACKET_SIZE];

    // All steps empty (no color/audio changes).
    // Just set the timeout in housekeeping.
    buf[57] = timeout_secs;

    buf[59] = 0xFF;
    buf[60] = 0xFF;
    buf[61] = 0xFF;

    set_checksum(&mut buf);
    buf
}

/// HID-backed Kuando Busylight device with keepalive thread.
pub struct HidKuandoDevice {
    device: Arc<std::sync::Mutex<hidapi::HidDevice>>,
    serial: Option<String>,
    alive: Arc<AtomicBool>,
    keepalive_handle: Option<JoinHandle<()>>,
}

impl HidKuandoDevice {
    fn new(device: hidapi::HidDevice, serial: Option<String>) -> Self {
        let device = Arc::new(std::sync::Mutex::new(device));
        let alive = Arc::new(AtomicBool::new(true));

        // Spawn keepalive thread.
        let dev_clone = Arc::clone(&device);
        let alive_clone = Arc::clone(&alive);
        let handle = thread::spawn(move || {
            let keepalive = build_keepalive_packet(KEEPALIVE_TIMEOUT_SECS);
            while alive_clone.load(Ordering::SeqCst) {
                // Sleep in 500ms intervals to allow responsive shutdown.
                for _ in 0..10 {
                    if !alive_clone.load(Ordering::SeqCst) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(500));
                }

                if let Ok(dev) = dev_clone.lock() {
                    if let Err(e) = dev.write(&keepalive) {
                        log::warn!("Kuando keepalive failed: {e}");
                    }
                }
            }
        });

        Self {
            device,
            serial,
            alive,
            keepalive_handle: Some(handle),
        }
    }
}

impl Drop for HidKuandoDevice {
    fn drop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Some(handle) = self.keepalive_handle.take() {
            let _ = handle.join();
        }
        // Send off command.
        if let Ok(dev) = self.device.lock() {
            let off_packet = build_color_packet(Color::off());
            let _ = dev.write(&off_packet);
        }
    }
}

impl StatusLightDevice for HidKuandoDevice {
    fn driver_name(&self) -> &str {
        "Kuando"
    }

    fn serial(&self) -> Option<&str> {
        self.serial.as_deref()
    }

    fn set_color(&self, color: Color) -> Result<()> {
        let report = build_color_packet(color);
        let dev = self.device.lock().map_err(|_| {
            StatusLightError::Hid(hidapi::HidError::IoError {
                error: std::io::Error::other("mutex poisoned"),
            })
        })?;
        let written = dev.write(&report)?;
        if written != PACKET_SIZE {
            return Err(StatusLightError::WriteMismatch {
                expected: PACKET_SIZE,
                actual: written,
            });
        }
        log::debug!("Kuando: set color to {color}");
        Ok(())
    }
}

/// Driver for Kuando Busylight USB lights.
pub struct KuandoDriver;

impl DeviceDriver for KuandoDriver {
    fn id(&self) -> &str {
        "kuando"
    }

    fn display_name(&self) -> &str {
        "Kuando Busylight"
    }

    fn supported_hardware(&self) -> Vec<SupportedDevice> {
        VID_PID
            .iter()
            .map(|&(vid, pid)| {
                let name = match (vid, pid) {
                    (0x27bb, 0x3bca) => "Busylight UC Alpha",
                    (0x27bb, 0x3bcb) => "Busylight Alpha",
                    (0x27bb, 0x3bcd) => "Busylight UC Omega",
                    (0x27bb, 0x3bce) => "Busylight Omega",
                    (0x27bb, 0x3bcf) => "Busylight",
                    (0x04d8, 0xf848) => "Busylight Alpha",
                    _ => "Busylight",
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
        hid_helpers::enumerate_hid(VID_PID, "kuando")
    }

    fn open(&self) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_first_hid(VID_PID)?;
        Ok(Box::new(HidKuandoDevice::new(device, serial)))
    }

    fn open_serial(&self, serial: &str) -> Result<Box<dyn StatusLightDevice>> {
        let (device, serial) = hid_helpers::open_hid_by_serial(VID_PID, serial)?;
        Ok(Box::new(HidKuandoDevice::new(device, serial)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwm_conversion_zero() {
        assert_eq!(rgb_to_pwm(0), 0);
    }

    #[test]
    fn pwm_conversion_max() {
        assert_eq!(rgb_to_pwm(255), 100);
    }

    #[test]
    fn pwm_conversion_midpoint() {
        // 128 * 100 / 255 = 50.19... → 50
        assert_eq!(rgb_to_pwm(128), 50);
    }

    #[test]
    fn color_packet_size() {
        let packet = build_color_packet(Color::new(255, 0, 0));
        assert_eq!(packet.len(), PACKET_SIZE);
    }

    #[test]
    fn color_packet_pwm_values() {
        let packet = build_color_packet(Color::new(255, 128, 0));
        assert_eq!(packet[1], 100, "red PWM should be 100");
        assert_eq!(packet[2], 50, "green PWM should be 50");
        assert_eq!(packet[3], 0, "blue PWM should be 0");
    }

    #[test]
    fn color_packet_padding() {
        let packet = build_color_packet(Color::off());
        assert_eq!(packet[59], 0xFF);
        assert_eq!(packet[60], 0xFF);
        assert_eq!(packet[61], 0xFF);
    }

    #[test]
    fn color_packet_checksum() {
        let packet = build_color_packet(Color::new(255, 0, 0));
        let expected_sum: u16 = packet[..62].iter().map(|&b| b as u16).sum();
        let stored_sum = ((packet[62] as u16) << 8) | (packet[63] as u16);
        assert_eq!(stored_sum, expected_sum);
    }

    #[test]
    fn keepalive_packet_timeout() {
        let packet = build_keepalive_packet(15);
        assert_eq!(packet[57], 15);
    }

    #[test]
    fn keepalive_packet_checksum() {
        let packet = build_keepalive_packet(15);
        let expected_sum: u16 = packet[..62].iter().map(|&b| b as u16).sum();
        let stored_sum = ((packet[62] as u16) << 8) | (packet[63] as u16);
        assert_eq!(stored_sum, expected_sum);
    }

    #[test]
    fn keepalive_packet_padding() {
        let packet = build_keepalive_packet(15);
        assert_eq!(packet[59], 0xFF);
        assert_eq!(packet[60], 0xFF);
        assert_eq!(packet[61], 0xFF);
    }
}
