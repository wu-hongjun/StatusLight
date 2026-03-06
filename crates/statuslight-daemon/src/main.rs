mod api;
mod slack;
mod state;
mod update;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::net::UnixListener;

use state::AppState;

#[derive(Parser)]
#[command(
    name = "slickyd",
    version,
    about = "HTTP daemon for Slicky USB status lights"
)]
struct Args {
    /// Path to the Unix domain socket.
    #[arg(long, default_value = "/tmp/slicky.sock")]
    socket: PathBuf,

    /// Slack app-level token for Socket Mode (overrides config).
    #[arg(long)]
    slack_app_token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();

    // Remove stale socket file if it exists.
    if args.socket.exists() {
        std::fs::remove_file(&args.socket)
            .with_context(|| format!("failed to remove stale socket: {}", args.socket.display()))?;
    }

    let state = AppState::new();

    // Try to open the device at startup (non-fatal if not found).
    match slicky_core::HidSlickyDevice::open() {
        Ok(dev) => {
            log::info!("Slicky device found at startup");
            *state.inner.device.lock().await = Some(dev);
        }
        Err(e) => {
            log::warn!("No device at startup (will retry on requests): {e}");
        }
    }

    // Load config.
    let config = slicky_core::Config::load().unwrap_or_else(|e| {
        log::warn!("Failed to load config, using defaults: {e}");
        slicky_core::Config::default()
    });

    // Configure Slack state from config (CLI arg overrides config for app_token).
    let app_token = args.slack_app_token.or(config.slack.app_token);
    let bot_token = config.slack.bot_token;
    let user_token = config.slack.user_token;

    // Build emoji_colors map — config stores hex strings directly.
    let emoji_colors = config.slack.emoji_colors;

    {
        let mut slack_state = state.inner.slack.lock().await;
        slack_state.app_token = app_token.clone();
        slack_state.bot_token = bot_token;
        slack_state.user_token = user_token.clone();
        slack_state.emoji_colors = emoji_colors;
        slack_state.rules = config.slack.rules;
    }

    // Resolve the authenticated user's Slack ID (needed for user_change filtering).
    if let Some(uid) = slack::resolve_user_id(&state).await {
        log::info!("Resolved Slack user ID: {uid}");
        state.inner.slack.lock().await.user_id = Some(uid);
    }

    // Start Slack tasks based on available tokens.
    let has_app = app_token.is_some();
    let has_user = user_token.is_some();

    if has_app {
        slack::start_socket_mode(&state).await;
        log::info!("Socket Mode enabled");
    }
    if has_user {
        slack::start_emoji_poll(&state).await;
        log::info!("Emoji status polling enabled (60s interval)");
    }
    if has_app || has_user {
        state.inner.slack.lock().await.enabled = true;
    }

    // Spawn a non-blocking update check on startup.
    update::spawn_check_if_due();

    let app = api::router(state.clone());

    let listener = UnixListener::bind(&args.socket)
        .with_context(|| format!("failed to bind socket: {}", args.socket.display()))?;
    log::info!("Listening on {}", args.socket.display());

    // Graceful shutdown on SIGINT / SIGTERM.
    let shutdown_signal = async {
        let ctrl_c = tokio::signal::ctrl_c();
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = ctrl_c => { log::info!("Received SIGINT, shutting down"); }
            _ = sigterm.recv() => { log::info!("Received SIGTERM, shutting down"); }
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .context("server error")?;

    // Clean up socket file.
    let _ = std::fs::remove_file(&args.socket);

    // Stop all Slack tasks.
    slack::stop_all(&state).await;

    log::info!("Daemon stopped");
    Ok(())
}
