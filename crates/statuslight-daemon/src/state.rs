//! Shared application state for the StatusLight daemon.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8};
use std::sync::Arc;

use statuslight_core::{Color, SlackRule, StatusLightDevice};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Shared state passed to all axum route handlers.
#[derive(Clone)]
pub struct AppState {
    pub(crate) inner: Arc<AppStateInner>,
}

/// The inner state protected by `Arc`.
pub(crate) struct AppStateInner {
    /// All open device handles. Empty if no devices are connected.
    /// `Mutex` because `HidDevice` is `Send` but not `Sync`.
    pub(crate) devices: Mutex<Vec<Box<dyn StatusLightDevice>>>,
    /// The last color set on the device.
    pub(crate) current_color: Mutex<Option<Color>>,
    /// Global brightness level (0–100).
    pub(crate) brightness: AtomicU8,
    /// Slack integration state.
    pub(crate) slack: Mutex<SlackState>,
    /// When true, the emoji poller should not overwrite the device color.
    pub(crate) event_animation_active: AtomicBool,
    /// Handle to the currently running event animation task, if any.
    pub(crate) event_animation_handle: Mutex<Option<JoinHandle<()>>>,
    /// When true, background Slack polling should not overwrite the device color.
    /// Set by manual API color changes, cleared by Slack-driven color changes.
    pub(crate) manual_override: AtomicBool,
    /// Whether the Socket Mode WebSocket is currently connected.
    pub(crate) socket_mode_connected: AtomicBool,
    /// Shared HTTP client for all outbound requests (Slack API, etc.).
    pub(crate) http_client: reqwest::Client,
    /// Handle to the button polling task, if running.
    pub(crate) button_poll_handle: Mutex<Option<JoinHandle<()>>>,
}

/// Slack configuration and runtime state.
pub(crate) struct SlackState {
    /// Whether Slack integration is active.
    pub(crate) enabled: bool,
    /// App-level token (`xapp-...`) for Socket Mode.
    pub(crate) app_token: Option<String>,
    /// Bot token (`xoxb-...`) for API calls.
    pub(crate) bot_token: Option<String>,
    /// User token (`xoxp-...`) for profile read/write.
    pub(crate) user_token: Option<String>,
    /// Emoji-to-color mappings (emoji → hex color string).
    pub(crate) emoji_colors: HashMap<String, String>,
    /// The authenticated user's Slack ID (for filtering `user_change` events).
    pub(crate) user_id: Option<String>,
    /// Event-driven animation rules.
    pub(crate) rules: Vec<SlackRule>,
    /// Handle to the Socket Mode WebSocket task, if running.
    pub(crate) socket_handle: Option<JoinHandle<()>>,
    /// Handle to the background emoji polling task, if running.
    pub(crate) emoji_poll_handle: Option<JoinHandle<()>>,
}

impl AppState {
    /// Create a new `AppState` with no device connected and Slack disabled.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AppStateInner {
                devices: Mutex::new(Vec::new()),
                current_color: Mutex::new(None),
                brightness: AtomicU8::new(100),
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
                http_client: reqwest::Client::new(),
                button_poll_handle: Mutex::new(None),
            }),
        }
    }
}
