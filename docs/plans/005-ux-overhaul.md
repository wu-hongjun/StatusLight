# Plan 005 — UX Overhaul: Config, Slack OAuth, Startup, Auto-Update

## Context

The current OpenSlicky UX has several pain points:
- **Slack**: No OAuth. Users must manually obtain a `xoxp-` token and pass it as a CLI arg to `slickyd` on every startup. No persistence.
- **Startup**: No launchd integration. Users must manually run `slickyd` from terminal.
- **Updates**: No update checking. Users have no way to know a new version is available.

Goal: Make OpenSlicky "extremely simple" — one command to connect Slack, one command to enable startup, and automatic update notifications.

## Overview

Four features, each building on the previous:

1. **Config file** (slicky-core) — persistent `~/.config/openslicky/config.toml`
2. **Slack OAuth** (slicky-cli) — `slicky slack login/logout/status`
3. **Startup management** (slicky-cli) — `slicky startup enable/disable/status`
4. **Auto-update** (slicky-cli + slicky-daemon) — `slicky update check` + daemon auto-check

## 1. Config File (`slicky-core`)

New module: `crates/slicky-core/src/config.rs`

```toml
# ~/.config/openslicky/config.toml
[slack]
token = "xoxp-..."
poll_interval_secs = 30

[startup]
enabled = false

[updates]
auto_check = true
last_check = "2026-03-05T00:00:00Z"
latest_version = "0.2.0"
```

Key struct:
```rust
#[derive(Serialize, Deserialize, Default)]
pub struct Config {
    pub slack: SlackConfig,      // token, poll_interval_secs
    pub startup: StartupConfig,  // enabled
    pub updates: UpdateConfig,   // auto_check, last_check, latest_version
}
```

Methods: `Config::load()`, `Config::save()`, `Config::path()` (returns `~/.config/openslicky/config.toml`)

## 2. Slack Credentials & Secrets Strategy

**Slack App**: `A0AJS883QK0`

Secrets are baked into the release binary at compile time via `env!()` macros — never hardcoded in source. Two sources:

| Context | How secrets are provided |
|---------|------------------------|
| **Local dev** | `.env` file (already gitignored) loaded via `export $(cat .env \| xargs)` before `cargo build` |
| **CI release** | GitHub Secrets → env vars in the release workflow |

### `.env` file (local, gitignored)
Contains `SLACK_CLIENT_ID`, `SLACK_CLIENT_SECRET`, and `SLACK_SIGNING_SECRET`.

### GitHub Secrets to add (via repo Settings → Secrets)
- `SLACK_CLIENT_ID`
- `SLACK_CLIENT_SECRET`

### Release workflow update (`.github/workflows/release.yml`)
Env vars added to the build step so `env!()` macros resolve at compile time.

### In Rust code (`slack.rs`)
```rust
const SLACK_CLIENT_ID: &str = env!("SLACK_CLIENT_ID");
const SLACK_CLIENT_SECRET: &str = env!("SLACK_CLIENT_SECRET");
```

## 3. Slack OAuth Flow (`slicky-cli`)

Module: `crates/slicky-cli/src/slack.rs`

Slack App configured with `users.profile:read` user scope and redirect URL `http://127.0.0.1:19876/callback`.

### `slicky slack login` flow:
1. Bind `TcpListener` on `127.0.0.1:19876`
2. Open browser to Slack OAuth authorize URL
3. User clicks "Allow" in Slack
4. Slack redirects to callback with `?code=XXXX`
5. Parse code from HTTP request line
6. Send "Success!" HTML response
7. POST `oauth.v2.access` with `{client_id, client_secret, code}` via `ureq`
8. Extract `authed_user.access_token` from response
9. Save to config, write config file

### `slicky slack logout`: Remove token from config
### `slicky slack status`: Show connection state with masked token

## 4. Startup Management (`slicky-cli`)

Module: `crates/slicky-cli/src/startup.rs`

### `slicky startup enable`:
1. Find `slickyd` binary (sibling of current exe, or `which slickyd`)
2. Write LaunchAgent plist to `~/Library/LaunchAgents/com.openslicky.daemon.plist`
3. `launchctl load -w` the plist
4. Set `config.startup.enabled = true`

### `slicky startup disable`:
1. `launchctl unload -w` the plist
2. Delete plist file
3. Set `config.startup.enabled = false`

### `slicky startup status`: Show if enabled + if daemon is running

Plist features: `RunAtLoad=true`, `KeepAlive=true`, logs to `/tmp/slicky-daemon.log`

## 5. Auto-Update (`slicky-cli` + `slicky-daemon`)

Modules: `crates/slicky-cli/src/update.rs` (blocking/ureq), `crates/slicky-daemon/src/update.rs` (async/reqwest)

- Hit `https://api.github.com/repos/wu-hongjun/OpenSilcky/releases/latest`
- Parse `tag_name`, compare with `CARGO_PKG_VERSION` via `semver`
- Rate-limit: at most once per 24h (check `config.updates.last_check`)
- If newer: log message with download URL (no auto-download)
- `slicky update check`: manual check from CLI
- Daemon: `tokio::spawn` non-blocking check on startup if `config.updates.auto_check` is true

## 6. Daemon Changes (`slicky-daemon/src/main.rs`)

Token priority: CLI arg > config file.
Spawns update check on startup.

## Dependencies Added

**Workspace `Cargo.toml`**: `toml`, `dirs`, `ureq` (with json feature), `open`, `semver`, `chrono`

| Crate | New deps |
|-------|----------|
| slicky-core | `anyhow`, `toml`, `dirs`, `chrono` |
| slicky-cli | `serde_json`, `ureq`, `open`, `semver`, `dirs`, `chrono` |
| slicky-daemon | `semver`, `chrono` |

## Files Created/Modified

| File | Action |
|------|--------|
| `.env` | **New** — Slack secrets for local dev (gitignored) |
| `.github/workflows/release.yml` | Added env vars to build step |
| `Cargo.toml` | Added workspace deps |
| `crates/slicky-core/Cargo.toml` | Added deps |
| `crates/slicky-core/src/lib.rs` | Added `pub mod config; pub use config::Config;` |
| `crates/slicky-core/src/config.rs` | **New** — Config struct, load/save |
| `crates/slicky-cli/Cargo.toml` | Added deps |
| `crates/slicky-cli/src/main.rs` | Added Slack/Startup/Update subcommands |
| `crates/slicky-cli/src/slack.rs` | **New** — OAuth login/logout/status |
| `crates/slicky-cli/src/startup.rs` | **New** — launchd enable/disable/status |
| `crates/slicky-cli/src/update.rs` | **New** — blocking update check |
| `crates/slicky-daemon/Cargo.toml` | Added deps |
| `crates/slicky-daemon/src/main.rs` | Load config, token fallback, spawn update check |
| `crates/slicky-daemon/src/update.rs` | **New** — async update check |
| `docs/plans/005-ux-overhaul.md` | **New** — this plan |

## Post-Merge Steps

After merging, add `SLACK_CLIENT_ID` and `SLACK_CLIENT_SECRET` as GitHub repository secrets (Settings → Secrets → Actions).

## Verification

1. `cargo build --workspace` — compiles clean
2. `cargo clippy --workspace -- -D warnings` — passes
3. `cargo test --workspace` — all tests pass
4. `slicky slack login` opens browser, completes OAuth
5. `slicky slack status` shows "logged in"
6. `slicky startup enable` creates plist, daemon starts
7. `slicky startup status` shows enabled + running
8. `slicky startup disable` stops daemon, removes plist
9. `slicky update check` prints current vs latest version
10. `slickyd` reads token from config (no `--slack-token` needed)
