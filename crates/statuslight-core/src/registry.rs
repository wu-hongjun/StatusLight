//! Device registry for multi-driver device discovery.
//!
//! The [`DeviceRegistry`] holds all registered [`DeviceDriver`] instances
//! and provides methods to enumerate and open devices across all drivers.

use std::time::Duration;

use crate::{
    DeviceDriver, DeviceInfo, Result, StatusLightDevice, StatusLightError, SupportedDevice,
};

/// Maximum time to wait for [`hidapi::HidApi::new()`] before giving up.
///
/// On macOS the IOKit backend can deadlock indefinitely; the timeout prevents
/// the process from hanging forever.
const HIDAPI_TIMEOUT: Duration = Duration::from_secs(5);

/// Attempt to construct a [`hidapi::HidApi`] instance within `timeout`.
///
/// `hidapi::HidApi::new()` can block indefinitely on macOS due to an IOKit
/// backend deadlock. This helper spawns a dedicated OS thread and waits at
/// most `timeout` for the call to return, returning `None` on timeout or error.
fn new_hidapi_with_timeout(timeout: Duration) -> Option<hidapi::HidApi> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = hidapi::HidApi::new();
        // Ignore send errors — the receiver may have timed out and been dropped.
        let _ = tx.send(result);
    });
    match rx.recv_timeout(timeout) {
        Ok(Ok(api)) => Some(api),
        Ok(Err(e)) => {
            log::warn!("HidApi::new() failed: {e}");
            None
        }
        Err(_) => {
            log::warn!(
                "HidApi::new() timed out after {}s — skipping HID enumeration",
                timeout.as_secs()
            );
            None
        }
    }
}

/// Registry of available device drivers.
pub struct DeviceRegistry {
    drivers: Vec<Box<dyn DeviceDriver>>,
}

impl DeviceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            drivers: Vec::new(),
        }
    }

    /// Create a registry with all built-in drivers.
    pub fn with_builtins() -> Self {
        let mut reg = Self::new();
        reg.register(Box::new(crate::drivers::SlickyDriver));
        reg.register(Box::new(crate::drivers::LuxaforDriver));
        reg.register(Box::new(crate::drivers::Blink1Driver));
        reg.register(Box::new(crate::drivers::BlinkStickDriver));
        reg.register(Box::new(crate::drivers::EmbravaDriver));
        reg.register(Box::new(crate::drivers::KuandoDriver));
        reg.register(Box::new(crate::drivers::EposDriver));
        reg.register(Box::new(crate::drivers::MuteMeDriver));
        reg
    }

    /// Register a device driver.
    pub fn register(&mut self, driver: Box<dyn DeviceDriver>) {
        self.drivers.push(driver);
    }

    /// List all supported hardware across all registered drivers.
    ///
    /// Returns `(driver_display_name, supported_devices)` tuples.
    pub fn supported_all(&self) -> Vec<(String, Vec<SupportedDevice>)> {
        self.drivers
            .iter()
            .map(|d| (d.display_name().to_string(), d.supported_hardware()))
            .collect()
    }

    /// Enumerate devices across all registered drivers.
    ///
    /// Returns a list of `(driver_id, device_info)` tuples.
    /// Drivers that fail to enumerate are logged and skipped.
    pub fn enumerate_all(&self) -> Vec<(String, DeviceInfo)> {
        let api = match new_hidapi_with_timeout(HIDAPI_TIMEOUT) {
            Some(api) => api,
            None => return Vec::new(),
        };
        let mut all = Vec::new();
        for driver in &self.drivers {
            match driver.enumerate(&api) {
                Ok(devices) => {
                    for info in devices {
                        all.push((driver.id().to_string(), info));
                    }
                }
                Err(e) => {
                    log::warn!("Driver '{}' enumeration failed: {e}", driver.id());
                }
            }
        }
        all
    }

    /// Open the first available device from any driver.
    ///
    /// Tries each driver in registration order. If a driver returns
    /// [`DeviceNotFound`](StatusLightError::DeviceNotFound), the next driver
    /// is tried. Any other error (e.g. `MultipleDevices`, `Hid`) is returned
    /// immediately.
    pub fn open_any(&self) -> Result<Box<dyn StatusLightDevice>> {
        let api =
            new_hidapi_with_timeout(HIDAPI_TIMEOUT).ok_or(StatusLightError::DeviceNotFound)?;
        for driver in &self.drivers {
            match driver.open(&api) {
                Ok(device) => return Ok(device),
                Err(StatusLightError::DeviceNotFound) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(StatusLightError::DeviceNotFound)
    }

    /// Open a device by driver ID and optional serial number.
    pub fn open(
        &self,
        driver_id: &str,
        serial: Option<&str>,
    ) -> Result<Box<dyn StatusLightDevice>> {
        let api =
            new_hidapi_with_timeout(HIDAPI_TIMEOUT).ok_or(StatusLightError::DeviceNotFound)?;
        let driver = self
            .drivers
            .iter()
            .find(|d| d.id() == driver_id)
            .ok_or_else(|| StatusLightError::UnknownDriver(driver_id.to_string()))?;
        match serial {
            Some(s) => driver.open_serial(&api, s),
            None => driver.open(&api),
        }
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}
