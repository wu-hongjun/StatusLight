# Plan 023 — Button-to-Status Sync

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Let users change their status by pressing the Slicky's physical button — the daemon detects the color change via CMD 0x0B polling and maps it to a preset status, optionally syncing to Slack.

**Architecture:** A new background polling task in the daemon reads the device color every 2 seconds. When it detects a change to a known button-cycle color (white, red, yellow, green, off), it maps it to a status preset and optionally updates the user's Slack status. The "in-meeting" preset default color changes from orange to white to align with the first button-cycle color.

**Tech Stack:** Rust, tokio (async polling), hidapi (CMD 0x0B), Slack Web API (`users.profile.set`)

---

## Experimental Findings (from Plan 022)

Button cycle with exact hex values confirmed via CMD 0x0B readback:

| Press | Visual | Hex | Proposed Status |
|-------|--------|-----|-----------------|
| From custom | White | `#FFFFFF` | In Meeting |
| 1st | Red | `#FF0000` | Busy |
| 2nd | Yellow | `#FFFF00` | Away |
| 3rd | Green | `#00FF00` | Available |
| 4th | Off | `#000000` | Off (clear status) |

## Design Decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| Polling interval | 2 seconds | Fast enough for responsive UX, low CPU cost for a single HID read |
| Where does polling live? | New daemon task (`button_poll`) | Parallel to existing `emoji_poll`, uses same device pool |
| Color matching | Exact hex match only | Button colors are always pure (0x00/0xFF channels); fuzzy matching not needed |
| "in-meeting" default color | Change from `#FF4500` to `#FFFFFF` | Aligns with first button-cycle color (white) |
| Button-to-Slack sync | Opt-in via config | `[button] slack_sync = true` in config.toml |
| Conflict with emoji_poll | Button poll takes priority when `manual_override` is true | Button press is a form of manual input |
| Config for color→status map | Hardcoded initially | The Slicky button cycle is fixed in firmware; no need to make it configurable yet |

---

## Phase 1: Change "In Meeting" Default Color

### Task 1.1: Update `InMeeting` preset color

**File:** `crates/statuslight-core/src/color.rs`

Change line in `Preset::color()`:

```rust
// Before:
Self::InMeeting => Color::new(255, 69, 0),

// After:
Self::InMeeting => Color::new(255, 255, 255),
```

### Task 1.2: Update test expectations

**File:** `crates/statuslight-core/src/color.rs`

Find any test that asserts the InMeeting color value and update it to match `(255, 255, 255)`.

### Task 1.3: Verify

```bash
cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

**Commit:** `feat(core): change in-meeting preset default from orange to white`

---

## Phase 2: Button Color Map & Config

### Task 2.1: Add button color constants to `protocol.rs`

**File:** `crates/statuslight-core/src/protocol.rs`

Add after the existing constants, before `build_set_color_report`:

```rust
/// Known colors from the Slicky button cycle, in cycle order.
/// These are the exact values returned by CMD 0x0B after each button press.
pub const BUTTON_CYCLE_COLORS: &[(Color, &str)] = &[
    (Color { r: 255, g: 255, b: 255 }, "in-meeting"),  // white
    (Color { r: 255, g: 0, b: 0 }, "busy"),             // red
    (Color { r: 255, g: 255, b: 0 }, "away"),           // yellow
    (Color { r: 0, g: 255, b: 0 }, "available"),         // green
    (Color { r: 0, g: 0, b: 0 }, "off"),                 // off
];

/// Look up the preset name for a button-cycle color, if it matches exactly.
pub fn button_cycle_preset(color: Color) -> Option<&'static str> {
    BUTTON_CYCLE_COLORS
        .iter()
        .find(|(c, _)| *c == color)
        .map(|(_, name)| *name)
}
```

This requires `Color` to implement `PartialEq`. Check if it already does — if not, add `#[derive(PartialEq)]` to the `Color` struct in `color.rs`.

### Task 2.2: Add button config to `Config`

**File:** `crates/statuslight-core/src/config.rs`

Add a new config section:

```rust
/// Button integration settings.
#[derive(Debug, Serialize, Deserialize)]
pub struct ButtonConfig {
    /// Enable the daemon button-polling loop.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Polling interval in seconds.
    #[serde(default = "default_button_poll_interval")]
    pub poll_interval_secs: u64,
    /// Sync button-press status changes to Slack (requires Slack tokens).
    #[serde(default)]
    pub slack_sync: bool,
}

fn default_true() -> bool {
    true
}

fn default_button_poll_interval() -> u64 {
    2
}

impl Default for ButtonConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            poll_interval_secs: 2,
            slack_sync: false,
        }
    }
}
```

Add to the main `Config` struct:

```rust
    /// Button integration settings.
    #[serde(default)]
    pub button: ButtonConfig,
```

### Task 2.3: Export new types from `lib.rs`

**File:** `crates/statuslight-core/src/lib.rs`

Ensure `ButtonConfig` is publicly exported. Add to the re-exports if needed.

### Task 2.4: Add tests for `button_cycle_preset`

**File:** `crates/statuslight-core/src/protocol.rs` (inside `mod tests`)

```rust
    #[test]
    fn button_cycle_preset_white_is_in_meeting() {
        assert_eq!(
            button_cycle_preset(Color::new(255, 255, 255)),
            Some("in-meeting")
        );
    }

    #[test]
    fn button_cycle_preset_red_is_busy() {
        assert_eq!(
            button_cycle_preset(Color::new(255, 0, 0)),
            Some("busy")
        );
    }

    #[test]
    fn button_cycle_preset_unknown_color() {
        assert_eq!(
            button_cycle_preset(Color::new(128, 64, 32)),
            None
        );
    }

    #[test]
    fn button_cycle_preset_off() {
        assert_eq!(
            button_cycle_preset(Color::new(0, 0, 0)),
            Some("off")
        );
    }
```

### Task 2.5: Verify

```bash
cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

**Commit:** `feat(core): add button cycle color map and ButtonConfig`

---

## Phase 3: Daemon Button Polling

### Task 3.1: Add button poll state to `AppStateInner`

**File:** `crates/statuslight-daemon/src/state.rs`

Add to `AppStateInner`:

```rust
    /// Handle to the button polling task, if running.
    pub(crate) button_poll_handle: Mutex<Option<JoinHandle<()>>>,
```

Initialize in `AppState::new()`:

```rust
    button_poll_handle: Mutex::new(None),
```

### Task 3.2: Implement button poll loop

**File:** `crates/statuslight-daemon/src/button.rs` (new file)

```rust
//! Background button-press polling for Slicky devices.

use std::sync::atomic::Ordering;
use std::time::Duration;

use statuslight_core::{protocol, Color, DeviceRegistry};

use crate::state::AppState;

/// Start the button polling task.
///
/// Polls the device color every `interval` seconds. When the color changes to
/// a known button-cycle color (and differs from the last color set by the
/// daemon), it is treated as a button press — the daemon's current_color is
/// updated and, if configured, the Slack status is synced.
pub async fn start_button_poll(state: &AppState, interval_secs: u64, slack_sync: bool) {
    stop_button_poll(state).await;

    let state_clone = state.clone();
    let handle = tokio::spawn(async move {
        let mut last_seen: Option<Color> = None;

        loop {
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;

            // Read the current color from the device.
            let color = {
                let mut devices = state_clone.inner.devices.lock().await;
                if devices.is_empty() {
                    if let Ok(dev) = DeviceRegistry::with_builtins().open_any() {
                        devices.push(dev);
                    }
                }
                if devices.is_empty() {
                    continue;
                }
                match devices[0].get_color() {
                    Some(Ok(c)) => c,
                    _ => continue,
                }
            };

            // Skip if color hasn't changed since last poll.
            if last_seen == Some(color) {
                continue;
            }
            last_seen = Some(color);

            // Check if it matches a button-cycle color.
            let Some(preset_name) = protocol::button_cycle_preset(color) else {
                // Custom color set via API — not a button press.
                continue;
            };

            // Check if this differs from what the daemon last set.
            let daemon_color = *state_clone.inner.current_color.lock().await;
            if daemon_color == Some(color) {
                // The daemon set this color itself (e.g. via API); not a button press.
                continue;
            }

            log::info!("Button press detected: {} → {preset_name}", color.to_hex());

            // Update daemon's current_color to reflect the button press.
            *state_clone.inner.current_color.lock().await = Some(color);

            // Set manual_override so emoji polling doesn't fight the button.
            state_clone
                .inner
                .manual_override
                .store(true, Ordering::SeqCst);

            // Sync to Slack if configured.
            if slack_sync {
                sync_slack_status(&state_clone, preset_name).await;
            }
        }
    });

    *state.inner.button_poll_handle.lock().await = Some(handle);
}

/// Stop the button polling task.
pub async fn stop_button_poll(state: &AppState) {
    if let Some(handle) = state.inner.button_poll_handle.lock().await.take() {
        handle.abort();
    }
}

/// Map a preset name to a Slack status emoji and text, then update the profile.
async fn sync_slack_status(state: &AppState, preset_name: &str) {
    let slack = state.inner.slack.lock().await;
    let user_token = match slack.user_token.clone() {
        Some(t) => t,
        None => return,
    };
    drop(slack);

    let (emoji, text) = match preset_name {
        "in-meeting" => (":spiral_calendar_pad:", "In a meeting"),
        "busy" => (":no_entry:", "Busy"),
        "away" => (":away:", "Away"),
        "available" => ("", ""),  // clear status
        "off" => ("", ""),        // clear status
        _ => return,
    };

    let body = serde_json::json!({
        "profile": {
            "status_text": text,
            "status_emoji": emoji,
            "status_expiration": 0
        }
    });

    let client = state.inner.http_client.clone();
    match client
        .post("https://slack.com/api/users.profile.set")
        .bearer_auth(&user_token)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                if json["ok"].as_bool() != Some(true) {
                    log::warn!(
                        "Slack status sync failed: {}",
                        json["error"].as_str().unwrap_or("unknown")
                    );
                } else {
                    log::info!("Slack status synced: {preset_name}");
                }
            }
        }
        Err(e) => log::warn!("Slack status sync request failed: {e}"),
    }
}
```

### Task 3.3: Register module and start poll in `main.rs`

**File:** `crates/statuslight-daemon/src/main.rs`

Add module declaration:

```rust
mod button;
```

After the Slack startup block (after `state.inner.slack.lock().await.enabled = true;`), add:

```rust
    // Start button polling if enabled.
    if config.button.enabled {
        button::start_button_poll(
            &state,
            config.button.poll_interval_secs,
            config.button.slack_sync,
        )
        .await;
        log::info!(
            "Button polling enabled ({}s interval, slack_sync={})",
            config.button.poll_interval_secs,
            config.button.slack_sync
        );
    }
```

Before the final `slack::stop_all(&state).await;` in shutdown, add:

```rust
    button::stop_button_poll(&state).await;
```

### Task 3.4: Verify

```bash
cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

**Commit:** `feat(daemon): add button-press polling with optional Slack status sync`

---

## Phase 4: CLI & API Visibility

### Task 4.1: Show button status in `statuslight status`

**File:** `crates/statuslight-cli/src/main.rs`

In the `Commands::Status` arm, after printing the color, add a line that maps the color to a button-cycle preset name if it matches:

```rust
// After the Color: line, add:
if let Some(preset) = statuslight_core::protocol::button_cycle_preset(color) {
    println!("Status:  {preset}");
}
```

This applies in both direct and daemon modes. In daemon mode, parse the hex from the response and call `button_cycle_preset`.

### Task 4.2: Add `GET /button-status` daemon endpoint

**File:** `crates/statuslight-daemon/src/api.rs`

Add a new route `.route("/button-status", get(get_button_status))` and handler:

```rust
#[derive(Serialize)]
struct ButtonStatusResponse {
    detected_preset: Option<String>,
    device_color: Option<ColorResponse>,
    polling_enabled: bool,
    slack_sync: bool,
}
```

The handler reads the current device color (from `state.inner.current_color`), maps it via `button_cycle_preset`, and returns the result.

### Task 4.3: Verify

```bash
cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

**Commit:** `feat: show button status in CLI and add /button-status endpoint`

---

## Phase 5: Documentation

### Task 5.1: Update protocol docs

**File:** `docs/reference/protocol.md`

Update the "Button Behavior" section to confirm CMD 0x0B reflects button state and add the color→status mapping table.

### Task 5.2: Update daemon API docs

**File:** `docs/reference/daemon-api.md`

Add `GET /button-status` endpoint documentation.

### Task 5.3: Add button config to docs

Document the `[button]` config section with `enabled`, `poll_interval_secs`, and `slack_sync` fields.

### Task 5.4: Save plan

Already saved as `docs/plans/023-button-status-sync.md`.

**Commit:** `docs: document button-to-status sync feature`

---

## Phase 6: Build, Install & Verify

### Task 6.1: Full workspace check

```bash
cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace
```

### Task 6.2: Build and install

```bash
cargo build --workspace --release
cargo install --path crates/statuslight-cli
bash scripts/build-app.sh 0.3.0
cp -R target/release/StatusLight.app /Applications/StatusLight.app
```

### Task 6.3: End-to-end test with physical device

1. Start daemon: verify "Button polling enabled" in logs
2. Press button through full cycle: verify daemon logs "Button press detected" for each
3. Run `statuslight status`: verify `Status: busy` / `Status: available` etc.
4. If Slack configured with `slack_sync = true`: verify Slack status updates on button press

### Task 6.4: Code review

Dispatch `codex-rust-reviewer` for statuslight-core and statuslight-daemon.

---

## Files Modified

| File | Changes |
|------|---------|
| `crates/statuslight-core/src/color.rs` | InMeeting preset: orange → white |
| `crates/statuslight-core/src/protocol.rs` | Button cycle color map, `button_cycle_preset()` |
| `crates/statuslight-core/src/config.rs` | Add `ButtonConfig` struct |
| `crates/statuslight-core/src/lib.rs` | Export `ButtonConfig` |
| `crates/statuslight-daemon/src/state.rs` | Add `button_poll_handle` |
| `crates/statuslight-daemon/src/main.rs` | Start/stop button poll |
| `crates/statuslight-daemon/src/api.rs` | Add `GET /button-status` |
| `crates/statuslight-cli/src/main.rs` | Show status preset name in `statuslight status` |
| `docs/reference/protocol.md` | Confirm button readback, add mapping table |
| `docs/reference/daemon-api.md` | Document `/button-status` endpoint |

## New Files

| File | Purpose |
|------|---------|
| `crates/statuslight-daemon/src/button.rs` | Button poll loop + Slack sync |
| `docs/plans/023-button-status-sync.md` | This plan |

## Verification

After each phase:
1. `cargo fmt --all && cargo clippy --workspace -- -D warnings && cargo test --workspace`

End-to-end (with physical Slicky device):
- Press button → daemon logs "Button press detected: #FF0000 → busy"
- `statuslight status` shows `Status: busy`
- With `slack_sync = true`: Slack profile updates to `:no_entry: Busy`
- Press to green → Slack status clears
