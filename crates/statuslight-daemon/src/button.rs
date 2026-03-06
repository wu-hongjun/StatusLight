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
        // First read is baseline only — don't treat it as a button press.
        let mut last_seen: Option<Color> = None;
        let mut baseline_set = false;

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

            // First successful read is baseline — record it and move on.
            if !baseline_set {
                last_seen = Some(color);
                baseline_set = true;
                log::debug!("Button poll baseline: {}", color.to_hex());
                continue;
            }

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

            // Check-and-update current_color in a single lock to avoid races.
            {
                let mut current = state_clone.inner.current_color.lock().await;
                if *current == Some(color) {
                    // The daemon set this color itself; not a button press.
                    continue;
                }
                *current = Some(color);
            }

            log::info!("Button press detected: {} → {preset_name}", color.to_hex());

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
        "available" | "off" => ("", ""),
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
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(json) => {
                if json["ok"].as_bool() != Some(true) {
                    log::warn!(
                        "Slack status sync failed: {}",
                        json["error"].as_str().unwrap_or("unknown")
                    );
                } else {
                    log::info!("Slack status synced: {preset_name}");
                }
            }
            Err(e) => log::warn!("Slack status sync: failed to parse response: {e}"),
        },
        Err(e) => log::warn!("Slack status sync request failed: {e}"),
    }
}
