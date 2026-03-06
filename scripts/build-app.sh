#!/usr/bin/env bash
#
# build-app.sh — Build StatusLight.app bundle and DMG
#
# Usage: bash scripts/build-app.sh <version>
#   e.g. bash scripts/build-app.sh 0.1.0
#
# Expects release binaries at target/release/{statuslight,statuslightd}.
# Produces  target/release/StatusLight.app/  and  StatusLight-v<version>-aarch64-apple-darwin.dmg

set -euo pipefail

VERSION="${1:?Usage: build-app.sh <version>}"
# Strip leading 'v' if present for plist version strings
PLIST_VERSION="${VERSION#v}"

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$REPO_ROOT/target/release/StatusLight.app"
CONTENTS="$APP/Contents"
MACOS_DIR="$CONTENTS/MacOS"

echo "==> Building StatusLight.app (version ${PLIST_VERSION})"

# --- Clean previous build ------------------------------------------------
rm -rf "$APP"
mkdir -p "$MACOS_DIR"

# --- Copy binaries --------------------------------------------------------
# The CLI binary is renamed to statuslight-cli inside the bundle to avoid a
# case-insensitive collision with the SwiftUI launcher binary (StatusLight).
for bin in statuslight statuslightd; do
  src="$REPO_ROOT/target/release/$bin"
  if [[ ! -f "$src" ]]; then
    echo "ERROR: $src not found. Run 'cargo build --workspace --release' first." >&2
    exit 1
  fi
  dest_name="$bin"
  if [[ "$bin" == "statuslight" ]]; then
    dest_name="statuslight-cli"
  fi
  cp "$src" "$MACOS_DIR/$dest_name"
done

# --- Info.plist -----------------------------------------------------------
sed "s/\${VERSION}/${PLIST_VERSION}/g" \
  "$REPO_ROOT/macos/Info.plist.template" > "$CONTENTS/Info.plist"

# --- PkgInfo --------------------------------------------------------------
printf 'APPL????' > "$CONTENTS/PkgInfo"

# --- App icon (Resources/AppIcon.icns) ------------------------------------
echo "==> Generating app icon..."
RESOURCES_DIR="$CONTENTS/Resources"
mkdir -p "$RESOURCES_DIR"
ICONSET_DIR="$RESOURCES_DIR/AppIcon.iconset"
swift "$REPO_ROOT/scripts/generate-icon.swift" "$ICONSET_DIR"
iconutil -c icns "$ICONSET_DIR" -o "$RESOURCES_DIR/AppIcon.icns"
rm -rf "$ICONSET_DIR"

# --- Compile SwiftUI launcher (Contents/MacOS/StatusLight) ------------------
echo "==> Compiling SwiftUI launcher..."
swiftc \
  -target arm64-apple-macosx13.0 \
  -O \
  -o "$MACOS_DIR/StatusLight" \
  "$REPO_ROOT/macos/StatusLight/StatusLightApp.swift" \
  "$REPO_ROOT/macos/StatusLight/StatusLightCLI.swift" \
  -framework SwiftUI \
  -framework AppKit \
  -parse-as-library

# --- Ad-hoc codesign (prevents "damaged" Gatekeeper error) -----------------
echo "==> Ad-hoc signing app bundle..."
codesign --force --deep -s - "$APP"

echo "==> App bundle created at: $APP"

# --- Build DMG (if create-dmg is available) --------------------------------
if command -v create-dmg &>/dev/null; then
  TAG="v${PLIST_VERSION}"
  DMG_NAME="StatusLight-${TAG}-aarch64-apple-darwin.dmg"
  DMG_PATH="$REPO_ROOT/$DMG_NAME"

  echo "==> Building DMG: $DMG_NAME"

  # Stage contents: .app + uninstaller .app
  DMG_STAGE="$REPO_ROOT/target/release/dmg-stage"
  rm -rf "$DMG_STAGE"
  mkdir -p "$DMG_STAGE"
  cp -R "$APP" "$DMG_STAGE/"

  # Build the uninstaller as a double-clickable .app (AppleScript applet)
  echo "==> Building Uninstall StatusLight.app..."
  UNINSTALL_APP="$DMG_STAGE/Uninstall StatusLight.app"
  osacompile -o "$UNINSTALL_APP" "$REPO_ROOT/scripts/uninstall.applescript"

  # Embed the shell script inside the applet for Terminal-based uninstall too
  cp "$REPO_ROOT/scripts/uninstall.sh" "$UNINSTALL_APP/Contents/Resources/uninstall.sh"
  chmod +x "$UNINSTALL_APP/Contents/Resources/uninstall.sh"

  # Ad-hoc sign the uninstaller applet
  codesign --force --deep -s - "$UNINSTALL_APP"

  # create-dmg fails if target exists
  rm -f "$DMG_PATH"

  create-dmg \
    --volname "StatusLight ${TAG}" \
    --window-size 500 340 \
    --icon-size 80 \
    --app-drop-link 350 120 \
    --icon "StatusLight.app" 150 120 \
    --icon "Uninstall StatusLight.app" 250 260 \
    --no-internet-enable \
    "$DMG_PATH" \
    "$DMG_STAGE"

  rm -rf "$DMG_STAGE"

  echo "==> DMG created at: $DMG_PATH"
else
  echo "==> Skipping DMG (install create-dmg: brew install create-dmg)"
fi
