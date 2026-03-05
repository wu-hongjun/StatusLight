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
    poll_interval_secs: u64,
    has_token: bool,
    emoji_map: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct SlackConfigureRequest {
    token: Option<String>,
    poll_interval_secs: Option<u64>,
    emoji_map: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize)]
struct SlackConfigureResponse {
    enabled: bool,
    poll_interval_secs: u64,
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
    // Try as hex first, then as preset name.
    let color = Color::from_hex(&req.color)
        .or_else(|_| Preset::from_name(&req.color).map(|p| p.color()))
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
    let emoji_map: std::collections::HashMap<String, String> = slack
        .emoji_map
        .iter()
        .map(|(k, v)| (k.clone(), v.to_hex()))
        .collect();
    Json(SlackStatusResponse {
        enabled: slack.enabled,
        poll_interval_secs: slack.poll_interval_secs,
        has_token: slack.token.is_some(),
        emoji_map,
    })
}

async fn post_slack_configure(
    State(state): State<AppState>,
    Json(req): Json<SlackConfigureRequest>,
) -> Result<Json<SlackConfigureResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut slack = state.inner.slack.lock().await;

    if let Some(token) = req.token {
        slack.token = Some(token);
    }
    if let Some(interval) = req.poll_interval_secs {
        slack.poll_interval_secs = interval;
    }
    if let Some(emoji_map) = req.emoji_map {
        let mut map = std::collections::HashMap::new();
        for (emoji, color_str) in emoji_map {
            let color = Color::from_hex(&color_str).map_err(map_slicky_error)?;
            map.insert(emoji, color);
        }
        slack.emoji_map = map;
    }

    let response = SlackConfigureResponse {
        enabled: slack.enabled,
        poll_interval_secs: slack.poll_interval_secs,
    };

    // If polling was already running, restart with new config.
    if slack.enabled {
        drop(slack);
        slack::stop_polling(&state).await;
        slack::start_polling(&state).await;
    }

    Ok(Json(response))
}

async fn post_slack_enable(
    State(state): State<AppState>,
) -> Result<Json<SlackEnableResponse>, (StatusCode, Json<ErrorResponse>)> {
    let has_token = state.inner.slack.lock().await.token.is_some();
    if !has_token {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "no Slack token configured".to_string(),
            }),
        ));
    }

    slack::start_polling(&state).await;
    Ok(Json(SlackEnableResponse { enabled: true }))
}

async fn post_slack_disable(State(state): State<AppState>) -> impl IntoResponse {
    slack::stop_polling(&state).await;
    Json(SlackEnableResponse { enabled: false })
}
