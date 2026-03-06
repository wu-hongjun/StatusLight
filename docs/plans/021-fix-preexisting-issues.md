# Plan 021 — Fix Pre-existing Issues

**Status:** Implemented

**Context:** After v0.2.0 release, a multi-codex review identified ~16 pre-existing issues across the codebase. These were deferred during the review but are now fixed.

## Changes Made

### Phase 1: Mechanical Fixes
- `pub` → `pub(crate)` on all `AppStateInner` and `SlackState` fields
- Threaded `&HidApi` parameter through `hid_helpers`, `DeviceDriver` trait, all 8 drivers, and `DeviceRegistry`
- Note: Removing `async` from handlers was reverted — axum requires async fns

### Phase 2: Daemon Async Safety
- SIGTERM handler uses `.context()` instead of `.expect()`
- `spawn_blocking` for `Config::load()` in `post_color()` (safe on any runtime flavor)
- Fixed stale `enabled` state in `post_slack_configure` response

### Phase 3: Shared reqwest::Client
- Added `http_client: reqwest::Client` to `AppStateInner`
- Used across all Slack API calls instead of per-request client creation

### Phase 4: Correctness Fixes
- Old `~/.config/openslicky/` cleaned up after migration
- Documented Send-not-Sync design on `StatusLightDevice` trait
- Fixed animation `prev_color` race (capture inside spawned task)
- ACK failures logged instead of aborting Socket Mode connection

### Phase 5: Token Security (Revised)
- **Changed from OS keyring to file-based AES-256-GCM encryption** to avoid macOS Keychain popups
- New `token_store` module encrypts with machine-derived key (SHA-256 of hostname + username + salt)
- Tokens stored in `~/.config/statuslight/tokens.enc`
- `/slack/configure` API persists tokens to encrypted store
- `--slack-app-token` CLI flag deprecated and hidden

## Design Decisions

| Question | Decision | Rationale |
|----------|----------|-----------|
| HidApi sharing | Pass `&HidApi` as function parameter | Supports hot-plug; OnceLock would cache stale device list |
| StatusLightDevice + Sync? | Do NOT add Sync bound | HidDevice is not Sync; would force all 8 drivers to add Mutex |
| Token storage | AES-256-GCM file encryption | No OS popups; machine-derived key prevents casual plaintext exposure |
| Deprecate CLI token flags? | Yes — hide from help, warn on use | Security: prevents tokens in shell history |

## Dependencies Added
- `aes-gcm = "0.10"` — AES-256-GCM authenticated encryption
- `rand = "0.9"` — Nonce generation
- `base64 = "0.22"` — Encoding encrypted data
- `sha2 = "0.10"` — Key derivation from machine identity
