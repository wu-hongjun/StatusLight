# Daemon API Reference

**Binary:** `statuslightd`

The daemon listens on a Unix domain socket (default `/tmp/statuslight.sock`) and exposes a JSON REST API.

## Starting the Daemon

```bash
statuslightd                                          # default socket
statuslightd --socket /tmp/statuslight.sock                # custom socket
statuslightd --slack-token xoxp-... --slack-interval 30  # with Slack
```

## Endpoints

### `GET /status`

Returns current daemon state.

**Response 200:**

```json
{
  "device_connected": true,
  "current_color": { "r": 255, "g": 0, "b": 0, "hex": "#FF0000" },
  "slack_sync_enabled": false
}
```

When no device is connected, `current_color` is `null`.

---

### `POST /color`

Set by preset name or hex string.

**Request:**

```json
{ "color": "red" }
{ "color": "#FF8000" }
```

**Response 200:**

```json
{ "color": { "r": 255, "g": 0, "b": 0, "hex": "#FF0000" } }
```

**Response 400** (bad input):

```json
{ "error": "unknown preset: foobar" }
```

**Response 503** (no device):

```json
{ "error": "no Slicky device found (VID=0x04D8, PID=0xEC24)" }
```

---

### `POST /rgb`

Set by exact RGB values.

**Request:**

```json
{ "r": 255, "g": 128, "b": 0 }
```

**Response 200:**

```json
{ "color": { "r": 255, "g": 128, "b": 0, "hex": "#FF8000" } }
```

---

### `POST /off`

Turn off the light. No request body required.

**Response 200:**

```json
{ "color": { "r": 0, "g": 0, "b": 0, "hex": "#000000" } }
```

---

### `GET /presets`

List all available presets.

**Response 200:**

```json
[
  { "name": "red", "hex": "#FF0000" },
  { "name": "green", "hex": "#00FF00" },
  { "name": "blue", "hex": "#0000FF" }
]
```

---

### `GET /devices`

List connected Slicky devices.

**Response 200:**

```json
[
  {
    "path": "DevSrvsID:4298190949",
    "serial": "77971799",
    "manufacturer": "Lexcelon",
    "product": "Slicky-1.0"
  }
]
```

---

### `GET /device-color`

Read the current color directly from the device hardware (if the driver supports readback).

**Query parameters:**

| Param | Type | Description |
|-------|------|-------------|
| `device` | string | Optional serial number to target a specific device |

**Response 200** (readback supported):

```json
{
  "device_color": { "r": 255, "g": 0, "b": 0, "hex": "#FF0000" },
  "supports_readback": true
}
```

**Response 200** (readback not supported):

```json
{
  "device_color": null,
  "supports_readback": false
}
```

**Response 500** (read failed):

```json
{ "error": "device read timed out" }
```

**Response 503** (no device):

```json
{ "error": "no compatible device found" }
```

---

### `GET /slack/status`

Get Slack integration status.

**Response 200:**

```json
{
  "enabled": false,
  "poll_interval_secs": 30,
  "has_token": false,
  "emoji_map": {}
}
```

---

### `POST /slack/configure`

Configure Slack integration.

**Request:**

```json
{
  "token": "xoxp-...",
  "poll_interval_secs": 30,
  "emoji_map": {
    ":no_entry:": "#FF0000",
    ":calendar:": "#FF4500",
    ":palm_tree:": "#808080"
  }
}
```

All fields are optional — only provided fields are updated.

**Response 200:**

```json
{ "enabled": true, "poll_interval_secs": 30 }
```

---

### `POST /slack/enable`

Start Slack polling. A token must already be configured.

**Response 200:**

```json
{ "enabled": true }
```

**Response 400** (no token):

```json
{ "error": "no Slack token configured" }
```

---

### `POST /slack/disable`

Stop Slack polling.

**Response 200:**

```json
{ "enabled": false }
```

## Error Responses

All errors return a JSON body with an `error` field:

```json
{ "error": "description of what went wrong" }
```

| Status | Meaning |
|--------|---------|
| 400 | Bad input (invalid color, unknown preset, missing token) |
| 500 | Internal error (HID communication failure) |
| 503 | Device not available |
