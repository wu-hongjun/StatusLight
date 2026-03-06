//! Slack integration for the daemon: Socket Mode client, rule matcher,
//! emoji status poller, and event animation runner.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use slicky_core::{AnimationType, Color, HidSlickyDevice, SlackRule, SlickyDevice};
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
pub async fn start_socket_mode(state: &AppState) {
    stop_socket_mode(state).await;

    let mut slack = state.inner.slack.lock().await;
    let app_token = match &slack.app_token {
        Some(t) => t.clone(),
        None => return,
    };
    let rules = slack.rules.clone();
    let state_clone = state.clone();

    let handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut backoff = Duration::from_secs(1);
        let max_backoff = Duration::from_secs(60);

        loop {
            match run_socket_loop(&client, &app_token, &rules, &state_clone).await {
                Ok(()) => {
                    log::info!("Socket Mode connection closed normally");
                    backoff = Duration::from_secs(1);
                }
                Err(e) => {
                    log::warn!("Socket Mode error: {e}, reconnecting in {backoff:?}");
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(max_backoff);
        }
    });

    slack.socket_handle = Some(handle);
}

/// Run a single Socket Mode WebSocket session. Returns when the connection drops.
async fn run_socket_loop(
    client: &reqwest::Client,
    app_token: &str,
    rules: &[SlackRule],
    state: &AppState,
) -> anyhow::Result<()> {
    let url = get_ws_url(client, app_token).await?;
    // Log only scheme+host — the URL contains a session ticket.
    let redacted = url.split('?').next().unwrap_or("wss://...");
    log::info!("Socket Mode connecting to {redacted}");

    let (ws_stream, _) = tokio_tungstenite::connect_async(&url).await?;
    let (mut write, mut read) = ws_stream.split();
    log::info!("Socket Mode connected");

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

                    // Dispatch event.
                    if envelope["type"].as_str() == Some("events_api") {
                        if let Some(event) = envelope.get("payload").and_then(|p| p.get("event")) {
                            handle_event(event, rules, state).await;
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
async fn handle_event(event: &serde_json::Value, rules: &[SlackRule], state: &AppState) {
    let raw_type = event["type"].as_str().unwrap_or("");
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

    for rule in rules {
        if rule_matches(rule, &event_type, event) {
            log::info!("Rule '{}' matched event '{event_type}'", rule.name);
            trigger_animation(rule, state).await;
            return;
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
        while start.elapsed().as_secs_f64() < duration {
            let frame = anim.frame(start.elapsed().as_secs_f64(), speed, &[color]);
            set_device_color(&state_clone, frame).await;
            tokio::time::sleep(Duration::from_millis(33)).await;
        }

        // Restore previous color.
        state_clone
            .inner
            .event_animation_active
            .store(false, Ordering::SeqCst);
        if let Some(c) = prev_color {
            set_device_color(&state_clone, c).await;
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
/// the `emoji_colors` config. Skips when an event animation is active.
pub async fn start_emoji_poll(state: &AppState) {
    stop_emoji_poll(state).await;

    let mut slack = state.inner.slack.lock().await;
    let user_token = match &slack.user_token {
        Some(t) => t.clone(),
        None => return,
    };
    let emoji_colors = slack.emoji_colors.clone();
    let state_clone = state.clone();

    let handle = tokio::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            // Skip if an event animation is playing.
            if !state_clone
                .inner
                .event_animation_active
                .load(Ordering::SeqCst)
            {
                match fetch_emoji_color(&client, &user_token, &emoji_colors).await {
                    Ok(Some(color)) => {
                        set_device_color(&state_clone, color).await;
                    }
                    Ok(None) => {
                        log::debug!("Emoji poll: no matching status emoji");
                    }
                    Err(e) => {
                        log::warn!("Emoji poll error: {e}");
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

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
    let mut slack = state.inner.slack.lock().await;
    slack.enabled = false;
}
