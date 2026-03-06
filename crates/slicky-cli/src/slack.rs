//! Slack setup/disconnect/status for the CLI.
//!
//! Uses a per-user Slack app model: users create their own app from a manifest
//! and paste three tokens (app, bot, user) via a guided wizard.

use std::io::{self, Write};

use anyhow::{bail, ensure, Context, Result};
use slicky_core::Config;

const HEX_CHARS: [u8; 16] = *b"0123456789ABCDEF";

/// Percent-encode a string per RFC 3986 (unreserved chars pass through).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX_CHARS[(b >> 4) as usize]));
                out.push(char::from(HEX_CHARS[(b & 0x0F) as usize]));
            }
        }
    }
    out
}

/// Build the Slack app-creation URL with the manifest pre-filled.
fn manifest_url() -> String {
    let manifest = serde_json::json!({
        "display_information": {
            "name": "Status Light",
            "description": "USB status light controller"
        },
        "features": {
            "bot_user": {
                "display_name": "Status Light",
                "always_online": false
            }
        },
        "oauth_config": {
            "scopes": {
                "user": [
                    "users.profile:read",
                    "users.profile:write",
                    "im:read",
                    "im:history"
                ],
                "bot": [
                    "im:read",
                    "im:history"
                ]
            }
        },
        "settings": {
            "socket_mode_enabled": true,
            "event_subscriptions": {
                "user_events": ["message.im"],
                "bot_events": ["app_mention"]
            },
            "org_deploy_enabled": false,
            "token_rotation_enabled": false
        }
    });
    let encoded = percent_encode(&manifest.to_string());
    format!("https://api.slack.com/apps?new_app=1&manifest_json={encoded}")
}

/// Prompt the user to press Enter to continue (accepts empty input).
fn prompt_continue(msg: &str) -> Result<()> {
    print!("{msg}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(())
}

/// `slicky slack open-setup` — open browser with pre-filled manifest (non-interactive).
pub fn open_setup() -> Result<()> {
    let url = manifest_url();
    open::that(&url).context("failed to open browser")?;
    Ok(())
}

/// `slicky slack configure` — non-interactive token setup (for macOS app).
///
/// Validates tokens and saves to config. Prints JSON result for machine parsing.
pub fn configure(app_token: &str, bot_token: &str, user_token: &str) -> Result<()> {
    ensure!(
        app_token.starts_with("xapp-"),
        "Expected xapp- prefix for app-level token"
    );
    ensure!(
        bot_token.starts_with("xoxb-"),
        "Expected xoxb- prefix for bot token"
    );
    ensure!(
        user_token.starts_with("xoxp-"),
        "Expected xoxp- prefix for user token"
    );

    validate_token(bot_token, "bot")?;
    validate_token(user_token, "user")?;
    validate_app_token(app_token)?;

    save_tokens(app_token, bot_token, user_token)?;

    println!("{{\"status\":\"configured\"}}");
    Ok(())
}

/// `slicky slack setup` — guided wizard to configure Slack tokens.
pub fn setup() -> Result<()> {
    println!("=== Status Light — Slack Setup ===\n");

    // Step 1: Open browser with pre-filled manifest.
    println!("Step 1: Create your Slack app");
    println!("  Opening Slack with a pre-filled app manifest...");
    let url = manifest_url();
    if open::that(&url).is_err() {
        println!("  Could not open browser. Visit this URL manually:");
        println!("  {url}");
    }
    println!();
    println!("  Select your workspace, review the permissions, and click Create.");
    prompt_continue("  Press Enter when done... ")?;

    // Step 2: App-level token.
    println!("\nStep 2: Generate an App-Level Token");
    println!("  In your app settings: Basic Information > App-Level Tokens");
    println!("  Click 'Generate Token and Scopes', add scope: connections:write");
    let app_token = prompt("  App-Level Token (xapp-...): ")?;
    ensure!(
        app_token.starts_with("xapp-"),
        "Expected xapp- prefix for app-level token"
    );

    // Step 3: Install and copy bot + user tokens.
    println!("\nStep 3: Install the app and copy tokens");
    println!("  Go to Install App > Install to Workspace");
    println!("  Then copy both tokens from the Install App page.");
    let bot_token = prompt("  Bot Token (xoxb-...): ")?;
    ensure!(
        bot_token.starts_with("xoxb-"),
        "Expected xoxb- prefix for bot token"
    );
    let user_token = prompt("  User Token (xoxp-...): ")?;
    ensure!(
        user_token.starts_with("xoxp-"),
        "Expected xoxp- prefix for user token"
    );

    // Step 4: Validate and save.
    println!("\nValidating tokens...");
    validate_token(&bot_token, "bot")?;
    validate_token(&user_token, "user")?;
    validate_app_token(&app_token)?;

    save_tokens(&app_token, &bot_token, &user_token)?;

    println!("\nSetup complete! Restart the daemon to enable Socket Mode.");
    Ok(())
}

/// Save tokens to config with default emoji colors and rules.
fn save_tokens(app_token: &str, bot_token: &str, user_token: &str) -> Result<()> {
    let mut config = Config::load()?;
    config.slack.app_token = Some(app_token.to_string());
    config.slack.bot_token = Some(bot_token.to_string());
    config.slack.user_token = Some(user_token.to_string());
    config.slack.events_enabled = true;

    // Populate default emoji_colors if empty.
    if config.slack.emoji_colors.is_empty() {
        config.slack.emoji_colors.extend(default_emoji_colors());
    }

    // Populate a default DM rule if empty.
    if config.slack.rules.is_empty() {
        config.slack.rules.push(slicky_core::SlackRule {
            name: "DM notification".to_string(),
            event: "message.im".to_string(),
            from_user: None,
            contains: None,
            animation: "flash".to_string(),
            color: "#00FF00".to_string(),
            speed: 2.0,
            repeat: 3,
            duration_secs: None,
        });
    }

    config.save()?;
    Ok(())
}

/// `slicky slack disconnect` — clear all Slack tokens.
pub fn disconnect() -> Result<()> {
    let mut config = Config::load()?;
    let had_tokens = config.slack.app_token.is_some()
        || config.slack.bot_token.is_some()
        || config.slack.user_token.is_some();

    if !had_tokens {
        println!("Slack: not connected.");
        return Ok(());
    }

    config.slack.app_token = None;
    config.slack.bot_token = None;
    config.slack.user_token = None;
    config.slack.events_enabled = false;
    config.save()?;
    println!("Slack tokens removed. Restart the daemon to take effect.");
    Ok(())
}

/// `slicky slack status` — show connection state.
pub fn status() -> Result<()> {
    let config = Config::load()?;

    let has_app = config.slack.app_token.is_some();
    let has_bot = config.slack.bot_token.is_some();
    let has_user = config.slack.user_token.is_some();

    if has_app || has_bot || has_user {
        println!("Slack: connected");
        println!(
            "  App token:  {}",
            if has_app { "configured" } else { "missing" }
        );
        println!(
            "  Bot token:  {}",
            if has_bot { "configured" } else { "missing" }
        );
        println!(
            "  User token: {}",
            if has_user { "configured" } else { "missing" }
        );
        println!(
            "  Events:     {}",
            if config.slack.events_enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
        println!("  Rules:      {}", config.slack.rules.len());
        println!("  Emoji map:  {} entries", config.slack.emoji_colors.len());
    } else {
        println!("Slack: not connected");
        println!("Run `slicky slack setup` to connect.");
    }
    Ok(())
}

/// `slicky slack set-status` — set Slack status text and emoji.
pub fn set_status(text: &str, emoji: &str) -> Result<()> {
    let config = Config::load()?;
    let token = config.slack.user_token.ok_or_else(|| {
        anyhow::anyhow!("not connected to Slack — run `slicky slack setup` first")
    })?;

    let body = serde_json::json!({
        "profile": {
            "status_text": text,
            "status_emoji": emoji,
            "status_expiration": 0
        }
    });

    let json_body = serde_json::to_string(&body).context("failed to serialize request")?;

    let resp = ureq::post("https://slack.com/api/users.profile.set")
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json; charset=utf-8")
        .send(json_body.as_bytes())
        .context("failed to set Slack status")?;

    let json: serde_json::Value = serde_json::from_reader(resp.into_body().into_reader())
        .context("failed to parse Slack response")?;

    if !json["ok"].as_bool().unwrap_or(false) {
        let err = json["error"].as_str().unwrap_or("unknown error");
        bail!("Slack API error: {err}");
    }

    if text.is_empty() {
        println!("Slack status cleared");
    } else {
        println!("Slack status set: {emoji} {text}");
    }
    Ok(())
}

/// `slicky slack clear-status` — clear Slack status.
pub fn clear_status() -> Result<()> {
    set_status("", "")
}

/// Prompt the user for input, returning the trimmed value.
fn prompt(msg: &str) -> Result<String> {
    print!("{msg}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        bail!("no input provided");
    }
    Ok(trimmed)
}

/// Validate a bot or user token via `auth.test`.
fn validate_token(token: &str, label: &str) -> Result<()> {
    let resp = ureq::post("https://slack.com/api/auth.test")
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(&[])
        .with_context(|| format!("failed to validate {label} token"))?;

    let json: serde_json::Value = serde_json::from_reader(resp.into_body().into_reader())
        .context("failed to parse auth.test response")?;

    if !json["ok"].as_bool().unwrap_or(false) {
        let err = json["error"].as_str().unwrap_or("unknown error");
        bail!("{label} token validation failed: {err}");
    }

    println!(
        "  {label} token valid (team: {})",
        json["team"].as_str().unwrap_or("?")
    );
    Ok(())
}

/// Validate an app-level token via `apps.connections.open`.
fn validate_app_token(token: &str) -> Result<()> {
    let resp = ureq::post("https://slack.com/api/apps.connections.open")
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send(&[])
        .context("failed to validate app token")?;

    let json: serde_json::Value = serde_json::from_reader(resp.into_body().into_reader())
        .context("failed to parse apps.connections.open response")?;

    if !json["ok"].as_bool().unwrap_or(false) {
        let err = json["error"].as_str().unwrap_or("unknown error");
        bail!("app token validation failed: {err}");
    }

    println!("  app token valid (Socket Mode ready)");
    Ok(())
}

/// Default emoji-to-color hex mappings.
fn default_emoji_colors() -> Vec<(String, String)> {
    vec![
        (":no_entry:".to_string(), "#FF0000".to_string()),
        (":red_circle:".to_string(), "#FF0000".to_string()),
        (":calendar:".to_string(), "#FF4500".to_string()),
        (":spiral_calendar_pad:".to_string(), "#FF4500".to_string()),
        (":palm_tree:".to_string(), "#808080".to_string()),
        (":house:".to_string(), "#00FF00".to_string()),
        (":large_green_circle:".to_string(), "#00FF00".to_string()),
    ]
}
