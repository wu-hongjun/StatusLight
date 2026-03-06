//! HTTP API route handlers for the Slicky daemon.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use slicky_core::{Color, HidSlickyDevice, Preset, SlickyDevice, SlickyError};

use crate::slack;
use crate::state::AppState;

/// Build the axum router with all routes.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(get_status))
        .route("/color", post(post_color))
        .route("/rgb", post(post_rgb))
        .route("/off", post(post_off))
        .route("/presets", get(get_presets))
        .route("/devices", get(get_devices))
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
    current_color: Option<ColorResponse>,
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

// --- Error mapping ---

fn map_slicky_error(e: SlickyError) -> (StatusCode, Json<ErrorResponse>) {
    let status = match &e {
        SlickyError::DeviceNotFound => StatusCode::SERVICE_UNAVAILABLE,
        SlickyError::MultipleDevices { .. } => StatusCode::SERVICE_UNAVAILABLE,
        SlickyError::InvalidHexColor(_)
        | SlickyError::UnknownPreset(_)
        | SlickyError::DuplicatePreset(_)
        | SlickyError::PresetNotFound(_) => StatusCode::BAD_REQUEST,
        SlickyError::Hid(_) | SlickyError::WriteMismatch { .. } => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    };
    (
        status,
        Json(ErrorResponse {
            error: e.to_string(),
        }),
    )
}

// --- Helper: try to set color on the device ---

async fn try_set_color(
    state: &AppState,
    color: Color,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    let mut device_guard = state.inner.device.lock().await;

    // Try to reconnect if no device is held.
    if device_guard.is_none() {
        match HidSlickyDevice::open() {
            Ok(dev) => *device_guard = Some(dev),
            Err(e) => return Err(map_slicky_error(e)),
        }
    }

    let dev = device_guard
        .as_ref()
        .ok_or_else(|| map_slicky_error(SlickyError::DeviceNotFound))?;

    if let Err(e) = dev.set_color(color) {
        // Device may have been disconnected — drop it so we reconnect next time.
        *device_guard = None;
        return Err(map_slicky_error(e));
    }

    drop(device_guard);
    *state.inner.current_color.lock().await = Some(color);

    // Mark manual override so background Slack polling doesn't overwrite.
    state
        .inner
        .manual_override
        .store(true, std::sync::atomic::Ordering::SeqCst);

    Ok(())
}

// --- Route handlers ---

async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let device_guard = state.inner.device.lock().await;
    let device_connected = device_guard.is_some();
    drop(device_guard);

    let current_color = state
        .inner
        .current_color
        .lock()
        .await
        .map(ColorResponse::from);
    let slack_sync_enabled = state.inner.slack.lock().await.enabled;

    Json(StatusResponse {
        device_connected,
        current_color,
        slack_sync_enabled,
    })
}

async fn post_color(
    State(state): State<AppState>,
    Json(req): Json<ColorRequest>,
) -> Result<Json<SetColorResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Try as hex first, then as preset name (with config overrides), then custom presets.
    let color = Color::from_hex(&req.color)
        .or_else(|_| {
            let config = slicky_core::Config::load().unwrap_or_default();
            // Check built-in presets with color overrides.
            Preset::from_name(&req.color)
                .map(|p| p.color_with_overrides(&config.colors))
                .or_else(|_| {
                    // Check custom presets.
                    config
                        .custom_presets
                        .iter()
                        .find(|cp| cp.name == req.color)
                        .map(|cp| Color::from_hex(&cp.color))
                        .transpose()?
                        .ok_or_else(|| SlickyError::UnknownPreset(req.color.clone()))
                })
        })
        .map_err(map_slicky_error)?;

    try_set_color(&state, color).await?;
    Ok(Json(SetColorResponse {
        color: color.into(),
    }))
}

async fn post_rgb(
    State(state): State<AppState>,
    Json(req): Json<RgbRequest>,
) -> Result<Json<SetColorResponse>, (StatusCode, Json<ErrorResponse>)> {
    let color = Color::new(req.r, req.g, req.b);
    try_set_color(&state, color).await?;
    Ok(Json(SetColorResponse {
        color: color.into(),
    }))
}

async fn post_off(
    State(state): State<AppState>,
) -> Result<Json<SetColorResponse>, (StatusCode, Json<ErrorResponse>)> {
    let color = Color::off();
    try_set_color(&state, color).await?;
    Ok(Json(SetColorResponse {
        color: color.into(),
    }))
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
    let devices = HidSlickyDevice::enumerate().map_err(map_slicky_error)?;
    let entries: Vec<DeviceEntry> = devices
        .into_iter()
        .map(|d| DeviceEntry {
            path: d.path,
            serial: d.serial,
            manufacturer: d.manufacturer,
            product: d.product,
        })
        .collect();
    Ok(Json(entries))
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
    let client = reqwest::Client::new();
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

    let response = SlackConfigureResponse {
        enabled: slack_state.enabled,
    };

    // If already running, restart with new config.
    if slack_state.enabled {
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
    }

    Ok(Json(response))
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
