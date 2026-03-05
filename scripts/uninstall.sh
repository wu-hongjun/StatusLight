#!/usr/bin/env bash
#
# uninstall.sh — Fully remove OpenSlicky from macOS
#
# Works standalone (even if the .app is already deleted).
# Can be run from the DMG, from the app bundle, or from anywhere.
#
# Usage:
#   bash uninstall.sh              # interactive (asks before removing config)
#   bash uninstall.sh --purge      # also removes ~/.config/openslicky/
#   bash uninstall.sh --keep-config # skip config removal prompt
#

set -euo pipefail

PLIST_LABEL="com.openslicky.daemon"
PLIST_PATH="$HOME/Library/LaunchAgents/${PLIST_LABEL}.plist"
SYMLINK_DIR="/usr/local/bin"
APP_PATH="/Applications/OpenSlicky.app"
CONFIG_DIR="$HOME/.config/openslicky"

PURGE=false
KEEP_CONFIG=false
for arg in "$@"; do
    case "$arg" in
        --purge) PURGE=true ;;
        --keep-config) KEEP_CONFIG=true ;;
    esac
done

# Helper: wait for a process to fully exit (up to 5 seconds, then SIGKILL)
wait_for_exit() {
    local name="$1"
    for _ in $(seq 10); do
        pgrep -x "$name" &>/dev/null || return 0
        sleep 0.5
    done
    # Force kill if still alive
    pkill -9 -x "$name" 2>/dev/null || true
}

echo "==> OpenSlicky Uninstaller"
echo ""

# --- 1. Quit running OpenSlicky app ------------------------------------------
if pgrep -x OpenSlicky &>/dev/null; then
    echo "Quitting OpenSlicky app..."
    osascript -e 'tell application "OpenSlicky" to quit' 2>/dev/null || true
    wait_for_exit OpenSlicky
    echo "  OpenSlicky app quit."
else
    echo "OpenSlicky app is not running."
fi

# --- 2. Unload LaunchAgent ---------------------------------------------------
echo "Unloading LaunchAgent..."
if launchctl list "$PLIST_LABEL" &>/dev/null; then
    launchctl unload -w "$PLIST_PATH" 2>/dev/null || true
    echo "  LaunchAgent unloaded."
else
    echo "  No LaunchAgent loaded."
fi

if [[ -f "$PLIST_PATH" ]]; then
    rm -f "$PLIST_PATH"
    echo "  Removed LaunchAgent plist."
fi

# --- 3. Kill all slicky processes and wait for confirmed exit -----------------
echo "Stopping slicky processes..."
for proc in slickyd slicky; do
    if pkill -x "$proc" 2>/dev/null; then
        echo "  Sent SIGTERM to $proc."
        wait_for_exit "$proc"
        echo "  $proc stopped."
    fi
done

# --- 4. Turn off the light (now that no other process holds the HID handle) ---
echo "Turning off light..."
if [[ -x "$SYMLINK_DIR/slicky" ]]; then
    "$SYMLINK_DIR/slicky" off 2>/dev/null && echo "  Light turned off." \
        || echo "  Could not turn off light (device may not be connected)."
elif [[ -x "$APP_PATH/Contents/MacOS/slicky" ]]; then
    "$APP_PATH/Contents/MacOS/slicky" off 2>/dev/null && echo "  Light turned off." \
        || echo "  Could not turn off light."
else
    echo "  No slicky binary found to turn off light."
fi

# --- 5. Remove symlinks (requires admin) and app bundle ----------------------
NEED_ADMIN=false
for bin in slicky slickyd; do
    if [[ -L "$SYMLINK_DIR/$bin" || -f "$SYMLINK_DIR/$bin" ]]; then
        NEED_ADMIN=true
        break
    fi
done

if $NEED_ADMIN || [[ -d "$APP_PATH" ]]; then
    echo ""
    echo "Removing CLI symlinks and app bundle (requires admin)..."
    ADMIN_SCRIPT="rm -f '$SYMLINK_DIR/slicky' '$SYMLINK_DIR/slickyd'"
    [[ -d "$APP_PATH" ]] && ADMIN_SCRIPT="$ADMIN_SCRIPT && rm -rf '$APP_PATH'"
    if command -v osascript &>/dev/null; then
        osascript -e "do shell script \"$ADMIN_SCRIPT\" with administrator privileges" 2>/dev/null \
            && echo "  Removed." \
            || echo "  WARNING: Admin access denied. Remove manually:"$'\n'"    sudo rm -f $SYMLINK_DIR/slicky $SYMLINK_DIR/slickyd"$'\n'"    sudo rm -rf $APP_PATH"
    else
        echo "  Run manually: sudo rm -f $SYMLINK_DIR/slicky $SYMLINK_DIR/slickyd && sudo rm -rf $APP_PATH"
    fi
elif [[ -d "$APP_PATH" ]]; then
    echo ""
    echo "Removing $APP_PATH..."
    rm -rf "$APP_PATH" 2>/dev/null \
        && echo "  App removed." \
        || echo "  WARNING: Could not remove $APP_PATH. Drag it to Trash manually."
else
    echo "No symlinks or app bundle found."
fi

# --- 6. Remove install markers (always, so reinstall works) -------------------
rm -f "$CONFIG_DIR"/.installed-* 2>/dev/null && echo "Install markers removed." || true

# --- 7. Configuration --------------------------------------------------------
if [[ -d "$CONFIG_DIR" ]]; then
    if $PURGE; then
        echo ""
        echo "Removing configuration at $CONFIG_DIR..."
        rm -rf "$CONFIG_DIR"
        echo "  Configuration removed."
    elif ! $KEEP_CONFIG; then
        echo ""
        echo "Configuration directory exists at: $CONFIG_DIR"
        printf "Remove configuration? [y/N] "
        read -r answer
        if [[ "$answer" =~ ^[Yy] ]]; then
            rm -rf "$CONFIG_DIR"
            echo "  Configuration removed."
        else
            echo "  Configuration preserved."
        fi
    else
        echo "Configuration preserved at $CONFIG_DIR."
    fi
fi

echo ""
echo "==> OpenSlicky has been uninstalled."
