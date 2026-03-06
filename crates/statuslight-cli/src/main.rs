mod animate;
mod color_cmd;
mod daemon_client;
mod preset_cmd;
mod slack;
mod startup;
mod update;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use daemon_client::DeviceProxy;
use slicky_core::{AnimationType, Color, Config, HidSlickyDevice, Preset};

#[derive(Parser)]
#[command(
    name = "slicky",
    version,
    about = "Control your Slicky USB status light"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Set light to a named preset (e.g., red, busy, available, in-meeting)
    Set { name: String },
    /// Set light to exact RGB values (0-255 each)
    Rgb { r: u8, g: u8, b: u8 },
    /// Set light to a hex color (#RRGGBB or RRGGBB)
    Hex { color: String },
    /// Turn the light off
    Off,
    /// List all available preset names and their colors
    Presets,
    /// List connected Slicky devices
    Devices,
    /// Play an animation on the light (blocking, Ctrl-C to stop)
    Animate {
        #[command(subcommand)]
        action: AnimateAction,
    },
    /// Manage preset color overrides
    Color {
        #[command(subcommand)]
        action: ColorAction,
    },
    /// Manage custom presets
    Preset {
        #[command(subcommand)]
        action: PresetAction,
    },
    /// Manage Slack integration
    Slack {
        #[command(subcommand)]
        action: SlackAction,
    },
    /// Manage automatic startup
    Startup {
        #[command(subcommand)]
        action: StartupAction,
    },
    /// Check for updates
    Update {
        #[command(subcommand)]
        action: UpdateAction,
    },
    /// Show device, Slack, and configuration status
    Status,
    /// View or manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum AnimateAction {
    /// Smooth sine-wave breathing effect
    Breathing {
        /// Color name(s) or hex (repeatable, default: white)
        #[arg(long)]
        color: Vec<String>,
        /// Speed multiplier (default: 1.0)
        #[arg(long, default_value = "1.0")]
        speed: f64,
        /// Brightness cap (0.0–1.0, default: 1.0)
        #[arg(long, default_value = "1.0")]
        brightness: f64,
    },
    /// Hard on/off blink
    Flash {
        /// Color name(s) or hex (repeatable, default: red)
        #[arg(long)]
        color: Vec<String>,
        /// Speed multiplier (default: 1.0)
        #[arg(long, default_value = "1.0")]
        speed: f64,
        /// Brightness cap (0.0–1.0, default: 1.0)
        #[arg(long, default_value = "1.0")]
        brightness: f64,
    },
    /// Morse code SOS pattern
    Sos {
        /// Color name(s) or hex (repeatable, default: white)
        #[arg(long)]
        color: Vec<String>,
        /// Speed multiplier (default: 1.0)
        #[arg(long, default_value = "1.0")]
        speed: f64,
        /// Brightness cap (0.0–1.0, default: 1.0)
        #[arg(long, default_value = "1.0")]
        brightness: f64,
    },
    /// Sharp rise then exponential decay
    Pulse {
        /// Color name(s) or hex (repeatable, default: white)
        #[arg(long)]
        color: Vec<String>,
        /// Speed multiplier (default: 1.0)
        #[arg(long, default_value = "1.0")]
        speed: f64,
        /// Brightness cap (0.0–1.0, default: 1.0)
        #[arg(long, default_value = "1.0")]
        brightness: f64,
    },
    /// Cycle through the full hue spectrum (or cycle through specified colors)
    Rainbow {
        /// Color name(s) or hex (repeatable; omit for full spectrum)
        #[arg(long)]
        color: Vec<String>,
        /// Speed multiplier (default: 1.0)
        #[arg(long, default_value = "1.0")]
        speed: f64,
        /// Brightness cap (0.0–1.0, default: 1.0)
        #[arg(long, default_value = "1.0")]
        brightness: f64,
    },
    /// Smooth transition between colors
    Transition {
        /// Color name(s) or hex (repeatable, default: red↔blue)
        #[arg(long)]
        color: Vec<String>,
        /// Second color (backward compat, appended to color list)
        #[arg(long)]
        color2: Option<String>,
        /// Speed multiplier (default: 1.0)
        #[arg(long, default_value = "1.0")]
        speed: f64,
        /// Brightness cap (0.0–1.0, default: 1.0)
        #[arg(long, default_value = "1.0")]
        brightness: f64,
    },
}

#[derive(Subcommand)]
enum ColorAction {
    /// Override a built-in preset color
    Override {
        /// Preset name (e.g. "red", "busy")
        name: String,
        /// Hex color (e.g. "#FF4444")
        hex: String,
    },
    /// Reset a preset color to its default
    Reset {
        /// Preset name, or omit with --all to reset all
        name: Option<String>,
        /// Reset all color overrides
        #[arg(long)]
        all: bool,
    },
    /// List all preset colors with overrides
    List,
}

#[derive(Subcommand)]
enum PresetAction {
    /// Add a new custom preset
    Add {
        /// Preset name
        name: String,
        /// Hex color (e.g. "#6A0DAD")
        #[arg(long)]
        color: String,
        /// Optional animation type (breathing, flash, sos, pulse, rainbow, transition)
        #[arg(long)]
        animation: Option<String>,
        /// Animation speed multiplier (default: 1.0)
        #[arg(long, default_value = "1.0")]
        speed: f64,
    },
    /// Remove a custom preset
    Remove {
        /// Preset name to remove
        name: String,
    },
    /// List all custom presets
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SlackAction {
    /// Set up Slack integration (guided wizard, auto-opens browser)
    Setup,
    /// Open browser with pre-filled Slack app manifest (non-interactive)
    OpenSetup,
    /// Non-interactive token configuration (for macOS app)
    Configure {
        /// App-level token (xapp-...)
        #[arg(long, required_unless_present = "stdin")]
        app_token: Option<String>,
        /// Bot token (xoxb-...)
        #[arg(long, required_unless_present = "stdin")]
        bot_token: Option<String>,
        /// User token (xoxp-...)
        #[arg(long, required_unless_present = "stdin")]
        user_token: Option<String>,
        /// Read tokens from stdin (one per line: app, bot, user)
        #[arg(long)]
        stdin: bool,
    },
    /// Remove Slack credentials
    Disconnect,
    /// Show Slack connection status
    Status,
    /// Set your Slack status text and emoji
    SetStatus {
        /// Status text (e.g. "In a meeting")
        #[arg(long)]
        text: String,
        /// Status emoji (e.g. ":calendar:")
        #[arg(long)]
        emoji: String,
    },
    /// Clear your Slack status
    ClearStatus,
}

#[derive(Subcommand)]
enum StartupAction {
    /// Enable automatic startup on login (macOS LaunchAgent)
    Enable,
    /// Disable automatic startup
    Disable,
    /// Show startup status
    Status,
}

#[derive(Subcommand)]
enum UpdateAction {
    /// Check for a newer version of OpenSlicky
    Check,
    /// Show cached update status as JSON (no network, for macOS app)
    Status,
    /// Download and install the latest update
    Install,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Dump the full configuration to stdout
    Show,
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Set { name } => {
            // Try built-in preset first (with config overrides), then custom presets.
            if let Ok(preset) = Preset::from_name(&name) {
                let config = Config::load()?;
                let color = preset.color_with_overrides(&config.colors);
                let device = DeviceProxy::open()?;
                device.set_color(color).context("failed to set color")?;
                println!("Set to {} ({})", preset.name(), color);
            } else {
                // Check custom presets.
                let config = Config::load()?;
                let lower = name.to_lowercase();
                if let Some(cp) = config
                    .custom_presets
                    .iter()
                    .find(|p| p.name.to_lowercase() == lower)
                {
                    let color =
                        Color::from_hex(&cp.color).context("invalid color in custom preset")?;

                    // If the custom preset has an animation, run it.
                    if let Some(ref anim_name) = cp.animation {
                        let anim = AnimationType::from_name(anim_name)
                            .ok_or_else(|| anyhow::anyhow!("unknown animation: {anim_name}"))?;
                        animate::run(anim, &[color], cp.speed, 1.0)?;
                    } else {
                        let device = DeviceProxy::open()?;
                        device.set_color(color).context("failed to set color")?;
                        println!("Set to {} ({})", cp.name, color);
                    }
                } else {
                    bail!("unknown preset: {name}");
                }
            }
        }
        Commands::Rgb { r, g, b } => {
            let color = Color::new(r, g, b);
            let device = DeviceProxy::open()?;
            device.set_color(color).context("failed to set color")?;
            println!("Set to RGB({}, {}, {}) {}", r, g, b, color);
        }
        Commands::Hex { color: hex } => {
            let color = Color::from_hex(&hex).context("failed to parse hex color")?;
            let device = DeviceProxy::open()?;
            device.set_color(color).context("failed to set color")?;
            println!("Set to {color}");
        }
        Commands::Off => {
            let device = DeviceProxy::open()?;
            device.off().context("failed to turn off")?;
            println!("Light off");
        }
        Commands::Presets => {
            let config = Config::load()?;
            println!("{:<15}COLOR", "NAME");
            println!("{}", "-".repeat(28));
            for p in Preset::all() {
                let color = p.color_with_overrides(&config.colors);
                println!("{:<15}{}", p.name(), color);
            }
            if !config.custom_presets.is_empty() {
                println!();
                println!("Custom presets:");
                for cp in &config.custom_presets {
                    let anim = cp
                        .animation
                        .as_deref()
                        .map(|a| format!(" [{a}]"))
                        .unwrap_or_default();
                    println!("  {:<13}{}{}", cp.name, cp.color, anim);
                }
            }
        }
        Commands::Devices => {
            let devices = HidSlickyDevice::enumerate().context("failed to enumerate devices")?;
            if devices.is_empty() {
                println!("No Slicky devices found");
            } else {
                for (i, d) in devices.iter().enumerate() {
                    println!("Device {}:", i + 1);
                    if let Some(ref s) = d.serial {
                        println!("  Serial:       {s}");
                    }
                    if let Some(ref m) = d.manufacturer {
                        println!("  Manufacturer: {m}");
                    }
                    if let Some(ref p) = d.product {
                        println!("  Product:      {p}");
                    }
                }
            }
        }
        Commands::Animate { action } => {
            let (anim_type, colors, speed, brightness) = match action {
                AnimateAction::Breathing {
                    color,
                    speed,
                    brightness,
                } => (
                    AnimationType::Breathing,
                    parse_colors(&color)?,
                    speed,
                    brightness,
                ),
                AnimateAction::Flash {
                    color,
                    speed,
                    brightness,
                } => (
                    AnimationType::Flash,
                    parse_colors(&color)?,
                    speed,
                    brightness,
                ),
                AnimateAction::Sos {
                    color,
                    speed,
                    brightness,
                } => (AnimationType::Sos, parse_colors(&color)?, speed, brightness),
                AnimateAction::Pulse {
                    color,
                    speed,
                    brightness,
                } => (
                    AnimationType::Pulse,
                    parse_colors(&color)?,
                    speed,
                    brightness,
                ),
                AnimateAction::Rainbow {
                    color,
                    speed,
                    brightness,
                } => (
                    AnimationType::Rainbow,
                    parse_colors(&color)?,
                    speed,
                    brightness,
                ),
                AnimateAction::Transition {
                    mut color,
                    color2,
                    speed,
                    brightness,
                } => {
                    // Backward compat: append --color2 to the list
                    if let Some(c2) = color2 {
                        color.push(c2);
                    }
                    (
                        AnimationType::Transition,
                        parse_colors(&color)?,
                        speed,
                        brightness,
                    )
                }
            };
            animate::run(anim_type, &colors, speed, brightness)?;
        }
        Commands::Color { action } => match action {
            ColorAction::Override { name, hex } => color_cmd::override_color(&name, &hex)?,
            ColorAction::Reset { name, all } => {
                if all {
                    color_cmd::reset_all()?;
                } else if let Some(name) = name {
                    color_cmd::reset_color(&name)?;
                } else {
                    bail!("specify a preset name or use --all");
                }
            }
            ColorAction::List => color_cmd::list_colors()?,
        },
        Commands::Preset { action } => match action {
            PresetAction::Add {
                name,
                color,
                animation,
                speed,
            } => preset_cmd::add(&name, &color, animation.as_deref(), speed)?,
            PresetAction::Remove { name } => preset_cmd::remove(&name)?,
            PresetAction::List { json } => {
                if json {
                    preset_cmd::list_json()?;
                } else {
                    preset_cmd::list()?;
                }
            }
        },
        Commands::Slack { action } => match action {
            SlackAction::Setup => slack::setup()?,
            SlackAction::OpenSetup => slack::open_setup()?,
            SlackAction::Configure {
                app_token,
                bot_token,
                user_token,
                stdin,
            } => {
                if stdin {
                    slack::configure_from_stdin()?;
                } else {
                    // unwrap is safe: clap ensures these are present when --stdin is absent
                    slack::configure(
                        &app_token.unwrap(),
                        &bot_token.unwrap(),
                        &user_token.unwrap(),
                    )?;
                }
            }
            SlackAction::Disconnect => slack::disconnect()?,
            SlackAction::Status => slack::status()?,
            SlackAction::SetStatus { text, emoji } => slack::set_status(&text, &emoji)?,
            SlackAction::ClearStatus => slack::clear_status()?,
        },
        Commands::Startup { action } => match action {
            StartupAction::Enable => startup::enable()?,
            StartupAction::Disable => startup::disable()?,
            StartupAction::Status => startup::status()?,
        },
        Commands::Update { action } => match action {
            UpdateAction::Check => update::check()?,
            UpdateAction::Status => update::status()?,
            UpdateAction::Install => update::install()?,
        },
        Commands::Status => {
            let devices = HidSlickyDevice::enumerate().context("failed to enumerate devices")?;
            if devices.is_empty() {
                println!("Device:  not connected");
            } else {
                println!("Device:  connected ({} found)", devices.len());
            }

            let config = Config::load()?;
            let slack_connected = config.slack.app_token.is_some()
                || config.slack.bot_token.is_some()
                || config.slack.user_token.is_some();
            if slack_connected {
                println!("Slack:   configured");
            } else {
                println!("Slack:   not configured");
            }
            println!("Color overrides: {}", config.colors.len());
            println!("Custom presets:  {}", config.custom_presets.len());
        }
        Commands::Config { action } => match action {
            ConfigAction::Show => {
                let config = Config::load()?;
                let toml_str =
                    toml::to_string_pretty(&config).context("failed to serialize config")?;
                print!("{toml_str}");
            }
        },
    }

    Ok(())
}

/// Parse a list of color strings into a `Vec<Color>`.
fn parse_colors(strings: &[String]) -> Result<Vec<Color>> {
    strings.iter().map(|s| parse_color(s)).collect()
}

/// Parse a color string that can be either a preset name or a hex value.
fn parse_color(s: &str) -> Result<Color> {
    if let Ok(preset) = Preset::from_name(s) {
        let config = Config::load()?;
        Ok(preset.color_with_overrides(&config.colors))
    } else {
        Color::from_hex(s).context("invalid color (not a preset name or hex value)")
    }
}
