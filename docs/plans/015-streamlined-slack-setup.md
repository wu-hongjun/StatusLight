# Plan 015 — Streamlined Slack Setup Flow

## Context

The Slack setup from Plan 014 works but requires 12+ steps across 3 interfaces. This plan streamlines it:

- **CLI**: Auto-opens browser with pre-filled manifest URL (no manual manifest copying)
- **macOS app**: In-app 4-step wizard with paste fields and inline validation
- Default app name changed to **"Status Light"**

## Changes

### CLI (`slack.rs`, `main.rs`)
- `percent_encode()` — inline RFC 3986 encoding (no new dependency)
- `manifest_url()` — builds Slack app-creation URL with pre-filled JSON manifest
- `prompt_continue()` — accepts empty input for "press Enter" prompts
- `open_setup()` — non-interactive, opens browser (for macOS app)
- `configure()` — non-interactive token setup with validation (for macOS app)
- `save_tokens()` — extracted shared token-saving logic
- `setup()` — updated to auto-open browser, clearer step-by-step flow
- New `SlackAction` variants: `OpenSetup`, `Configure { app_token, bot_token, user_token }`

### macOS App (`SlickyCLI.swift`, `OpenSlickyApp.swift`)
- `openSlackAppCreation()` — CLI bridge for `slack open-setup`
- `configureSlack()` — CLI bridge for `slack configure`
- `SlackSetupWizard` — 4-step sheet: create app, app token, install & tokens, verify & connect
- `SlackSection` — "Setup Guide" button replaced with "Connect Slack" + wizard sheet
- Removed `openSlackSetup()` (was opening GitHub Pages)

### Docs
- `docs/slack-setup/index.md` — simplified to reflect auto-open browser flow
- `docs/slack-setup/manifest.json` — app name "OpenSlicky" → "Status Light"

## Files Modified

| File | Change |
|------|--------|
| `crates/slicky-cli/src/slack.rs` | New helpers + streamlined setup |
| `crates/slicky-cli/src/main.rs` | New SlackAction variants |
| `macos/OpenSlicky/SlickyCLI.swift` | New CLI bridge methods |
| `macos/OpenSlicky/OpenSlickyApp.swift` | SlackSetupWizard + updated SlackSection |
| `docs/slack-setup/index.md` | Simplified docs |
| `docs/slack-setup/manifest.json` | Renamed to "Status Light" |
