//! Transparent proxy that routes device commands through the daemon's Unix
//! socket API when the daemon is running, falling back to direct HID access.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use slicky_core::{Color, HidSlickyDevice, SlickyDevice};

/// Socket path used by the daemon.
const DAEMON_SOCKET: &str = "/tmp/slicky.sock";

/// Maximum response size we'll read from the daemon (64 KiB).
const MAX_RESPONSE_SIZE: u64 = 64 * 1024;

/// Timeout for socket read/write operations.
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

/// A device proxy that transparently routes to the daemon or direct HID.
pub enum DeviceProxy {
    /// Route commands through the daemon's Unix socket.
    Daemon,
    /// Direct HID access (no daemon running).
    Direct(HidSlickyDevice),
}

impl DeviceProxy {
    /// Try to connect to the daemon socket first; fall back to direct HID.
    pub fn open() -> Result<Self> {
        if Path::new(DAEMON_SOCKET).exists() {
            return Ok(Self::Daemon);
        }
        let device =
            HidSlickyDevice::open().context("failed to open Slicky device (no daemon running)")?;
        Ok(Self::Direct(device))
    }

    /// Set the device to the given color.
    pub fn set_color(&self, color: Color) -> Result<()> {
        match self {
            Self::Daemon => {
                let body = format!(r#"{{"r":{},"g":{},"b":{}}}"#, color.r, color.g, color.b);
                let resp = http_post(DAEMON_SOCKET, "/rgb", &body)?;
                if !resp.status_ok {
                    bail!("daemon error: {}", resp.body);
                }
                Ok(())
            }
            Self::Direct(dev) => dev.set_color(color).context("failed to set color"),
        }
    }

    /// Turn the device off.
    pub fn off(&self) -> Result<()> {
        match self {
            Self::Daemon => {
                let resp = http_post(DAEMON_SOCKET, "/off", "")?;
                if !resp.status_ok {
                    bail!("daemon error: {}", resp.body);
                }
                Ok(())
            }
            Self::Direct(dev) => dev.off().context("failed to turn off"),
        }
    }

    /// Send a POST request to the daemon (only works in Daemon mode).
    /// Returns the response body on success.
    pub fn post(&self, path: &str, body: &str) -> Result<String> {
        match self {
            Self::Daemon => {
                let resp = http_post(DAEMON_SOCKET, path, body)?;
                if !resp.status_ok {
                    bail!("daemon error: {}", resp.body);
                }
                Ok(resp.body)
            }
            Self::Direct(_) => bail!("daemon not running"),
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
