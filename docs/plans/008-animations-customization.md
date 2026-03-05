# Plan 008 — Animations, Color Customization, Custom Presets & Color Picker

## Context

OpenSlicky's Slicky device accepts single 65-byte HID color reports — no built-in animation. Presets are a hardcoded Rust enum with fixed RGB values. Users want animations (breathing, flash, SOS, etc.), a color picker, customizable built-in colors, and the ability to create their own presets. All animation must be software-driven (rapid HID writes from the host).

## Architecture

- **Animations**: CLI blocking process (`slicky animate breathing --color green`) opens device, runs 30 FPS loop, exits on SIGTERM. SwiftUI manages the child process lifecycle.
- **Config-based customization**: `~/.config/openslicky/config.toml` gains `[colors]` (override map) and `[[custom_presets]]` (user presets) sections.
- **Color picker**: Native SwiftUI `ColorPicker` → convert to hex → `slicky hex #XXYYZZ`.

## Config Schema Additions

```toml
# Override built-in preset colors
[colors]
red = "#FF4444"
busy = "#CC0000"

# User-created presets
[[custom_presets]]
name = "focus"
color = "#6A0DAD"

[[custom_presets]]
name = "meeting-pulse"
color = "#FF4500"
animation = "breathing"
speed = 1.5
```

Rust structs added to `Config`:
```rust
pub colors: HashMap<String, String>,           // preset name → hex override
pub custom_presets: Vec<CustomPreset>,          // user presets
```

## New Files

| File | Purpose |
|------|---------|
| `crates/slicky-core/src/animation.rs` | Animation types enum, frame computation math (pure functions) |
| `crates/slicky-cli/src/animate.rs` | CLI `animate` handler: device loop at 30 FPS, ctrlc cleanup |
| `crates/slicky-cli/src/color_cmd.rs` | `slicky color override/reset/list` handlers |
| `crates/slicky-cli/src/preset_cmd.rs` | `slicky preset add/remove/list` handlers |
| `docs/plans/008-animations-customization.md` | This plan |

## Modified Files

| File | Changes |
|------|---------|
| `crates/slicky-core/src/lib.rs` | Add `pub mod animation;`, re-export types |
| `crates/slicky-core/src/color.rs` | Add `Color::lerp()`, `scale_brightness()`, `from_hsv()`, `Preset::color_with_overrides()` |
| `crates/slicky-core/src/config.rs` | Add `colors: HashMap`, `custom_presets: Vec<CustomPreset>`, `CustomPreset` struct |
| `crates/slicky-core/src/error.rs` | Add `DuplicatePreset`, `PresetNotFound` variants |
| `crates/slicky-cli/src/main.rs` | Add `Animate`, `ColorCmd`, `PresetCmd` commands; update `Set` to check overrides + custom presets |
| `crates/slicky-cli/Cargo.toml` | Add `ctrlc = "3"` dependency |
| `crates/slicky-ffi/src/lib.rs` | Handle new error variants in `error_code()` match |
| `crates/slicky-daemon/src/api.rs` | Handle new error variants in `map_slicky_error()` match |
| `macos/OpenSlicky/OpenSlickyApp.swift` | Add `ColorPickerSection`, `AnimationSection`, `CustomPresetsSection`; ViewModel animation process mgmt |
| `macos/OpenSlicky/SlickyCLI.swift` | Add `animate()`, `stopAnimation()`, `setHex()`, `listPresetsJSON()` methods |

## Animation Types & Math

All take `t = elapsed_secs * speed`, return `Color`.

| Type | Period | Formula |
|------|--------|---------|
| **Breathing** | 4s | `brightness = (1 - cos(2πt/4)) / 2` (min 0.05); `color * brightness` |
| **Flash** | 1s | `if (t % 1) < 0.5 → color else → off` |
| **SOS** | 8.4s | Morse `... --- ...` (dot=0.2s, dash=0.6s, gaps), then 3s pause |
| **Pulse** | 2s | Rise 0→1 in 20%, exponential decay `e^(-4x)` in remaining 80% |
| **Rainbow** | 6s | `hue = (t%6)/6 * 360°`, full saturation/value via `Color::from_hsv()` |
| **Transition** | 4s | `factor = (1-cos(2πt/4))/2`; `Color::lerp(c1, c2, factor)` |

## CLI Commands

```bash
# Animations (blocking, Ctrl-C to stop)
slicky animate breathing --color green --speed 2.0
slicky animate flash --color red
slicky animate sos --color white
slicky animate rainbow
slicky animate transition --color red --color2 blue

# Color overrides
slicky color override red "#FF4444"
slicky color reset red
slicky color reset --all
slicky color list

# Custom presets
slicky preset add focus --color "#6A0DAD"
slicky preset add meeting-pulse --color "#FF4500" --animation breathing --speed 1.5
slicky preset remove focus
slicky preset list              # human-readable
slicky preset list --json       # for SwiftUI parsing
```

## SwiftUI View Changes

Updated popover hierarchy:
```
MainView
  StatusSection          (unchanged)
  ColorGridSection       (colors now read from config overrides)
  CustomPresetsSection   NEW — grid of user presets, click to set/animate
  ColorPickerSection     NEW — native ColorPicker + "Set" button + hex display
  AnimationSection       NEW — type picker + speed slider + Play/Stop
  SlackSection           (unchanged)
  SettingsSection        (unchanged)
  FooterSection          (unchanged)
```

**Animation process lifecycle in ViewModel:**
1. `startAnimation()` → kill existing process → spawn `slicky animate ...` → store `Process` reference
2. `stopAnimation()` → `process.terminate()` + `waitUntilExit()` on background queue
3. Any `setPreset()`/`turnOff()`/`setPickerColor()` → calls `stopAnimation()` first

## Implementation Order

```
Phase 1: Core animation module     ← Color helpers + animation.rs (unit testable)
Phase 2: Config extensions          ← colors + custom_presets in config.rs (unit testable)
Phase 3: CLI animate command        ← animate.rs + ctrlc loop (needs device)
Phase 4: CLI color/preset commands  ← color_cmd.rs + preset_cmd.rs
Phase 5: SwiftUI integration       ← new view sections + process management
```

Phases 1-2 are independent (parallel). Phase 3 needs 1. Phase 4 needs 2. Phase 5 needs 3+4.

## Verification

1. `cargo test --workspace` — all existing + new unit tests pass (103 tests)
2. `cargo clippy --workspace -- -D warnings` — no warnings
3. `slicky animate breathing --color green` — light breathes, Ctrl-C stops cleanly
4. `slicky color override red "#FF4444"` → `slicky set red` uses new color; `slicky color reset red` restores default
5. `slicky preset add focus --color "#6A0DAD"` → `slicky set focus` works
6. `build-app.sh 0.1.0` compiles; menu bar popover shows color picker, animation controls, custom presets
7. Clicking animation Play → device animates; clicking a color preset → stops animation, sets static color
8. Color picker → pick color → Set → device changes
