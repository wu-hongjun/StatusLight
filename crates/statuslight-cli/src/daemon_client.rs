//! Transparent proxy that routes device commands through the daemon's Unix
//! socket API when the daemon is running, falling back to direct HID access.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use statuslight_core::{Color, DeviceRegistry, StatusLightDevice};

/// Socket path used by the daemon.
const DAEMON_SOCKET: &str = "/tmp/statuslight.sock";

/// Maximum response size we'll read from the daemon (64 KiB).
const MAX_RESPONSE_SIZE: u64 = 64 * 1024;

/// Timeout for socket read/write operations.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

/// A device proxy that transparently routes to the daemon or direct HID.
pub enum DeviceProxy {
    /// Route commands through the daemon's Unix socket.
    Daemon {
        /// Optional serial filter for `?device=` query param.
        device_serial: Option<String>,
    },
    /// Direct device access (no daemon running) — single device.
    Direct(Box<dyn StatusLightDevice>),
    /// Direct device access — multiple devices.
    DirectMulti(Vec<Box<dyn StatusLightDevice>>),
}

impl DeviceProxy {
    /// Try to connect to the daemon socket first; fall back to direct HID.
    ///
    /// - `all`: if true, open all devices in direct mode
    /// - `device_serial`: target a specific device by serial
    pub fn open(all: bool, device_serial: Option<&str>) -> Result<Self> {
        if Path::new(DAEMON_SOCKET).exists() {
            return Ok(Self::Daemon {
                device_serial: device_serial.map(String::from),
            });
        }

        let registry = DeviceRegistry::with_builtins();

        if all {
            let all_devices = registry.enumerate_all();
            if all_devices.is_empty() {
                bail!("no devices found");
            }
            let mut devices = Vec::new();
            for (driver_id, info) in &all_devices {
                match registry.open(driver_id, info.serial.as_deref()) {
                    Ok(dev) => devices.push(dev),
                    Err(e) => {
                        log::warn!("Failed to open device (driver={driver_id}): {e}");
                    }
                }
            }
            if devices.is_empty() {
                bail!("failed to open any device");
            }
            Ok(Self::DirectMulti(devices))
        } else if let Some(serial) = device_serial {
            // Find which driver owns this serial.
            let all_devices = registry.enumerate_all();
            let (driver_id, _) = all_devices
                .iter()
                .find(|(_, info)| info.serial.as_deref() == Some(serial))
                .context("no device with that serial")?;
            let device = registry.open(driver_id, Some(serial))?;
            Ok(Self::Direct(device))
        } else {
            let device = registry
                .open_any()
                .context("failed to open device (no daemon running)")?;
            Ok(Self::Direct(device))
        }
    }

    /// Set the device to the given color.
    pub fn set_color(&self, color: Color) -> Result<()> {
        match self {
            Self::Daemon { device_serial } => {
                let body = format!(r#"{{"r":{},"g":{},"b":{}}}"#, color.r, color.g, color.b);
                let path = match device_serial {
                    Some(s) => format!("/rgb?device={s}"),
                    None => "/rgb".to_string(),
                };
                let resp = http_post(DAEMON_SOCKET, &path, &body)?;
                if !resp.status_ok {
                    bail!("daemon error: {}", resp.body);
                }
                Ok(())
            }
            Self::Direct(dev) => dev.set_color(color).context("failed to set color"),
            Self::DirectMulti(devs) => {
                for dev in devs {
                    if let Err(e) = dev.set_color(color) {
                        log::warn!("Device {} failed: {e}", dev.driver_name());
                    }
                }
                Ok(())
            }
        }
    }

    /// Turn the device off.
    pub fn off(&self) -> Result<()> {
        match self {
            Self::Daemon { device_serial } => {
                let path = match device_serial {
                    Some(s) => format!("/off?device={s}"),
                    None => "/off".to_string(),
                };
                let resp = http_post(DAEMON_SOCKET, &path, "")?;
                if !resp.status_ok {
                    bail!("daemon error: {}", resp.body);
                }
                Ok(())
            }
            Self::Direct(dev) => dev.off().context("failed to turn off"),
            Self::DirectMulti(devs) => {
                for dev in devs {
                    if let Err(e) = dev.off() {
                        log::warn!("Device {} failed: {e}", dev.driver_name());
                    }
                }
                Ok(())
            }
        }
    }

    /// Send a POST request to the daemon (only works in Daemon mode).
    /// Returns the response body on success.
    pub fn post(&self, path: &str, body: &str) -> Result<String> {
        match self {
            Self::Daemon { .. } => {
                let resp = http_post(DAEMON_SOCKET, path, body)?;
                if !resp.status_ok {
                    bail!("daemon error: {}", resp.body);
                }
                Ok(resp.body)
            }
            Self::Direct(_) | Self::DirectMulti(_) => bail!("daemon not running"),
        }
    }

    /// Send a GET request to the daemon (only works in Daemon mode).
    /// Returns the response body on success.
    pub fn get(&self, path: &str) -> Result<String> {
        match self {
            Self::Daemon { .. } => {
                let resp = http_get(DAEMON_SOCKET, path)?;
                if !resp.status_ok {
                    bail!("daemon error: {}", resp.body);
                }
                Ok(resp.body)
            }
            Self::Direct(_) | Self::DirectMulti(_) => bail!("daemon not running"),
        }
    }

    /// Returns true if the daemon socket exists (daemon likely running).
    pub fn daemon_running() -> bool {
        Path::new(DAEMON_SOCKET).exists()
    }
}

/// Minimal HTTP response parsed from raw bytes.
struct HttpResponse {
    status_ok: bool,
    body: String,
}

/// Send a minimal HTTP/1.1 POST over a Unix socket and parse the response.
fn http_post(socket_path: &str, path: &str, body: &str) -> Result<HttpResponse> {
    let mut stream = UnixStream::connect(socket_path).context("failed to connect to daemon")?;
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;

    let request = format!(
        "POST {path} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {body}",
        body.len()
    );

    stream
        .write_all(request.as_bytes())
        .context("failed to write request to daemon")?;

    let mut raw = String::new();
    stream
        .take(MAX_RESPONSE_SIZE)
        .read_to_string(&mut raw)
        .context("failed to read response from daemon")?;

    parse_http_response(&raw)
}

/// Send a minimal HTTP/1.1 GET over a Unix socket and parse the response.
fn http_get(socket_path: &str, path: &str) -> Result<HttpResponse> {
    let mut stream = UnixStream::connect(socket_path).context("failed to connect to daemon")?;
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;

    let request = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: localhost\r\n\
         Connection: close\r\n\
         \r\n"
    );

    stream
        .write_all(request.as_bytes())
        .context("failed to write request to daemon")?;

    let mut raw = String::new();
    stream
        .take(MAX_RESPONSE_SIZE)
        .read_to_string(&mut raw)
        .context("failed to read response from daemon")?;

    parse_http_response(&raw)
}

/// Parse a raw HTTP response into status and body.
fn parse_http_response(raw: &str) -> Result<HttpResponse> {
    let (header_section, body) = raw
        .split_once("\r\n\r\n")
        .context("daemon returned malformed HTTP response")?;

    let status_line = header_section
        .lines()
        .next()
        .context("daemon returned empty HTTP response")?;

    // HTTP/1.1 200 OK  →  status code is the second token.
    let status_ok = status_line
        .split_whitespace()
        .nth(1)
        .is_some_and(|code| code.starts_with('2'));

    Ok(HttpResponse {
        status_ok,
        body: body.to_string(),
    })
}
