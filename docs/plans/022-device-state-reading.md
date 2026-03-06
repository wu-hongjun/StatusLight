---

# Plan 022 — Device State Reading & Slicky Protocol Documentation

## Plan Summary

This plan adds bidirectional communication to the StatusLight project. Currently all 8 drivers are write-only. The Slicky HID probe experiments discovered that the Slicky device supports a request/response protocol with commands for device info (0x00), serial number (0x01), and color readback (0x0B). This plan introduces an optional `get_color()` method on `StatusLightDevice`, implements it for the Slicky driver, and exposes the capability through the daemon API, CLI, and FFI layers.

---

## Phase 1: Protocol Layer -- New Commands in `protocol.rs`

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-core/src/protocol.rs`

Add new command constants and report builders/parsers alongside the existing `build_set_color_report()`.

### Specific changes:

**New constants:**

```rust
// Command bytes (expanding the existing CMD_SET_COLOR).
const CMD_DEVICE_INFO: u8 = 0x00;
const CMD_SERIAL: u8 = 0x01;
// CMD_SET_COLOR already exists as 0x0A
const CMD_GET_COLOR: u8 = 0x0B;

/// Default timeout for reading HID input reports (milliseconds).
pub const READ_TIMEOUT_MS: i32 = 200;
```

**New response offsets for CMD 0x0B (get color):**

```rust
// Response layout for CMD_GET_COLOR (0x0B):
// Byte 0: echo of command (0x0B)
// Byte 1: 0x04 (subcommand echo)
// Bytes 2-3: reserved
// Byte 4: reserved (0x00)
// Byte 5: Blue
// Byte 6: Green
// Byte 7: Red
const RESP_GET_COLOR_BLUE: usize = 5;
const RESP_GET_COLOR_GREEN: usize = 6;
const RESP_GET_COLOR_RED: usize = 7;
```

**New public functions:**

```rust
/// Build the 65-byte HID output report for querying current color (CMD 0x0B).
pub fn build_get_color_request() -> [u8; BUFFER_SIZE] {
    let mut buf = [0u8; BUFFER_SIZE];
    buf[IDX_REPORT_ID] = 0x00;
    buf[IDX_COMMAND] = CMD_GET_COLOR;
    buf
}

/// Parse a 64-byte input report response into a Color.
/// Returns `None` if the response doesn't look like a valid color response.
pub fn parse_get_color_response(resp: &[u8; REPORT_SIZE]) -> Option<Color> {
    if resp[0] != CMD_GET_COLOR {
        return None;
    }
    Some(Color::new(
        resp[RESP_GET_COLOR_RED],
        resp[RESP_GET_COLOR_GREEN],
        resp[RESP_GET_COLOR_BLUE],
    ))
}

/// Build the 65-byte HID output report for querying device info (CMD 0x00).
pub fn build_device_info_request() -> [u8; BUFFER_SIZE] {
    let mut buf = [0u8; BUFFER_SIZE];
    buf[IDX_REPORT_ID] = 0x00;
    buf[IDX_COMMAND] = CMD_DEVICE_INFO;
    buf
}

/// Build the 65-byte HID output report for querying serial number (CMD 0x01).
pub fn build_serial_request() -> [u8; BUFFER_SIZE] {
    let mut buf = [0u8; BUFFER_SIZE];
    buf[IDX_REPORT_ID] = 0x00;
    buf[IDX_COMMAND] = CMD_SERIAL;
    buf
}
```

**New tests** for all new functions, following the existing test pattern (check sizes, field values, round-trip parsing).

---

## Phase 2: Error Type -- Add `ReadTimeout` Variant

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-core/src/error.rs`

Add a new error variant for read failures:

```rust
/// The device did not respond within the timeout period.
#[error("device read timed out")]
ReadTimeout,

/// The device returned an unexpected response.
#[error("unexpected device response")]
UnexpectedResponse,
```

This also needs a corresponding FFI error code mapping in Phase 5.

---

## Phase 3: Trait Extension -- `get_color()` on `StatusLightDevice`

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-core/src/device.rs`

Add an optional method with a default implementation returning `None`:

```rust
pub trait StatusLightDevice: Send {
    // ... existing methods unchanged ...

    /// Read the current color from the device, if supported.
    ///
    /// Returns `None` if the device/driver does not support color readback.
    /// This is an optional capability — drivers that only support write
    /// operations inherit the default `None` implementation.
    fn get_color(&self) -> Option<Result<Color>> {
        None
    }
}
```

Key design decisions:
- Return type `Option<Result<Color>>` -- `None` means "unsupported", `Some(Err(...))` means "supported but failed", `Some(Ok(color))` means "success".
- Default returns `None` -- all 8 existing drivers continue to work unchanged with zero code modifications.
- No new trait needed -- extends the existing trait, which is the simplest approach.

### Implement for `HidSlickyDevice`:

```rust
impl StatusLightDevice for HidSlickyDevice {
    // ... existing methods unchanged ...

    fn get_color(&self) -> Option<Result<Color>> {
        Some(self.read_color())
    }
}

impl HidSlickyDevice {
    /// Send CMD 0x0B and read back the current color.
    fn read_color(&self) -> Result<Color> {
        let request = protocol::build_get_color_request();
        let written = self.device.write(&request)?;
        if written != BUFFER_SIZE {
            return Err(StatusLightError::WriteMismatch {
                expected: BUFFER_SIZE,
                actual: written,
            });
        }

        let mut buf = [0u8; protocol::REPORT_SIZE];
        let n = self.device.read_timeout(&mut buf, protocol::READ_TIMEOUT_MS)?;
        if n == 0 {
            return Err(StatusLightError::ReadTimeout);
        }

        protocol::parse_get_color_response(&buf)
            .ok_or(StatusLightError::UnexpectedResponse)
    }
}
```

### No changes needed to any other driver files

The 7 other drivers (`blink1.rs`, `blinkstick.rs`, `embrava.rs`, `epos.rs`, `kuando.rs`, `luxafor.rs`, `muteme.rs`) inherit the default `None` implementation and require zero modifications.

---

## Phase 4: Daemon -- Expose `get_color` via API

### 4a. State Update

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-daemon/src/state.rs`

No structural changes needed. The existing `current_color: Mutex<Option<Color>>` tracks software-side state. The new endpoint will read directly from the device.

### 4b. API Route

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-daemon/src/api.rs`

Add a new route `GET /device-color` that reads the actual color from the hardware:

```rust
// In router():
.route("/device-color", get(get_device_color))
```

New handler:

```rust
#[derive(Serialize)]
struct DeviceColorResponse {
    /// Color read from the device, if supported and successful.
    device_color: Option<ColorResponse>,
    /// Whether the device supports color readback.
    supports_readback: bool,
}

async fn get_device_color(
    State(state): State<AppState>,
    Query(query): Query<DeviceQuery>,
) -> Result<Json<DeviceColorResponse>, (StatusCode, Json<ErrorResponse>)> {
    let devices_guard = state.inner.devices.lock().await;

    if devices_guard.is_empty() {
        return Err(map_error(StatusLightError::DeviceNotFound));
    }

    // Find the target device.
    let device = if let Some(serial) = query.device.as_deref() {
        devices_guard
            .iter()
            .find(|d| d.serial() == Some(serial))
            .ok_or_else(|| map_error(StatusLightError::DeviceNotFound))?
    } else {
        &devices_guard[0]
    };

    match device.get_color() {
        None => Ok(Json(DeviceColorResponse {
            device_color: None,
            supports_readback: false,
        })),
        Some(Ok(color)) => Ok(Json(DeviceColorResponse {
            device_color: Some(color.into()),
            supports_readback: true,
        })),
        Some(Err(e)) => Err(map_error(e)),
    }
}
```

Also update `StatusResponse` to include a `supports_readback` field in the `GET /status` endpoint (optional enhancement -- can be deferred).

Update `map_error` to handle the new error variants:

```rust
StatusLightError::ReadTimeout | StatusLightError::UnexpectedResponse => {
    StatusCode::INTERNAL_SERVER_ERROR
}
```

---

## Phase 5: FFI -- Add `statuslight_get_color`

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-ffi/src/lib.rs`

Add new functions:

```rust
/// Read the current color from the device.
///
/// On success, writes the RGB values to the provided pointers and returns 0.
/// Returns -9 if the device does not support color readback.
/// Returns other negative error codes on failure.
///
/// # Safety
///
/// `r`, `g`, `b` must be valid, non-null pointers to `u8`.
#[no_mangle]
pub unsafe extern "C" fn statuslight_get_color(
    r: *mut u8,
    g: *mut u8,
    b: *mut u8,
) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if r.is_null() || g.is_null() || b.is_null() {
            return -5;
        }
        match DeviceRegistry::with_builtins().open_any() {
            Ok(dev) => match dev.get_color() {
                None => -9, // not supported
                Some(Ok(color)) => {
                    *r = color.r;
                    *g = color.g;
                    *b = color.b;
                    0
                }
                Some(Err(e)) => error_code(&e),
            },
            Err(e) => error_code(&e),
        }
    }))
    .unwrap_or(-3)
}
```

Update `error_code` to handle new variants:

```rust
StatusLightError::ReadTimeout => -10,
StatusLightError::UnexpectedResponse => -11,
```

Add new error code `-9` for "not supported" in the doc comment and FFI reference.

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-ffi/include/statuslight.h`

This will be regenerated by cbindgen, but the new declaration will be:

```c
int32_t statuslight_get_color(uint8_t *r, uint8_t *g, uint8_t *b);
```

---

## Phase 6: CLI -- Enhance `statuslight status`

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-cli/src/main.rs`

Enhance the `Commands::Status` handler. Currently it only checks if devices are connected. Add device color readback when in direct mode:

```rust
Commands::Status => {
    let registry = DeviceRegistry::with_builtins();
    let devices = registry.enumerate_all();
    if devices.is_empty() {
        println!("Device:  not connected");
    } else {
        println!("Device:  connected ({} found)", devices.len());

        // Try to read color from first device (direct HID only).
        if let Ok(dev) = registry.open_any() {
            match dev.get_color() {
                Some(Ok(color)) => println!("Color:   {} (from device)", color),
                Some(Err(e)) => println!("Color:   read error: {e}"),
                None => println!("Color:   readback not supported"),
            }
        }
    }

    // ... existing Slack and config status output ...
}
```

Also update the `daemon_client.rs` `DeviceProxy` to support reading color from the daemon:

```rust
impl DeviceProxy {
    /// Read the current color from the device, if supported.
    pub fn get_color(&self) -> Result<Option<Color>> {
        match self {
            Self::Daemon { device_serial } => {
                let path = match device_serial {
                    Some(s) => format!("/device-color?device={s}"),
                    None => "/device-color".to_string(),
                };
                let resp = http_get(DAEMON_SOCKET, &path)?;
                // Parse the JSON response...
                // Return None if supports_readback is false
            }
            Self::Direct(dev) => {
                match dev.get_color() {
                    Some(Ok(c)) => Ok(Some(c)),
                    Some(Err(e)) => Err(e.into()),
                    None => Ok(None),
                }
            }
            Self::DirectMulti(devs) => {
                // Read from first device
                if let Some(dev) = devs.first() {
                    match dev.get_color() {
                        Some(Ok(c)) => Ok(Some(c)),
                        Some(Err(e)) => Err(e.into()),
                        None => Ok(None),
                    }
                } else {
                    Ok(None)
                }
            }
        }
    }
}
```

Note: `daemon_client.rs` currently only has `http_post`. A simple `http_get` helper (or reuse of the existing pattern with GET method) will be needed.

---

## Phase 7: Documentation Updates

### 7a. Protocol Reference

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/docs/reference/protocol.md`

Major update to document:

1. Remove the "Write-only" statement from the Communication Pattern section.
2. Add new sections for each discovered command:
   - CMD 0x00 -- Device Info (request/response format)
   - CMD 0x01 -- Serial Number (request/response format with BCD explanation)
   - CMD 0x0A -- Set Color (existing, already documented)
   - CMD 0x0B -- Get Color (request/response format, BGR byte order)
3. Add a "Command Summary Table" listing all known commands.
4. Add notes about the HID Report Descriptor (33 bytes, usage page 0xFF00, input/output/feature report sizes).
5. Add notes about button behavior (firmware-only cycling, does not affect HID state).

### 7b. Daemon API Reference

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/docs/reference/daemon-api.md`

Add documentation for the new `GET /device-color` endpoint.

### 7c. FFI Reference

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/docs/reference/ffi.md`

Add documentation for the new `statuslight_get_color` function and new error codes (-9, -10, -11).

### 7d. CLI Reference

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/docs/reference/cli.md`

Update the `statuslight status` section to show the new color readback output.

### 7e. Plan Document

**File:** `/Users/hongjunwu/Documents/Git/StatusLight/docs/plans/022-device-state-reading.md`

Save this plan as the next sequential plan document.

---

## Implementation Sequence and Dependencies

The phases have the following dependency graph:

```
Phase 1 (protocol.rs)  ─┐
Phase 2 (error.rs)      ─┤
                         ├→ Phase 3 (device.rs - trait + Slicky impl)
                         │     │
                         │     ├→ Phase 4 (daemon api.rs)
                         │     ├→ Phase 5 (FFI lib.rs)
                         │     └→ Phase 6 (CLI main.rs + daemon_client.rs)
                         │
                         └→ Phase 7 (docs) -- can be done in parallel
```

Recommended implementation order:
1. Phase 1 + Phase 2 (independent, can be done in one commit)
2. Phase 3 (depends on Phase 1 and 2)
3. Phases 4, 5, 6 (depend on Phase 3; independent of each other)
4. Phase 7 (documentation, can be done at any point)

---

## Commit Plan

Following conventional commits:

1. `feat(core): add get-color protocol commands and response parsing`
   - Phase 1 + Phase 2 changes
2. `feat(core): add optional get_color() to StatusLightDevice trait`
   - Phase 3 changes
3. `feat(daemon): add GET /device-color endpoint for hardware color readback`
   - Phase 4 changes
4. `feat(ffi): add statuslight_get_color() for reading device color`
   - Phase 5 changes
5. `feat(cli): show device color in statuslight status`
   - Phase 6 changes
6. `docs: document Slicky protocol commands and color readback API`
   - Phase 7 changes

---

## Risk Assessment and Mitigations

1. **Backward compatibility**: The `get_color()` method has a default `None` implementation. All 7 non-Slicky drivers require zero changes. No existing tests or behavior will break.

2. **Thread safety**: `get_color(&self)` takes `&self` just like `set_color(&self)`. The daemon already serializes access via `Mutex<Vec<Box<dyn StatusLightDevice>>>`. No additional synchronization is needed.

3. **HID read timeout**: The `read_timeout(200ms)` matches the probe's tested value. If the device doesn't respond, we return `ReadTimeout` rather than hanging.

4. **FFI safety**: The new FFI function uses output pointers (`*mut u8`) rather than returning a struct, which is the simplest C-compatible pattern. Null pointer checks are included.

5. **Device open/close in FFI**: The current FFI pattern opens a fresh device for each call. This is maintained for `statuslight_get_color` -- open, write command, read response, close. This is stateless and consistent with existing behavior.

6. **Button state discrepancy**: The probe confirmed that the button's visual cycling does NOT update the HID-reported color. This is documented but not "fixed" -- it's firmware behavior. The `get_color()` method returns the last programmatically-set color, which is the correct HID state.

---

## What This Plan Does NOT Include (Intentional Scope Limits)

- **Blink(1) / BlinkStick color readback**: These devices use feature reports and likely support `get_feature_report()` for color reading. This is left as a future enhancement -- implementing it follows the exact same pattern established here.
- **Continuous polling / event streaming**: No WebSocket or polling loop for real-time color monitoring. The `get_color()` call is on-demand only.
- **Device info / serial query commands (0x00, 0x01)**: Protocol builders are added but not wired into the trait or API. These are included in `protocol.rs` for documentation completeness and future use (e.g., a `get_device_info()` trait method later).
- **Feature report reading**: The Slicky has a 1-byte feature report per its descriptor. Its purpose is unknown and left for future investigation.

---

### Critical Files for Implementation
- `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-core/src/protocol.rs` - Core protocol: add CMD 0x0B request builder and response parser
- `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-core/src/device.rs` - Trait change: add `get_color()` default method and Slicky implementation
- `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-core/src/error.rs` - Error types: add ReadTimeout and UnexpectedResponse variants
- `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-daemon/src/api.rs` - Daemon API: add GET /device-color endpoint and route
- `/Users/hongjunwu/Documents/Git/StatusLight/crates/statuslight-ffi/src/lib.rs` - FFI: add statuslight_get_color() with output pointer pattern
