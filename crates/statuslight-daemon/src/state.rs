//! Shared application state for the Slicky daemon.

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use slicky_core::{Color, HidSlickyDevice, SlackRule};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Shared state passed to all axum route handlers.
#[derive(Clone)]
pub struct AppState {
    pub inner: Arc<AppStateInner>,
}

/// The inner state protected by `Arc`.
pub struct AppStateInner {
    /// The HID device handle. `Option` allows starting without a connected device.
    /// `Mutex` because `HidDevice` is `Send` but not `Sync`.
    pub device: Mutex<Option<HidSlickyDevice>>,
    /// The last color set on the device.
    pub current_color: Mutex<Option<Color>>,
    /// Slack integration state.
    pub slack: Mutex<SlackState>,
    /// When true, the emoji poller should not overwrite the device color.
    pub event_animation_active: AtomicBool,
    /// Handle to the currently running event animation task, if any.
    pub event_animation_handle: Mutex<Option<JoinHandle<()>>>,
    /// When true, background Slack polling should not overwrite the device color.
    /// Set by manual API color changes, cleared by Slack-driven color changes.
    pub manual_override: AtomicBool,
    /// Whether the Socket Mode WebSocket is currently connected.
    pub socket_mode_connected: AtomicBool,
}

/// Slack configuration and runtime state.
pub struct SlackState {
    /// Whether Slack integration is active.
    pub enabled: bool,
    /// App-level token (`xapp-...`) for Socket Mode.
    pub app_token: Option<String>,
    /// Bot token (`xoxb-...`) for API calls.
    pub bot_token: Option<String>,
    /// User token (`xoxp-...`) for profile read/write.
    pub user_token: Option<String>,
    /// Emoji-to-color mappings (emoji → hex color string).
    pub emoji_colors: HashMap<String, String>,
    /// The authenticated user's Slack ID (for filtering `user_change` events).
    pub user_id: Option<String>,
    /// Event-driven animation rules.
    pub rules: Vec<SlackRule>,
    /// Handle to the Socket Mode WebSocket task, if running.
    pub socket_handle: Option<JoinHandle<()>>,
    /// Handle to the background emoji polling task, if running.
    pub emoji_poll_handle: Option<JoinHandle<()>>,
}

impl AppState {
    /// Create a new `AppState` with no device connected and Slack disabled.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                device: Mutex::new(None),
                current_color: Mutex::new(None),
                slack: Mutex::new(SlackState {
                    enabled: false,
                    app_token: None,
                    bot_token: None,
                    user_token: None,
                    emoji_colors: HashMap::new(),
                    user_id: None,
                    rules: Vec::new(),
                    socket_handle: None,
                    emoji_poll_handle: None,
                }),
                event_animation_active: AtomicBool::new(false),
                event_animation_handle: Mutex::new(None),
                manual_override: AtomicBool::new(false),
                socket_mode_connected: AtomicBool::new(false),
            }),
        }
    }
}
