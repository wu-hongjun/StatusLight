//! Slack integration for the daemon: Socket Mode client, rule matcher,
//! emoji status poller, and event animation runner.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use slicky_core::{AnimationType, Color, HidSlickyDevice, Preset, SlackRule, SlickyDevice};
use tokio_tungstenite::tungstenite::Message;

use crate::state::AppState;

// ---------------------------------------------------------------------------
// (A) Socket Mode WebSocket client
// ---------------------------------------------------------------------------

/// POST `apps.connections.open` to obtain a WebSocket URL for Socket Mode.
async fn get_ws_url(client: &reqwest::Client, app_token: &str) -> anyhow::Result<String> {
    let resp: serde_json::Value = client
        .post("https://slack.com/api/apps.connections.open")
        .bearer_auth(app_token)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await?
        .json()
        .await?;

    if !resp["ok"].as_bool().unwrap_or(false) {
        let err = resp["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("apps.connections.open failed: {err}");
    }

    resp["url"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("missing url in apps.connections.open response"))
}

/// Start the Socket Mode WebSocket connection in a background task.
///
/// The task reconnects automatically with exponential backoff on disconnection.
/// Only `app_token` is captured at spawn time (needed for WebSocket URL);
/// `rules`, `user_id`, and `emoji_colors` are read from `state.inner.slack`
/// on each event so that config changes take effect without restarting.
pub async fn start_socket_mode(state: &AppState) {
    stop_socket_mode(state).await;

    let mut slack = state.inner.slack.lock().await;
    let app_token = match &slack.app_token {
        Some(t) => t.clone(),
        None => return,
    };
    let state_clone = state.clone();

    let handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(60);

        loop {
            match run_socket_loop(&client, &app_token, &state_clone).await {
                Ok(()) => {
                    log::info!("Socket Mode connection closed normally");
                    backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    log::warn!("Socket Mode error: {e}, reconnecting in {backoff:?}");
                }
            }
            state_clone
                .inner
                .socket_mode_connected
                .store(false, Ordering::SeqCst);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    });

    slack.socket_handle = Some(handle);
}

/// Run a single Socket Mode WebSocket session. Returns when the connection drops.
///
/// `rules`, `user_id`, and `emoji_colors` are read from shared state on each
/// event so that configuration changes take effect without restarting.
async fn run_socket_loop(
    client: &reqwest::Client,
    app_token: &str,
    state: &AppState,
) -> anyhow::Result<()> {
    let url = get_ws_url(client, app_token).await?;
    // Log only scheme+host — the URL contains a session ticket.
    let redacted = url.split('?').next().unwrap_or("wss://...");
    log::info!("Socket Mode connecting to {redacted}");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await?;
    let (mut write, mut read) = ws_stream.split();
    log::info!("Socket Mode connected");
    state
        .inner
        .socket_mode_connected
        .store(true, Ordering::SeqCst);

    while let Some(msg) = read.next().await {
        let msg = msg?;
        match msg {
            Message::Text(text) => {
                if let Ok(envelope) = serde_json::from_str::<serde_json::Value>(&text) {
                    // Acknowledge the envelope immediately.
                    if let Some(envelope_id) = envelope["envelope_id"].as_str() {
                        let ack = serde_json::json!({ "envelope_id": envelope_id });
                        write.send(Message::Text(ack.to_string())).await?;
                    }

                    // Dispatch event — read rules/user_id/emoji_colors from state.
                    if envelope["type"].as_str() == Some("events_api") {
                        if let Some(event) = envelope.get("payload").and_then(|p| p.get("event")) {
                            handle_event(event, state).await;
                        }
                    }
                }
            }
            Message::Ping(data) => {
                write.send(Message::Pong(data)).await?;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    state
        .inner
        .socket_mode_connected
        .store(false, Ordering::SeqCst);
    Ok(())
}

/// Stop the Socket Mode task, if running.
pub async fn stop_socket_mode(state: &AppState) {
    let mut slack = state.inner.slack.lock().await;
    if let Some(handle) = slack.socket_handle.take() {
        handle.abort();
    }
}

// ---------------------------------------------------------------------------
// (B) Rule matcher and event handler
// ---------------------------------------------------------------------------

/// Dispatch a Slack event against the configured rules (first match wins).
///
/// Slack sends `type = "message"` with `channel_type = "im"` for DMs.
/// We normalize this to `message.im` so rules can use `event = "message.im"`.
///
/// `user_change` events are handled separately: we extract the status emoji
/// from the user's profile and map it to a color via `emoji_colors`.
///
/// All config (`rules`, `user_id`, `emoji_colors`) is read from shared state
/// per event so that changes take effect without restarting.
async fn handle_event(event: &serde_json::Value, state: &AppState) {
    let raw_type = event["type"].as_str().unwrap_or("");

    // Handle user_change events for bidirectional status sync.
    if raw_type == "user_change" {
        let slack = state.inner.slack.lock().await;
        let user_id = slack.user_id.clone();
        let emoji_colors = slack.emoji_colors.clone();
        drop(slack);
        handle_user_change(event, user_id.as_deref(), &emoji_colors, state).await;
        return;
    }

    let event_subtype = event["subtype"].as_str();

    // Skip bot messages to avoid echo loops.
    if event_subtype == Some("bot_message") || event.get("bot_id").is_some() {
        return;
    }

    // Normalize: "message" + channel_type "im" → "message.im"
    let event_type = if raw_type == "message" {
        if let Some(channel_type) = event["channel_type"].as_str() {
            format!("{raw_type}.{channel_type}")
        } else {
            raw_type.to_string()
        }
    } else {
        raw_type.to_string()
    };

    let rules = state.inner.slack.lock().await.rules.clone();
    for rule in &rules {
        if rule_matches(rule, &event_type, event) {
            log::info!("Rule '{}' matched event '{event_type}'", rule.name);
            trigger_animation(rule, state).await;
            return;
        }
    }
}

/// Handle a `user_change` event: sync the user's Slack status emoji to the light.
///
/// Skips if the event is for a different user, or if an event animation is active.
/// Clears `manual_override` when the user explicitly changes their Slack status,
/// since they want Slack-driven sync to take effect.
async fn handle_user_change(
    event: &serde_json::Value,
    user_id: Option<&str>,
    emoji_colors: &HashMap<String, String>,
    state: &AppState,
) {
    let our_uid = match user_id {
        Some(id) => id,
        None => {
            log::warn!("user_change: skipping — user_id not resolved (auth.test may have failed at startup)");
            return;
        }
    };

    // user_change fires for ALL users — filter to our own.
    let changed_uid = event["user"]["id"].as_str().unwrap_or("");
    if changed_uid != our_uid {
        return;
    }

    // User explicitly changed their Slack status — clear manual override so
    // Slack-driven sync takes effect again.
    state.inner.manual_override.store(false, Ordering::SeqCst);

    // Don't overwrite an in-progress event animation.
    if state.inner.event_animation_active.load(Ordering::SeqCst) {
        return;
    }

    let profile = &event["user"]["profile"];
    let emoji = match profile["status_emoji"].as_str() {
        Some(e) if !e.is_empty() => e,
        _ => {
            log::debug!("user_change: no status emoji set");
            return;
        }
    };

    // Check if the status has expired.
    if let Some(exp) = profile["status_expiration"].as_i64() {
        if exp > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            if exp < now {
                log::debug!("user_change: status emoji expired");
                return;
            }
        }
    }

    match emoji_colors.get(emoji) {
        Some(hex) => match Color::from_hex(hex) {
            Ok(color) => {
                log::info!("user_change: status emoji {emoji} → {hex}");
                set_device_color(state, color).await;
            }
            Err(e) => {
                log::warn!("user_change: invalid color {hex} for {emoji}: {e}");
            }
        },
        None => {
            log::debug!("user_change: no color mapping for {emoji}");
        }
    }
}

/// Check whether a single rule matches the given event.
fn rule_matches(rule: &SlackRule, event_type: &str, event: &serde_json::Value) -> bool {
    if rule.event != event_type {
        return false;
    }

    if let Some(ref user_id) = rule.from_user {
        let sender = event["user"].as_str().unwrap_or("");
        if sender != user_id {
            return false;
        }
    }

    if let Some(ref substring) = rule.contains {
        let text = event["text"].as_str().unwrap_or("");
        if !text.to_lowercase().contains(&substring.to_lowercase()) {
            return false;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// (C) Event animation runner
// ---------------------------------------------------------------------------

/// Play a temporary animation triggered by a matched rule, then restore.
async fn trigger_animation(rule: &SlackRule, state: &AppState) {
    let anim = match AnimationType::from_name(&rule.animation) {
        Some(a) => a,
        None => {
            log::warn!(
                "Unknown animation type in rule '{}': {}",
                rule.name,
                rule.animation
            );
            return;
        }
    };

    let color = match Color::from_hex(&rule.color) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Invalid color in rule '{}': {e}", rule.name);
            return;
        }
    };

    let speed = if rule.speed > 0.0 { rule.speed } else { 1.0 };
    let duration = rule
        .duration_secs
        .unwrap_or_else(|| (rule.repeat as f64) * anim.period() / speed);

    let prev_color = *state.inner.current_color.lock().await;

    // Cancel any in-progress event animation.
    if let Some(h) = state.inner.event_animation_handle.lock().await.take() {
        h.abort();
    }

    state
        .inner
        .event_animation_active
        .store(true, Ordering::SeqCst);

    let state_clone = state.clone();
    let handle = tokio::spawn(async move {
        let start = Instant::now();
        let mut last_frame = color;
        while start.elapsed().as_secs_f64() < duration {
            let frame = anim.frame(start.elapsed().as_secs_f64(), speed, &[color]);
            last_frame = frame;
            set_device_color(&state_clone, frame).await;
            tokio::time::sleep(Duration::from_millis(33)).await;
        }

        // Restore previous color only if no other source changed it during animation.
        state_clone
            .inner
            .event_animation_active
            .store(false, Ordering::SeqCst);
        let current = *state_clone.inner.current_color.lock().await;
        if current == Some(last_frame) {
            if let Some(c) = prev_color {
                set_device_color(&state_clone, c).await;
            }
        }
    });

    *state.inner.event_animation_handle.lock().await = Some(handle);
}

/// Set color on the HID device, reconnecting if needed.
async fn set_device_color(state: &AppState, color: Color) {
    let mut device_guard = state.inner.device.lock().await;
    if device_guard.is_none() {
        if let Ok(dev) = HidSlickyDevice::open() {
            *device_guard = Some(dev);
        }
    }
    if let Some(dev) = device_guard.as_ref() {
        if let Err(e) = dev.set_color(color) {
            log::warn!("Failed to set device color: {e}");
            *device_guard = None;
        } else {
            drop(device_guard);
            *state.inner.current_color.lock().await = Some(color);
        }
    }
}

// ---------------------------------------------------------------------------
// (D) Emoji status poller
// ---------------------------------------------------------------------------

/// Start the background emoji polling task (60s interval).
///
/// Uses the user_token to read the profile emoji and maps it to a color via
/// the `emoji_colors` config. Skips when an event animation is active or when
/// the user has manually set a color (`manual_override`).
///
/// `user_token` and `emoji_colors` are read from shared state on each
/// iteration so that config changes take effect without restarting.
pub async fn start_emoji_poll(state: &AppState) {
    stop_emoji_poll(state).await;

    let slack = state.inner.slack.lock().await;
    if slack.user_token.is_none() {
        return;
    }
    drop(slack);

    let state_clone = state.clone();

    let handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            // Skip if an event animation is playing or manual override is set.
            let skip = state_clone
                .inner
                .event_animation_active
                .load(Ordering::SeqCst)
                || state_clone.inner.manual_override.load(Ordering::SeqCst);

            if !skip {
                // Read current config from shared state each iteration.
                let slack = state_clone.inner.slack.lock().await;
                let user_token = slack.user_token.clone();
                let emoji_colors = slack.emoji_colors.clone();
                drop(slack);

                if let Some(ref token) = user_token {
                    match fetch_emoji_color(&client, token, &emoji_colors).await {
                        Ok(Some(color)) => {
                            set_device_color(&state_clone, color).await;
                        }
                        Ok(None) => {
                            // No emoji match — fall back to presence-based color.
                            log::debug!("Emoji poll: no matching status emoji, checking presence");
                            match fetch_presence(&client, token).await {
                                Ok(Some(color)) => {
                                    set_device_color(&state_clone, color).await;
                                }
                                Ok(None) => {
                                    log::debug!("Presence poll: no presence data");
                                }
                                Err(e) => {
                                    log::warn!("Presence poll error: {e}");
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Emoji poll error: {e}");
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    let mut slack = state.inner.slack.lock().await;
    slack.emoji_poll_handle = Some(handle);
}

/// Stop the emoji polling task, if running.
pub async fn stop_emoji_poll(state: &AppState) {
    let mut slack = state.inner.slack.lock().await;
    if let Some(handle) = slack.emoji_poll_handle.take() {
        handle.abort();
    }
}

/// Fetch the user's Slack status emoji and resolve it to a color via the
/// `emoji_colors` map (emoji → hex string → Color).
async fn fetch_emoji_color(
    client: &reqwest::Client,
    token: &str,
    emoji_colors: &HashMap<String, String>,
) -> anyhow::Result<Option<Color>> {
    let resp: serde_json::Value = client
        .get("https://slack.com/api/users.profile.get")
        .bearer_auth(token)
        .send()
        .await?
        .json()
        .await?;

    if !resp["ok"].as_bool().unwrap_or(false) {
        let err_msg = resp["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("Slack API error: {err_msg}");
    }

    let profile = &resp["profile"];
    let emoji = match profile["status_emoji"].as_str() {
        Some(e) if !e.is_empty() => e,
        _ => return Ok(None),
    };

    // Check if the status has expired.
    if let Some(exp) = profile["status_expiration"].as_i64() {
        if exp > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            if exp < now {
                return Ok(None);
            }
        }
    }

    match emoji_colors.get(emoji) {
        Some(hex) => Ok(Some(Color::from_hex(hex)?)),
        None => Ok(None),
    }
}

/// Fetch the authenticated user's Slack presence and map it to a color.
///
/// `"away"` → `Preset::Away` color, `"active"` → `Preset::Available` color.
async fn fetch_presence(client: &reqwest::Client, token: &str) -> anyhow::Result<Option<Color>> {
    let resp: serde_json::Value = client
        .get("https://slack.com/api/users.getPresence")
        .bearer_auth(token)
        .send()
        .await?
        .json()
        .await?;

    if !resp["ok"].as_bool().unwrap_or(false) {
        let err_msg = resp["error"].as_str().unwrap_or("unknown error");
        anyhow::bail!("Slack API error: {err_msg}");
    }

    match resp["presence"].as_str() {
        Some("away") => Ok(Some(Preset::Away.color())),
        Some("active") => Ok(Some(Preset::Available.color())),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// (E) User identity resolution
// ---------------------------------------------------------------------------

/// Resolve the authenticated user's Slack ID by calling `auth.test` with
/// the user token.  The returned ID is used to filter `user_change` events
/// (which fire for *every* user in the workspace).
pub async fn resolve_user_id(state: &AppState) -> Option<String> {
    let token = {
        let slack = state.inner.slack.lock().await;
        slack.user_token.clone()?
    };

    let client = reqwest::Client::new();
    let resp: serde_json::Value = match client
        .post("https://slack.com/api/auth.test")
        .bearer_auth(&token)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await
    {
        Ok(r) => match r.json().await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("Failed to parse auth.test response: {e}");
                return None;
            }
        },
        Err(e) => {
            log::warn!("Failed to call auth.test: {e}");
            return None;
        }
    };

    if !resp["ok"].as_bool().unwrap_or(false) {
        log::warn!(
            "auth.test failed: {}",
            resp["error"].as_str().unwrap_or("unknown")
        );
        return None;
    }

    resp["user_id"].as_str().map(String::from)
}

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// Stop all Slack background tasks (Socket Mode + emoji poll + event animation).
pub async fn stop_all(state: &AppState) {
    stop_socket_mode(state).await;
    stop_emoji_poll(state).await;
    if let Some(h) = state.inner.event_animation_handle.lock().await.take() {
        h.abort();
    }
    state
        .inner
        .event_animation_active
        .store(false, Ordering::SeqCst);
    state
        .inner
        .socket_mode_connected
        .store(false, Ordering::SeqCst);
    let mut slack = state.inner.slack.lock().await;
    slack.enabled = false;
}
