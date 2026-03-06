# FFI Reference

**Library:** `libstatuslight_ffi.a` (static) / `libstatuslight_ffi.dylib` (dynamic)

**Header:** `crates/statuslight-ffi/include/statuslight.h`

The FFI layer provides C-callable functions for use from Swift, C, or any language with C FFI support. Each call opens the device, performs the action, and closes it (stateless).

## Functions

### `statuslight_init`

```c
void statuslight_init(void);
```

Initialize logging. Safe to call multiple times — only the first call has effect.

### `statuslight_set_rgb`

```c
int32_t statuslight_set_rgb(uint8_t r, uint8_t g, uint8_t b);
```

Set the light to the given RGB color.

### `statuslight_set_hex`

```c
int32_t statuslight_set_hex(const char *hex);
```

Set the light to a hex color string (e.g., `"#FF0000"` or `"FF0000"`).

The `hex` pointer must be a valid, non-null, null-terminated UTF-8 C string.

### `statuslight_set_preset`

```c
int32_t statuslight_set_preset(const char *name);
```

Set the light to a named preset (e.g., `"red"`, `"busy"`, `"in-meeting"`).

The `name` pointer must be a valid, non-null, null-terminated UTF-8 C string.

### `statuslight_off`

```c
int32_t statuslight_off(void);
```

Turn the light off.

### `statuslight_is_connected`

```c
int32_t statuslight_is_connected(void);
```

Check if a Slicky device is connected. Returns `1` if connected, `0` if not. Never returns error codes.

### `statuslight_get_color`

```c
int32_t statuslight_get_color(uint8_t *r, uint8_t *g, uint8_t *b);
```

Read the current color from the device. On success, writes the RGB values to the provided pointers and returns `0`. Returns `-9` if the driver does not support color readback.

All three pointers must be valid, non-null pointers to writable `uint8_t` memory.

## Return Codes

All functions (except `statuslight_init` and `statuslight_is_connected`) return `int32_t`:

| Code | Meaning |
|------|---------|
| `0` | Success |
| `-1` | Device not found |
| `-2` | Multiple devices found |
| `-3` | HID communication error (or internal panic) |
| `-4` | Invalid color value or unknown preset |
| `-5` | Invalid argument (null pointer, bad UTF-8) |
| `-6` | Write failed (byte count mismatch) |
| `-7` | Unknown or invalid preset |
| `-8` | Unknown driver |
| `-9` | Readback not supported |
| `-10` | Unexpected device response |

## Swift Usage Example

```swift
import Foundation

// Link against libstatuslight_ffi.a and include statuslight.h via bridging header

statuslight_init()

let result = statuslight_set_preset("available")
if result == 0 {
    print("Light set to available")
} else {
    print("Error: \(result)")
}

if statuslight_is_connected() == 1 {
    print("Device is connected")
}
```

## Building the Library

```bash
cargo build -p statuslight-ffi --release
```

Output files:

- `target/release/libstatuslight_ffi.a` — static library
- `target/release/libstatuslight_ffi.dylib` — dynamic library
- `crates/statuslight-ffi/include/statuslight.h` — generated C header
