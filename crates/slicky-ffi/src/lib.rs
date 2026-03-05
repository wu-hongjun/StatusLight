//! C FFI bindings for controlling Slicky USB status lights from Swift/C.
//!
//! All functions return `i32` status codes:
//! -  `0` — success
//! - `-1` — device not found
//! - `-2` — multiple devices found
//! - `-3` — HID communication error (or panic caught)
//! - `-4` — invalid color value
//! - `-5` — invalid argument (null pointer, bad UTF-8)
//! - `-6` — write failed

use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Once;

use slicky_core::{Color, HidSlickyDevice, Preset, SlickyDevice, SlickyError};

static INIT: Once = Once::new();

/// Map a [`SlickyError`] to an FFI error code.
fn error_code(e: &SlickyError) -> i32 {
    match e {
        SlickyError::DeviceNotFound => -1,
        SlickyError::MultipleDevices { .. } => -2,
        SlickyError::Hid(_) => -3,
        SlickyError::InvalidHexColor(_) => -4,
        SlickyError::UnknownPreset(_) => -4,
        SlickyError::WriteMismatch { .. } => -6,
        SlickyError::DuplicatePreset(_) | SlickyError::PresetNotFound(_) => -4,
    }
}

/// Open the device and set it to the given color. Returns 0 on success.
fn set_color_inner(color: Color) -> i32 {
    match HidSlickyDevice::open() {
        Ok(dev) => match dev.set_color(color) {
            Ok(()) => 0,
            Err(e) => error_code(&e),
        },
        Err(e) => error_code(&e),
    }
}

/// Initialize logging. Safe to call multiple times.
#[no_mangle]
pub extern "C" fn slicky_init() {
    INIT.call_once(|| {
        env_logger::init();
    });
}

/// Set the light to the given RGB color.
#[no_mangle]
pub extern "C" fn slicky_set_rgb(r: u8, g: u8, b: u8) -> i32 {
    std::panic::catch_unwind(|| set_color_inner(Color::new(r, g, b))).unwrap_or(-3)
}

/// Set the light to a hex color string (e.g., "#FF0000" or "FF0000").
///
/// # Safety
///
/// `hex` must be a valid, non-null, null-terminated UTF-8 C string.
#[no_mangle]
pub unsafe extern "C" fn slicky_set_hex(hex: *const c_char) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if hex.is_null() {
            return -5;
        }
        // SAFETY: caller guarantees `hex` is a valid null-terminated C string.
        let c_str = unsafe { CStr::from_ptr(hex) };
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
pub unsafe extern "C" fn slicky_set_preset(name: *const c_char) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if name.is_null() {
            return -5;
        }
        // SAFETY: caller guarantees `name` is a valid null-terminated C string.
        let c_str = unsafe { CStr::from_ptr(name) };
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
pub extern "C" fn slicky_off() -> i32 {
    std::panic::catch_unwind(|| set_color_inner(Color::off())).unwrap_or(-3)
}

/// Check if a Slicky device is connected.
///
/// Returns `1` if connected, `0` if not. Never returns error codes.
#[no_mangle]
pub extern "C" fn slicky_is_connected() -> i32 {
    std::panic::catch_unwind(|| -> i32 {
        match HidSlickyDevice::enumerate() {
            Ok(devices) => i32::from(!devices.is_empty()),
            Err(_) => 0,
        }
    })
    .unwrap_or_default()
}
