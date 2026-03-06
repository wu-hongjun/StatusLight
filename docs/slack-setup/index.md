# Slack Setup

Connect OpenSlicky to Slack for real-time DM notifications and automatic status light sync.

## Overview

OpenSlicky uses a **per-user Slack app** model. You create your own Slack app from a provided manifest, then paste three tokens into the CLI or macOS app. This keeps your credentials private — no shared OAuth app, no secrets in the binary.

> **Using the macOS app?** Click **Connect Slack** in the Slack section for an in-app wizard that guides you through each step.

## CLI Setup

### Step 1: Run Setup

```bash
slicky slack setup
```

Your browser opens automatically with a pre-filled Slack app manifest. Select your workspace and click **Create**.

### Step 2: Generate App Token

In your new app's settings:

1. Go to **Basic Information** > **App-Level Tokens**
2. Click **Generate Token and Scopes**
3. Add scope: `connections:write`
4. Click **Generate** — paste the `xapp-...` token when prompted

### Step 3: Install and Copy Tokens

1. Go to **Install App** > **Install to Workspace** and authorize
2. Copy the **Bot User OAuth Token** (`xoxb-...`) — paste when prompted
3. Copy the **User OAuth Token** (`xoxp-...`) — paste when prompted

The wizard validates each token against the Slack API before saving.

### Step 4: Restart the Daemon

```bash
# If using LaunchAgent (recommended):
launchctl stop com.openslicky.daemon
# The LaunchAgent will auto-restart it.

# Or manually:
killall slickyd
slickyd &
```

Check that Socket Mode connected:

```bash
slicky slack status
```

## Configuration

After setup, your `~/.config/openslicky/config.toml` will contain:

```toml
[slack]
app_token = "xapp-1-..."
bot_token = "xoxb-..."
user_token = "xoxp-..."
events_enabled = true

[slack.emoji_colors]
":no_entry:" = "#FF0000"
":red_circle:" = "#FF0000"
":calendar:" = "#FF4500"
":spiral_calendar_pad:" = "#FF4500"
":palm_tree:" = "#808080"
":house:" = "#00FF00"
":large_green_circle:" = "#00FF00"

[[slack.rules]]
name = "DM notification"
event = "message.im"
animation = "flash"
color = "#00FF00"
speed = 2.0
repeat = 3
```

### Emoji Colors

The `[slack.emoji_colors]` table maps Slack status emojis to device colors. When your Slack status emoji changes, the daemon polls your profile and sets the light to the matching color.

### Event Rules

The `[[slack.rules]]` array defines real-time event reactions. Each rule matches a Slack event type and triggers a temporary animation:

| Field | Description |
|-------|-------------|
| `name` | Human-readable label |
| `event` | Slack event type (`message.im`, `app_mention`) |
| `from_user` | Optional: only match this Slack user ID |
| `contains` | Optional: only match messages containing this text |
| `animation` | Animation type (`flash`, `breathing`, `pulse`, `sos`, `rainbow`, `transition`) |
| `color` | Hex color for the animation |
| `speed` | Speed multiplier (default 1.0) |
| `repeat` | Number of animation cycles (default 1) |
| `duration_secs` | Override duration (replaces repeat-based calculation) |

Rules are matched in order — first match wins.

## Disconnect

```bash
slicky slack disconnect
```

This removes all three tokens and disables events. Restart the daemon afterward.
