# Plan 009 — Dual UI (Menu Bar vs Window) + CLI Completeness

## Context

The current SwiftUI app puts everything in a single `MenuBarExtra` popup — status, color grids, custom presets, color picker, animations, Slack, settings, and footer. This creates a cramped, scroll-heavy experience. The user wants:

1. **On/Off button moves to the top** status bar area
2. **Two distinct UIs** — a compact menu bar popup with essentials, and a full GUI window with all features (shown when "Show in Dock" is on)
3. **CLI completeness** — every operation should be possible from the CLI

## Architecture

### SwiftUI: Two Scene Types

```
OpenSlickyApp
├── MenuBarExtra (always present)
│   └── MenuBarView  — compact, essentials only
└── Window("OpenSlicky", id: "main")  — full GUI
    └── FullWindowView  — all features
```

- `MenuBarExtra` is always active (the app is always a menu bar app)
- `Window` is always declared in the scene builder (required by SwiftUI)
- When `showInDock = true`: activation policy is `.regular`, dock icon visible, clicking dock icon opens the window
- When `showInDock = false`: activation policy is `.accessory`, no dock icon, window not accessible (menu bar only)

### Menu Bar Popup (compact, ~340px)

```
┌──────────────────────────────────┐
│ ● Device connected      Off [⏻] │  ← StatusSection with On/Off
│──────────────────────────────────│
│ STATUS                           │
│ [Available] [Busy] [Away] [Meet] │  ← 4 status presets
│──────────────────────────────────│
│ COLORS                           │
│ [Red] [Org] [Yel] [Grn]         │
│ [Cyn] [Blu] [Pur] [Mag]         │  ← 9 color presets
│ [Wht]                            │
│──────────────────────────────────│
│ [🔲 Open OpenSlicky...]          │  ← open full window (if dock mode)
│──────────────────────────────────│
│ OpenSlicky v0.1.0                │  ← minimal footer
└──────────────────────────────────┘
```

### Full GUI Window

```
┌─ OpenSlicky ──────────────────────────────────────┐
│ ● Device connected              Off [⏻]           │
│───────────────────────────────────────────────────│
│ STATUS                                            │
│ [Available] [Busy] [Away] [In Meeting]            │
│───────────────────────────────────────────────────│
│ COLORS                                            │
│ [Red] [Orange] [Yellow] [Green]                   │
│ [Cyan] [Blue] [Purple] [Magenta]                  │
│ [White]                                           │
│───────────────────────────────────────────────────│
│ CUSTOM PRESETS                   (if any exist)   │
│ [focus] [meeting-pulse] ...                       │
│───────────────────────────────────────────────────│
│ COLOR PICKER                                      │
│ [picker]  #FF0000        [Set]                    │
│───────────────────────────────────────────────────│
│ ANIMATION                                         │
│ [Breathing]     [Play] / [Stop]                   │
│ Speed: slider  1.5x                               │
│───────────────────────────────────────────────────│
│ SLACK                                             │
│ ● Connected            [Disconnect]               │
│ Auto-sync status to Slack                         │
│───────────────────────────────────────────────────│
│ SETTINGS                                          │
│ Show in Dock                                      │
│───────────────────────────────────────────────────│
│ OpenSlicky v0.1.0               [Uninstall]       │
└───────────────────────────────────────────────────┘
```

### CLI Additions

Two new commands to fill gaps:

```bash
slicky status          # Device connected? Current color? Slack status?
slicky config show     # Dump full config.toml to stdout
```

## Files Modified

| File | Changes |
|------|---------|
| `macos/OpenSlicky/OpenSlickyApp.swift` | Split into MenuBarView + FullWindowView; move On/Off to StatusSection; add Window scene; add "Open OpenSlicky..." button |
| `crates/slicky-cli/src/main.rs` | Add `Status` and `Config Show` commands |
| `crates/slicky-cli/Cargo.toml` | Add `toml` dependency for config serialization |

## Implementation Order

1. **CLI additions** — Add `Status` and `Config Show` commands (Rust, testable)
2. **SwiftUI refactor** — Split views, add Window scene, move On/Off button

## Verification

1. `cargo test --workspace` — all tests pass
2. `cargo clippy --workspace -- -D warnings` — no warnings
3. `slicky status` — shows device/slack/config summary
4. `slicky config show` — dumps config TOML
5. Menu bar popup shows only: status+on/off, status presets, color presets, "Open OpenSlicky...", version
6. With "Show in Dock" enabled: dock icon appears, clicking it opens the full GUI window
7. Full window shows all sections: custom presets, color picker, animations, Slack, settings, uninstall
8. On/Off button in status bar turns off light and stops any animation
9. Toggling "Show in Dock" off hides dock icon and closes the window
