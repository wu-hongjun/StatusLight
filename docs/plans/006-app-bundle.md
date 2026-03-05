# Plan 006 — macOS .app Bundle & Drag-to-Install DMG

## Context

The current DMG contains raw binaries (`slicky`, `slickyd`, FFI artifacts) in a flat folder. Users don't know what to do with them. We need a proper macOS .app bundle that users drag to `/Applications`, with a first-launch installer that sets up CLI symlinks and the LaunchAgent.

## App Bundle Structure

```
OpenSlicky.app/
  Contents/
    MacOS/
      OpenSlicky       (launcher shell script — main executable)
      slicky           (CLI binary)
      slickyd          (daemon binary)
    Info.plist
    PkgInfo
```

No icon for now (macOS shows default). No Rust code changes needed.

## Launcher Script (`Contents/MacOS/OpenSlicky`)

When the user double-clicks the app:

1. **Detect App Translocation** — if running from DMG/temp path, show "drag to Applications first" error and exit
2. **Check versioned marker** (`~/.config/openslicky/.installed-<version>`) — if present, show "already installed" dialog with OK + Uninstall buttons
3. **First-time install:**
   - `osascript` prompts for admin password with explanation
   - `ln -sf` symlinks `/usr/local/bin/slicky` and `/usr/local/bin/slickyd` → binaries inside the .app
   - Runs `slicky startup enable` (installs LaunchAgent pointing to slickyd inside the .app)
   - Writes marker file
   - Shows success dialog
4. **Uninstall** (button in "already installed" dialog):
   - Runs `slicky startup disable`
   - Removes `/usr/local/bin` symlinks (admin prompt)
   - Removes marker files
   - Shows confirmation (preserves `config.toml`)

**Why symlinks work:** `startup.rs`'s `find_slickyd()` calls `std::env::current_exe()` which resolves symlinks on macOS. So `/usr/local/bin/slicky` resolves to `/Applications/OpenSlicky.app/Contents/MacOS/slicky`, and `exe.with_file_name("slickyd")` correctly finds the sibling binary.

## Info.plist

- `CFBundleIdentifier`: `com.openslicky.app` (distinct from `com.openslicky.daemon`)
- `CFBundleExecutable`: `OpenSlicky` (the launcher script)
- `LSUIElement`: `true` (no Dock icon — the app runs briefly and exits)
- `CFBundleVersion` / `CFBundleShortVersionString`: substituted from git tag at build time

## DMG Layout

Uses `create-dmg --app-drop-link` for the standard drag-to-install experience:
- App icon at (150, 150)
- /Applications alias at (350, 150)

## FFI Artifacts

Shipped separately as `OpenSlicky-FFI-<tag>.zip` attached to the GitHub Release. Not inside the .app.

## Files Created/Modified

| File | Action |
|------|--------|
| `macos/Info.plist.template` | **Created** — plist with `${VERSION}` placeholder |
| `scripts/build-app.sh` | **Created** — builds .app structure from release binaries |
| `.github/workflows/release.yml` | **Modified** — .app bundle DMG + separate FFI zip |
| `.gitignore` | **Modified** — added `*.dmg` |
| `docs/plans/006-app-bundle.md` | **Created** — this plan |

## Verification

1. `bash scripts/build-app.sh 0.1.0` creates `target/release/OpenSlicky.app/` with correct structure
2. DMG opens showing .app and /Applications alias side by side
3. Double-clicking .app from /Applications shows install dialog, creates symlinks, starts daemon
4. `which slicky` returns `/usr/local/bin/slicky`
5. `slicky set green` works from terminal
6. Double-clicking .app again shows "already installed" dialog
7. "Uninstall" removes symlinks, stops daemon, removes LaunchAgent
8. Running .app directly from DMG (without dragging) shows translocation warning
