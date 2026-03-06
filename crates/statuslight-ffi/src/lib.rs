//! C FFI bindings for controlling USB status lights from Swift/C.
//!
//! All functions return `i32` status codes:
//! -  `0` — success
//! - `-1` — device not found
//! - `-2` — multiple devices found
//! - `-3` — HID communication error (or panic caught)
//! - `-4` — invalid color value
//! - `-5` — invalid argument (null pointer, bad UTF-8)
//! - `-6` — write failed
//! - `-7` — unknown or invalid preset

use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Once;

use statuslight_core::{Color, DeviceRegistry, Preset, StatusLightError};

static INIT: Once = Once::new();
static BRIGHTNESS: AtomicU8 = AtomicU8::new(100);

/// Map a [`StatusLightError`] to an FFI error code.
fn error_code(e: &StatusLightError) -> i32 {
    match e {
        StatusLightError::DeviceNotFound => -1,
        StatusLightError::MultipleDevices { .. } => -2,
        StatusLightError::Hid(_) => -3,
        StatusLightError::InvalidHexColor(_) => -4,
        StatusLightError::UnknownPreset(_) => -7,
        StatusLightError::WriteMismatch { .. } => -6,
        StatusLightError::DuplicatePreset(_) | StatusLightError::PresetNotFound(_) => -7,
        StatusLightError::UnknownDriver(_) => -8,
        StatusLightError::ReadTimeout => -9,
        StatusLightError::UnexpectedResponse => -10,
    }
}

/// Open the first available device and set it to the given color. Returns 0 on success.
fn set_color_inner(color: Color) -> i32 {
    // Apply brightness.
    let brightness = BRIGHTNESS.load(Ordering::SeqCst);
    let scaled = color.scale_brightness(brightness as f64 / 100.0);

    match DeviceRegistry::with_builtins().open_any() {
        Ok(dev) => match dev.set_color(scaled) {
            Ok(()) => 0,
            Err(e) => error_code(&e),
        },
        Err(e) => error_code(&e),
    }
}

/// Initialize logging. Safe to call multiple times.
#[no_mangle]
pub extern "C" fn statuslight_init() {
    INIT.call_once(|| {
        let _ = env_logger::try_init();
    });
}

/// Set the light to the given RGB color.
#[no_mangle]
pub extern "C" fn statuslight_set_rgb(r: u8, g: u8, b: u8) -> i32 {
    std::panic::catch_unwind(|| set_color_inner(Color::new(r, g, b))).unwrap_or(-3)
}

/// Set the light to a hex color string (e.g., "#FF0000" or "FF0000").
///
/// # Safety
///
/// `hex` must be a valid, non-null, null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn statuslight_set_hex(hex: *const c_char) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if hex.is_null() {
            return -5;
        }
        // SAFETY: caller guarantees `hex` is a valid null-terminated C string.
        let c_str = CStr::from_ptr(hex);
        let s = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => return -5,
        };
        match Color::from_hex(s) {
            Ok(color) => set_color_inner(color),
            Err(e) => error_code(&e),
        }
    }))
    .unwrap_or(-3)
}

/// Set the light to a named preset (e.g., "red", "busy", "in-meeting").
///
/// # Safety
///
/// `name` must be a valid, non-null, null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn statuslight_set_preset(name: *const c_char) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if name.is_null() {
            return -5;
        }
        // SAFETY: caller guarantees `name` is a valid null-terminated C string.
        let c_str = CStr::from_ptr(name);
        let s = match c_str.to_str() {
            Ok(s) => s,
            Err(_) => return -5,
        };
        match Preset::from_name(s) {
            Ok(preset) => set_color_inner(preset.color()),
            Err(e) => error_code(&e),
        }
    }))
    .unwrap_or(-3)
}

/// Turn the light off.
#[no_mangle]
pub extern "C" fn statuslight_off() -> i32 {
    std::panic::catch_unwind(|| set_color_inner(Color::off())).unwrap_or(-3)
}

/// Check if any status light device is connected.
///
/// Returns `1` if connected, `0` if not. Never returns error codes.
#[no_mangle]
pub extern "C" fn statuslight_is_connected() -> i32 {
    std::panic::catch_unwind(|| -> i32 {
        let registry = DeviceRegistry::with_builtins();
        i32::from(!registry.enumerate_all().is_empty())
    })
    .unwrap_or(-3)
}

/// Set the global brightness level (0–100).
///
/// Returns 0 on success, the clamped brightness value is stored.
#[no_mangle]
pub extern "C" fn statuslight_set_brightness(brightness: u8) -> i32 {
    std::panic::catch_unwind(|| {
        BRIGHTNESS.store(brightness.min(100), Ordering::SeqCst);
        0
    })
    .unwrap_or(-3)
}

/// Get the current brightness level (0–100).
#[no_mangle]
pub extern "C" fn statuslight_get_brightness() -> i32 {
    std::panic::catch_unwind(|| BRIGHTNESS.load(Ordering::SeqCst) as i32).unwrap_or(-3)
}

/// Get the number of connected status light devices.
#[no_mangle]
pub extern "C" fn statuslight_device_count() -> i32 {
    std::panic::catch_unwind(|| -> i32 {
        let registry = DeviceRegistry::with_builtins();
        registry.enumerate_all().len() as i32
    })
    .unwrap_or(-3)
}
