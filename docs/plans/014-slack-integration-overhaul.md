# Plan 014 — Slack Integration Overhaul

## Summary

Replaced the broken shared-app OAuth model with per-user Slack apps (created from
a provided manifest). Added Socket Mode for real-time WebSocket events and
configurable event→animation rules stored in TOML.

## Changes Made

### Core (`slicky-core`)
- **config.rs**: Expanded `SlackConfig` with three-token model (`app_token`,
  `bot_token`, `user_token`), `events_enabled`, `emoji_colors` HashMap,
  `rules` Vec. Added `SlackRule` struct. Legacy `token` field migrates
  automatically to `user_token` on load.
- **animation.rs**: Added `AnimationType::period()` returning base cycle
  duration in seconds.
- **lib.rs**: Re-exported `SlackRule`.

### CLI (`slicky-cli`)
- **slack.rs**: Removed `env!()` macros for `SLACK_CLIENT_ID`/`SLACK_CLIENT_SECRET`,
  OAuth flow, TCP listener. Replaced with guided `setup()` wizard that prompts
  for three tokens, validates them via Slack API, saves to config with default
  emoji colors and rules. Added `disconnect()` to clear all tokens.
- **main.rs**: Renamed `SlackAction::Login`→`Setup`, `Logout`→`Disconnect`.
  Updated status command to check new token fields.

### Daemon (`slicky-daemon`)
- **state.rs**: Rewrote `SlackState` with three tokens, `emoji_colors` HashMap,
  `rules` Vec, separate `socket_handle` and `emoji_poll_handle`. Added
  `event_animation_active` AtomicBool and `event_animation_handle` to
  `AppStateInner`.
- **slack.rs**: Complete rewrite with four components:
  (A) Socket Mode WebSocket client with exponential backoff reconnection
  (B) Rule matcher — first-match event dispatch with user/text filters
  (C) Event animation runner — temporary animation then restore previous color
  (D) Emoji status poller — 60s interval, skips during event animations
- **api.rs**: Updated `SlackStatusResponse` with `has_app_token`, `has_bot_token`,
  `has_user_token`, `socket_connected`, `rules_count`. Updated configure/enable/
  disable handlers for Socket Mode + emoji poll.
- **main.rs**: Replaced `--slack-token`/`--slack-interval` with `--slack-app-token`.
  Startup starts Socket Mode and/or emoji poll based on available tokens.

### Dependencies
- Added `tokio-tungstenite` 0.24 and `futures-util` 0.3 to workspace and daemon.
- Added `time` and `sync` features to tokio workspace dep.

### Swift UI (`macos/OpenSlicky/`)
- **OpenSlickyApp.swift**: `connectSlack()` → `openSlackSetup()` (opens GitHub
  Pages guide). SlackSection shows "Socket Mode" indicator when connected.
- **SlickyCLI.swift**: Removed `slackLogin()`, added `slackDisconnect()`.
  Updated `isSlackConnected()` to check for "connected" instead of "logged in".

### CI/CD
- **release.yml**: Removed `SLACK_CLIENT_ID`/`SLACK_CLIENT_SECRET` env vars
  from the Build step. No longer needed since `env!()` macros are gone.

### Documentation
- **docs/slack-setup/index.md**: Step-by-step setup guide.
- **docs/slack-setup/manifest.json**: Slack app manifest for "Create from manifest".
- **mkdocs.yml**: Added Slack Setup page to nav.

## Verification

- `cargo fmt --all` — clean
- `cargo clippy --workspace -- -D warnings` — zero warnings
- `cargo test --workspace` — 105 tests pass (85 core + 20 CLI)
