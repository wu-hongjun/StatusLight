//! Device registry for multi-driver device discovery.
//!
//! The [`DeviceRegistry`] holds all registered [`DeviceDriver`] instances
//! and provides methods to enumerate and open devices across all drivers.

use crate::{DeviceDriver, DeviceInfo, Result, StatusLightDevice, StatusLightError};

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
        reg
    }

    /// Register a device driver.
    pub fn register(&mut self, driver: Box<dyn DeviceDriver>) {
        self.drivers.push(driver);
    }

    /// Enumerate devices across all registered drivers.
    ///
    /// Returns a list of `(driver_id, device_info)` tuples.
    /// Drivers that fail to enumerate are logged and skipped.
    pub fn enumerate_all(&self) -> Vec<(String, DeviceInfo)> {
        let mut all = Vec::new();
        for driver in &self.drivers {
            match driver.enumerate() {
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
        let last_error = StatusLightError::DeviceNotFound;
        for driver in &self.drivers {
            match driver.open() {
                Ok(device) => return Ok(device),
                Err(StatusLightError::DeviceNotFound) => continue,
                Err(e) => return Err(e),
            }
        }
        Err(last_error)
    }

    /// Open a device by driver ID and optional serial number.
    pub fn open(
        &self,
        driver_id: &str,
        serial: Option<&str>,
    ) -> Result<Box<dyn StatusLightDevice>> {
        let driver = self
            .drivers
            .iter()
            .find(|d| d.id() == driver_id)
            .ok_or(StatusLightError::DeviceNotFound)?;
        match serial {
            Some(s) => driver.open_serial(s),
            None => driver.open(),
        }
    }
}

impl Default for DeviceRegistry {
    fn default() -> Self {
        Self::new()
    }
}
