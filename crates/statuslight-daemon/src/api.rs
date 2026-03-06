//! HTTP API route handlers for the StatusLight daemon.

use std::sync::atomic::Ordering;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use statuslight_core::{Color, DeviceRegistry, Preset, StatusLightError};

use crate::slack;
use crate::state::AppState;

/// Build the axum router with all routes.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/color", post(post_color))
        .route("/rgb", post(post_rgb))
        .route("/off", post(post_off))
        .route("/brightness", post(post_brightness))
        .route("/presets", get(get_presets))
        .route("/devices", get(get_devices))
        .route("/device-color", get(get_device_color))
        .route("/button-status", get(get_button_status))
        .route("/slack/status", get(get_slack_status))
        .route("/slack/configure", post(post_slack_configure))
        .route("/slack/enable", post(post_slack_enable))
        .route("/slack/disable", post(post_slack_disable))
        .with_state(state)
}

// --- Response / Request types ---

#[derive(Serialize)]
struct ColorResponse {
    r: u8,
    g: u8,
    b: u8,
    hex: String,
}

impl From<Color> for ColorResponse {
    fn from(c: Color) -> Self {
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
            hex: c.to_hex(),
        }
    }
}

#[derive(Serialize)]
struct StatusResponse {
    device_connected: bool,
    device_count: usize,
    current_color: Option<ColorResponse>,
    brightness: u8,
    slack_sync_enabled: bool,
}

#[derive(Serialize)]
struct SetColorResponse {
    color: ColorResponse,
}

#[derive(Deserialize)]
struct ColorRequest {
    color: String,
}

#[derive(Deserialize)]
struct RgbRequest {
    r: u8,
    g: u8,
    b: u8,
}

#[derive(Deserialize)]
struct DeviceQuery {
    device: Option<String>,
}

#[derive(Deserialize)]
struct BrightnessRequest {
    brightness: u8,
}

#[derive(Serialize)]
struct BrightnessResponse {
    brightness: u8,
}

#[derive(Serialize)]
struct PresetEntry {
    name: &'static str,
    hex: String,
}

#[derive(Serialize)]
struct DeviceEntry {
    path: String,
    serial: Option<String>,
    manufacturer: Option<String>,
    product: Option<String>,
    vid: String,
    pid: String,
    driver: String,
}

#[derive(Serialize)]
struct DeviceColorResponse {
    device_color: Option<ColorResponse>,
    supports_readback: bool,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize)]
struct SlackStatusResponse {
    enabled: bool,
    has_app_token: bool,
    has_bot_token: bool,
    has_user_token: bool,
    socket_connected: bool,
    rules_count: usize,
    emoji_map: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct SlackConfigureRequest {
    app_token: Option<String>,
    bot_token: Option<String>,
    user_token: Option<String>,
    emoji_colors: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize)]
struct SlackConfigureResponse {
    enabled: bool,
}

#[derive(Serialize)]
struct SlackEnableResponse {
    enabled: bool,
}

#[derive(Serialize)]
struct ButtonStatusResponse {
    detected_preset: Option<String>,
    device_color: Option<ColorResponse>,
    polling_enabled: bool,
    slack_sync: bool,
}

// --- Error mapping ---

fn map_error(e: StatusLightError) -> (StatusCode, Json<ErrorResponse>) {
    let status = match &e {
        StatusLightError::DeviceNotFound | StatusLightError::UnknownDriver(_) => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        StatusLightError::MultipleDevices { .. } => StatusCode::SERVICE_UNAVAILABLE,
        StatusLightError::InvalidHexColor(_)
        | StatusLightError::UnknownPreset(_)
        | StatusLightError::DuplicatePreset(_)
        | StatusLightError::PresetNotFound(_) => StatusCode::BAD_REQUEST,
        StatusLightError::Hid(_)
        | StatusLightError::WriteMismatch { .. }
        | StatusLightError::ReadTimeout
        | StatusLightError::UnexpectedResponse => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(ErrorResponse {
            error: e.to_string(),
        }),
    )
}

// --- Helper: try to set color on devices ---

async fn try_set_color(
    state: &AppState,
    color: Color,
    device_serial: Option<&str>,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    // Apply brightness.
    let brightness = state.inner.brightness.load(Ordering::SeqCst);
    let scaled_color = color.scale_brightness(brightness as f64 / 100.0);

    let mut devices_guard = state.inner.devices.lock().await;

    // Try to reconnect if no devices are held.
    if devices_guard.is_empty() {
        drop(devices_guard);
        let dev = tokio::task::spawn_blocking(|| DeviceRegistry::with_builtins().open_any())
            .await
            .map_err(|_| map_error(StatusLightError::DeviceNotFound))?
            .map_err(map_error)?;
        devices_guard = state.inner.devices.lock().await;
        // Re-check after re-acquiring lock (another request may have reconnected).
        if devices_guard.is_empty() {
            devices_guard.push(dev);
        }
    }

    // Track indices of failed devices for removal.
    let mut failed = Vec::new();

    if let Some(serial) = device_serial {
        // Target a specific device by serial.
        let idx = devices_guard
            .iter()
            .position(|d| d.serial() == Some(serial));
        match idx {
            Some(i) => {
                if let Err(e) = devices_guard[i].set_color(scaled_color) {
                    log::warn!("Device serial={serial} failed: {e}");
                    failed.push(i);
                }
            }
            None => {
                return Err(map_error(StatusLightError::DeviceNotFound));
            }
        }
    } else {
        // Broadcast to all devices.
        for (i, dev) in devices_guard.iter().enumerate() {
            if let Err(e) = dev.set_color(scaled_color) {
                log::warn!("Device {} failed: {e}", dev.driver_name());
                failed.push(i);
            }
        }
    }

    // Remove failed devices (in reverse order to preserve indices).
    for &i in failed.iter().rev() {
        devices_guard.remove(i);
    }

    drop(devices_guard);
    *state.inner.current_color.lock().await = Some(color);

    // Mark manual override so background Slack polling doesn't overwrite.
    state.inner.manual_override.store(true, Ordering::SeqCst);

    Ok(())
}

// --- Route handlers ---

async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let devices_guard = state.inner.devices.lock().await;
    let device_count = devices_guard.len();
    let device_connected = device_count > 0;
    drop(devices_guard);

    let current_color = state
        .inner
        .current_color
        .lock()
        .await
        .map(ColorResponse::from);
    let slack_sync_enabled = state.inner.slack.lock().await.enabled;
    let brightness = state.inner.brightness.load(Ordering::SeqCst);

    Json(StatusResponse {
        device_connected,
        device_count,
        current_color,
        brightness,
        slack_sync_enabled,
    })
}

async fn post_color(
    State(state): State<AppState>,
    Query(query): Query<DeviceQuery>,
    Json(req): Json<ColorRequest>,
) -> Result<Json<SetColorResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Try as hex first, then as preset name (with config overrides), then custom presets.
    let color = if let Ok(c) = Color::from_hex(&req.color) {
        c
    } else {
        // Config::load() does synchronous file I/O; use spawn_blocking to avoid
        // blocking the async runtime (safe on any runtime flavor).
        let color_name = req.color.clone();
        let config = tokio::task::spawn_blocking(statuslight_core::Config::load)
            .await
            .unwrap_or_else(|e| {
                log::warn!("spawn_blocking panicked in post_color: {e}");
                Ok(statuslight_core::Config::default())
            })
            .unwrap_or_else(|e| {
                log::warn!("Config::load failed in post_color: {e}");
                statuslight_core::Config::default()
            });
        // Check built-in presets with color overrides.
        Preset::from_name(&color_name)
            .map(|p| p.color_with_overrides(&config.colors))
            .or_else(|_| {
                // Check custom presets.
                config
                    .custom_presets
                    .iter()
                    .find(|cp| cp.name == color_name)
                    .map(|cp| Color::from_hex(&cp.color))
                    .transpose()?
                    .ok_or_else(|| StatusLightError::UnknownPreset(color_name.clone()))
            })
            .map_err(map_error)?
    };

    try_set_color(&state, color, query.device.as_deref()).await?;
    Ok(Json(SetColorResponse {
        color: color.into(),
    }))
}

async fn post_rgb(
    State(state): State<AppState>,
    Query(query): Query<DeviceQuery>,
    Json(req): Json<RgbRequest>,
) -> Result<Json<SetColorResponse>, (StatusCode, Json<ErrorResponse>)> {
    let color = Color::new(req.r, req.g, req.b);
    try_set_color(&state, color, query.device.as_deref()).await?;
    Ok(Json(SetColorResponse {
        color: color.into(),
    }))
}

async fn post_off(
    State(state): State<AppState>,
    Query(query): Query<DeviceQuery>,
) -> Result<Json<SetColorResponse>, (StatusCode, Json<ErrorResponse>)> {
    let color = Color::off();
    try_set_color(&state, color, query.device.as_deref()).await?;
    Ok(Json(SetColorResponse {
        color: color.into(),
    }))
}

async fn post_brightness(
    State(state): State<AppState>,
    Json(req): Json<BrightnessRequest>,
) -> impl IntoResponse {
    let brightness = req.brightness.min(100);
    state.inner.brightness.store(brightness, Ordering::SeqCst);
    Json(BrightnessResponse { brightness })
}

async fn get_presets() -> impl IntoResponse {
    let presets: Vec<PresetEntry> = Preset::all()
        .iter()
        .map(|p| PresetEntry {
            name: p.name(),
            hex: p.color().to_hex(),
        })
        .collect();
    Json(presets)
}

async fn get_devices() -> Result<Json<Vec<DeviceEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let all = tokio::task::spawn_blocking(|| {
        let registry = DeviceRegistry::with_builtins();
        registry.enumerate_all()
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("Device enumeration task panicked: {e}");
        Vec::new()
    });
    let entries: Vec<DeviceEntry> = all
        .into_iter()
        .map(|(_, d)| DeviceEntry {
            path: d.path,
            serial: d.serial,
            manufacturer: d.manufacturer,
            product: d.product,
            vid: format!("0x{:04x}", d.vid),
            pid: format!("0x{:04x}", d.pid),
            driver: d.driver_id,
        })
        .collect();
    Ok(Json(entries))
}

async fn get_device_color(
    State(state): State<AppState>,
    Query(query): Query<DeviceQuery>,
) -> Result<Json<DeviceColorResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut devices_guard = state.inner.devices.lock().await;

    // Reconnect if no devices are held.
    if devices_guard.is_empty() {
        drop(devices_guard);
        let dev = tokio::task::spawn_blocking(|| DeviceRegistry::with_builtins().open_any())
            .await
            .map_err(|_| map_error(StatusLightError::DeviceNotFound))?
            .map_err(map_error)?;
        devices_guard = state.inner.devices.lock().await;
        // Re-check after re-acquiring lock (another request may have reconnected).
        if devices_guard.is_empty() {
            devices_guard.push(dev);
        }
    }

    // Find the target device.
    let idx = if let Some(ref serial) = query.device {
        devices_guard
            .iter()
            .position(|d| d.serial() == Some(serial.as_str()))
            .ok_or_else(|| map_error(StatusLightError::DeviceNotFound))?
    } else {
        0
    };

    match devices_guard[idx].get_color() {
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

async fn get_button_status(State(state): State<AppState>) -> impl IntoResponse {
    let current = *state.inner.current_color.lock().await;
    let config = tokio::task::spawn_blocking(statuslight_core::Config::load)
        .await
        .unwrap_or_else(|e| {
            log::warn!("spawn_blocking panicked: {e}");
            Ok(statuslight_core::Config::default())
        })
        .unwrap_or_else(|e| {
            log::warn!("Config::load failed: {e}");
            statuslight_core::Config::default()
        });

    let detected_preset = current
        .and_then(statuslight_core::protocol::button_cycle_preset)
        .map(String::from);

    Json(ButtonStatusResponse {
        detected_preset,
        device_color: current.map(ColorResponse::from),
        polling_enabled: config.button.enabled,
        slack_sync: config.button.slack_sync,
    })
}

async fn get_slack_status(State(state): State<AppState>) -> impl IntoResponse {
    let slack = state.inner.slack.lock().await;
    let connected = state
        .inner
        .socket_mode_connected
        .load(std::sync::atomic::Ordering::SeqCst);
    Json(SlackStatusResponse {
        enabled: slack.enabled,
        has_app_token: slack.app_token.is_some(),
        has_bot_token: slack.bot_token.is_some(),
        has_user_token: slack.user_token.is_some(),
        socket_connected: connected,
        rules_count: slack.rules.len(),
        emoji_map: slack.emoji_colors.clone(),
    })
}

async fn post_slack_configure(
    State(state): State<AppState>,
    Json(req): Json<SlackConfigureRequest>,
) -> Result<Json<SlackConfigureResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Validate tokens before storing.
    let client = state.inner.http_client.clone();
    if let Some(ref token) = req.user_token {
        let resp: serde_json::Value = client
            .post("https://slack.com/api/auth.test")
            .bearer_auth(token)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("failed to validate user token: {e}"),
                    }),
                )
            })?
            .json()
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("failed to parse auth.test response: {e}"),
                    }),
                )
            })?;
        if !resp["ok"].as_bool().unwrap_or(false) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!(
                        "invalid user token: {}",
                        resp["error"].as_str().unwrap_or("unknown error")
                    ),
                }),
            ));
        }
    }
    if let Some(ref token) = req.bot_token {
        let resp: serde_json::Value = client
            .post("https://slack.com/api/auth.test")
            .bearer_auth(token)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("failed to validate bot token: {e}"),
                    }),
                )
            })?
            .json()
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("failed to parse auth.test response: {e}"),
                    }),
                )
            })?;
        if !resp["ok"].as_bool().unwrap_or(false) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!(
                        "invalid bot token: {}",
                        resp["error"].as_str().unwrap_or("unknown error")
                    ),
                }),
            ));
        }
    }
    if let Some(ref token) = req.app_token {
        let resp: serde_json::Value = client
            .post("https://slack.com/api/apps.connections.open")
            .bearer_auth(token)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send()
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("failed to validate app token: {e}"),
                    }),
                )
            })?
            .json()
            .await
            .map_err(|e| {
                (
                    StatusCode::BAD_GATEWAY,
                    Json(ErrorResponse {
                        error: format!("failed to parse connections.open response: {e}"),
                    }),
                )
            })?;
        if !resp["ok"].as_bool().unwrap_or(false) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!(
                        "invalid app token: {}",
                        resp["error"].as_str().unwrap_or("unknown error")
                    ),
                }),
            ));
        }
    }

    // Persist tokens to the encrypted store (if any were provided).
    // Uses spawn_blocking because token I/O is synchronous.
    {
        let mut tokens_to_store = std::collections::HashMap::new();
        if let Some(ref token) = req.app_token {
            tokens_to_store.insert("slack_app_token".to_string(), token.clone());
        }
        if let Some(ref token) = req.bot_token {
            tokens_to_store.insert("slack_bot_token".to_string(), token.clone());
        }
        if let Some(ref token) = req.user_token {
            tokens_to_store.insert("slack_user_token".to_string(), token.clone());
        }
        if !tokens_to_store.is_empty() {
            let store_result = tokio::task::spawn_blocking(move || {
                crate::token_store::store_tokens(&tokens_to_store)
            })
            .await;
            match store_result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("failed to persist tokens: {e}"),
                        }),
                    ));
                }
                Err(e) => {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: format!("token store task panicked: {e}"),
                        }),
                    ));
                }
            }
        }
    }

    let mut slack_state = state.inner.slack.lock().await;

    if let Some(token) = req.app_token {
        slack_state.app_token = Some(token);
    }
    if let Some(token) = req.bot_token {
        slack_state.bot_token = Some(token);
    }
    if let Some(token) = req.user_token {
        slack_state.user_token = Some(token);
    }
    if let Some(emoji_colors) = req.emoji_colors {
        // Validate all hex values before accepting.
        for (emoji, hex) in &emoji_colors {
            Color::from_hex(hex).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: format!("invalid hex color for {emoji}: {hex}"),
                    }),
                )
            })?;
        }
        slack_state.emoji_colors = emoji_colors;
    }

    let was_enabled = slack_state.enabled;
    // If already running, restart with new config.
    if was_enabled {
        drop(slack_state);
        slack::stop_all(&state).await;

        // Re-resolve user_id with (potentially new) token.
        if let Some(uid) = slack::resolve_user_id(&state).await {
            state.inner.slack.lock().await.user_id = Some(uid);
        }

        // Re-start based on available tokens.
        let slack_state = state.inner.slack.lock().await;
        let has_app = slack_state.app_token.is_some();
        let has_user = slack_state.user_token.is_some();
        drop(slack_state);
        if has_app {
            slack::start_socket_mode(&state).await;
        }
        if has_user {
            slack::start_emoji_poll(&state).await;
        }
        state.inner.slack.lock().await.enabled = true;
    } else {
        drop(slack_state);
    }

    // Return the known state: true if we just restarted, false if not yet enabled.
    Ok(Json(SlackConfigureResponse {
        enabled: was_enabled,
    }))
}

async fn post_slack_enable(
    State(state): State<AppState>,
) -> Result<Json<SlackEnableResponse>, (StatusCode, Json<ErrorResponse>)> {
    let slack_state = state.inner.slack.lock().await;
    let has_app = slack_state.app_token.is_some();
    let has_user = slack_state.user_token.is_some();
    drop(slack_state);

    if !has_app && !has_user {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "no Slack tokens configured".to_string(),
            }),
        ));
    }

    // Clear manual override when Slack is (re-)enabled.
    state
        .inner
        .manual_override
        .store(false, std::sync::atomic::Ordering::SeqCst);

    if has_app {
        slack::start_socket_mode(&state).await;
    }
    if has_user {
        slack::start_emoji_poll(&state).await;
    }
    state.inner.slack.lock().await.enabled = true;

    Ok(Json(SlackEnableResponse { enabled: true }))
}

async fn post_slack_disable(State(state): State<AppState>) -> impl IntoResponse {
    slack::stop_all(&state).await;
    Json(SlackEnableResponse { enabled: false })
}
