# HID Protocol Reference

The Lexcelon Slicky-1.0 is a USB HID device that supports bidirectional vendor-specific HID communication for controlling its LED color and querying device state.

## Device Identification

| Field | Value |
|-------|-------|
| Vendor ID | `0x04D8` (Microchip Technology) |
| Product ID | `0xEC24` |
| Manufacturer | Lexcelon |
| Product | Slicky-1.0 |

## Command Summary

All commands use a 65-byte buffer: report ID (`0x00`) + 64-byte payload. The command byte is at offset 1.

| CMD | Name | Direction | Description |
|-----|------|-----------|-------------|
| `0x00` | Device Info | Host → Device → Host | Query hardware/firmware version |
| `0x01` | Serial | Host → Device → Host | Query device serial number |
| `0x0A` | Set Color | Host → Device | Set the LED to a specific RGB color |
| `0x0B` | Get Color | Host → Device → Host | Read the current LED color |

## CMD 0x0A — Set Color

### Request (65 bytes)

```
Index: [0]   [1]   [2]   [3]   [4]   [5]   [6]   [7]   [8]   [9..64]
Value: 0x00  0x0A  0x04  0x00  0x00  0x00  BLUE  GRN   RED   0x00...
       ^^^^  ^^^^  ^^^^                    ^^^^  ^^^^  ^^^^
       rpt   cmd   sub                     B     G     R
       ID
```

| Offset | Size | Description |
|--------|------|-------------|
| 0 | 1 | Report ID — always `0x00` |
| 1 | 1 | Command — `0x0A` (set color) |
| 2 | 1 | Subcommand — `0x04` |
| 3–5 | 3 | Reserved — `0x00` |
| 6 | 1 | **Blue** channel (0–255) |
| 7 | 1 | **Green** channel (0–255) |
| 8 | 1 | **Red** channel (0–255) |
| 9–64 | 56 | Padding — `0x00` |

> **BGR byte order**: The color bytes are in **BGR** order, not RGB. Blue is at index 6, green at 7, red at 8.

### Turn Off

To turn the light off, send a set-color report with R=0, G=0, B=0. The command and subcommand bytes remain the same.

## CMD 0x0B — Get Color

Reads the current LED color from the device.

### Request (65 bytes)

```
Index: [0]   [1]   [2..64]
Value: 0x00  0x0B  0x00...
       ^^^^  ^^^^
       rpt   cmd
       ID
```

### Response (64 bytes, read via HID input report)

```
Index: [0]   [1]   [2]   [3]   [4]   [5]   [6]   [7]   [8..63]
Value: 0x0B  0x04  0x00  0x00  0x00  BLUE  GRN   RED   0x00...
       ^^^^  ^^^^                    ^^^^  ^^^^  ^^^^
       cmd   sub                     B     G     R
```

| Offset | Size | Description |
|--------|------|-------------|
| 0 | 1 | Command echo — `0x0B` |
| 1 | 1 | Subcommand — `0x04` |
| 2–4 | 3 | Reserved — `0x00` |
| 5 | 1 | **Blue** channel (0–255) |
| 6 | 1 | **Green** channel (0–255) |
| 7 | 1 | **Red** channel (0–255) |
| 8–63 | 56 | Padding — `0x00` |

A read timeout of 200ms is used. The response color bytes are also in BGR order, at offsets 5-7 (shifted by one compared to the set-color command since there is no report ID byte in the input report).

## CMD 0x00 — Device Info

Queries hardware and firmware version information.

### Request (65 bytes)

```
Index: [0]   [1]   [2..64]
Value: 0x00  0x00  0x00...
       ^^^^  ^^^^
       rpt   cmd
       ID
```

### Response (64 bytes)

```
Index: [0]   [1]   [2]   [3]   [4]   [5]   [6..63]
Value: 0x00  0x02  0x00  0x00  0x02  0x01  0x00...
       ^^^^  ^^^^              ^^^^  ^^^^
       cmd   len?              fw?   fw?
```

The exact interpretation of the response fields beyond the command echo byte is not fully documented. The observed values suggest firmware version information.

## CMD 0x01 — Serial Query

Queries the device serial number.

### Request (65 bytes)

```
Index: [0]   [1]   [2..64]
Value: 0x00  0x01  0x00...
       ^^^^  ^^^^
       rpt   cmd
       ID
```

### Response (64 bytes)

```
Index: [0]   [1]   [2]   [3]   [4]   [5]   [6]   [7]   [8..63]
Value: 0x01  0x04  0x00  0x00  0x77  0x79  0x71  0x99  0x00...
       ^^^^  ^^^^              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       cmd   len?              serial (BCD-encoded digits)
```

The serial number bytes at offsets 4-7 contain BCD-encoded digits. For example, `0x77 0x79 0x71 0x99` decodes to serial `"77971799"`.

## HID Report Descriptor

The device declares a 33-byte HID report descriptor using vendor-specific usage page `0xFF00`:

| Report Type | Size |
|-------------|------|
| Input | 64 bytes |
| Output | 64 bytes |
| Feature | 1 byte |

Commands are sent as Output reports (65 bytes including report ID). Responses are read as Input reports (64 bytes, no report ID prefix).

## Button Behavior

The Slicky-1.0 has a physical button that cycles through a fixed color sequence:

**Cycle order:** Custom color → White → Red → Yellow → Green → Off → White → Red → ...

- The button cycling is handled entirely in firmware
- The button does **not** generate HID input reports
- Whether CMD 0x0B reflects the button-cycled color (vs. the last HID-set color) is under investigation
- The cycle colors (white, red, yellow, green) correspond to common status indicators and could be mapped to statuses (e.g., available, busy, away, in-meeting)

## Communication Pattern

- **Bidirectional**: The host sends output reports and reads input reports for query responses
- **Stateless**: Each command is independent. No handshake or session required
- **Single report**: One 65-byte write per command, one 64-byte read per response
- **Read timeout**: Use a 200ms timeout when reading responses; not all commands may produce a response on all firmware versions

## Constants

```rust
pub const VENDOR_ID: u16 = 0x04D8;
pub const PRODUCT_ID: u16 = 0xEC24;
pub const REPORT_SIZE: usize = 64;       // HID report payload
pub const BUFFER_SIZE: usize = 65;       // report ID + payload
pub const READ_TIMEOUT_MS: i32 = 200;    // read timeout for responses
```

## Reverse Engineering Notes

The protocol was reverse-engineered in two phases:

1. **Phase 1 (write commands)**: USB traffic capture from the original Lexcelon desktop application using Wireshark with USBPcap. Discovered CMD 0x0A set-color with BGR byte order.

2. **Phase 2 (read commands)**: Systematic probing of all 256 command bytes using `tools/slicky-probe`. Discovered CMD 0x00 (device info), CMD 0x01 (serial), and CMD 0x0B (color readback). Most command bytes produce no response.
