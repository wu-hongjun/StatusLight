# Plan 007 — Native SwiftUI GUI for OpenSlicky.app

## Context

The current `.app` uses a bash launcher that shows osascript dialogs ("already installed", install/uninstall). The user wants a real native macOS window with color buttons and Slack integration — not dialog boxes.

## Approach

Replace the bash launcher script with a **compiled SwiftUI app** (two `.swift` files, compiled with `swiftc`). The SwiftUI app communicates with the bundled `slicky` CLI binary via `Process` (subprocess calls) — no FFI linking needed.

## App Bundle Structure (unchanged)

```
OpenSlicky.app/Contents/
  MacOS/
    OpenSlicky       ← compiled SwiftUI binary (replaces bash script)
    slicky           (CLI binary)
    slickyd          (daemon binary)
  Info.plist         (LSUIElement changed to false)
  PkgInfo
```

## View Flow

```
App Launch
  ├── Translocated? → Warning: "Drag to Applications first"
  ├── No marker?    → InstallerView (Install button → admin symlinks + startup enable)
  └── Installed     → MainView
                        ├── StatusSection (device dot, current color, Slack status)
                        ├── ColorGridSection (4 status presets + 9 colors + off button)
                        ├── SlackSection (Connect/Disconnect button)
                        └── FooterSection (version, Uninstall)
```

## CLI Communication

The SwiftUI app calls the bundled `slicky` binary via `Process`:
- `slicky set <preset>` — set color
- `slicky off` — turn off
- `slicky slack login` — opens browser for OAuth (async, non-blocking)
- `slicky slack logout` — disconnect
- `slicky slack status` — parse "logged in" / "not logged in"
- `slicky startup enable/disable` — manage LaunchAgent
- `slicky devices` — check device connectivity

Admin operations (symlinks) use `osascript "do shell script ... with administrator privileges"`.

Status refreshes every 5 seconds via a timer.

## Files Created/Modified

| File | Action |
|------|--------|
| `macos/OpenSlicky/SlickyCLI.swift` | **Created** — async `Process` wrapper around CLI binary |
| `macos/OpenSlicky/OpenSlickyApp.swift` | **Created** — SwiftUI app with all views and ViewModel |
| `macos/Info.plist.template` | **Modified** — changed `LSUIElement` from `true` to `false` |
| `scripts/build-app.sh` | **Modified** — replaced bash heredoc with `swiftc` compilation |
| `docs/plans/007-swiftui-gui.md` | **Created** — this plan |

## Build Integration

In `build-app.sh`, the launcher heredoc was replaced with:
```bash
swiftc \
  -target arm64-apple-macosx13.0 \
  -O \
  -o "$MACOS_DIR/OpenSlicky" \
  "$REPO_ROOT/macos/OpenSlicky/OpenSlickyApp.swift" \
  "$REPO_ROOT/macos/OpenSlicky/SlickyCLI.swift" \
  -framework SwiftUI \
  -framework AppKit \
  -parse-as-library
```

No CI changes needed — `macos-latest` has `swiftc` available.

## Key Design Decisions

- **CLI-via-Process** (not FFI): uniform communication for all operations, no linking complexity
- **macOS 13+ target**: uses `ObservableObject`/`@Published` (not `@Observable` which needs macOS 14)
- **Two Swift files**: clean separation of CLI interop from UI code
- **5-second refresh timer**: polls device connectivity and Slack status
- **`LSUIElement: false`**: app appears in Dock with a proper window
