//! CLI handlers for the `slicky color` subcommand.
//!
//! Manages color overrides for built-in presets in the config file.

use anyhow::{Context, Result};
use slicky_core::{Color, Config, Preset};

/// Override a built-in preset color.
pub fn override_color(preset_name: &str, hex: &str) -> Result<()> {
    // Validate the preset name exists.
    Preset::from_name(preset_name).context("not a valid built-in preset name")?;
    // Validate the hex color.
    Color::from_hex(hex).context("invalid hex color")?;

    let mut config = Config::load()?;
    config
        .colors
        .insert(preset_name.to_lowercase(), hex.to_string());
    config.save()?;

    println!("Overrode {preset_name} → {hex}");
    Ok(())
}

/// Reset a preset color override back to default.
pub fn reset_color(preset_name: &str) -> Result<()> {
    let mut config = Config::load()?;
    let key = preset_name.to_lowercase();
    if config.colors.remove(&key).is_some() {
        config.save()?;
        println!("Reset {preset_name} to default");
    } else {
        println!("{preset_name} was not overridden");
    }
    Ok(())
}

/// Reset all color overrides.
pub fn reset_all() -> Result<()> {
    let mut config = Config::load()?;
    let count = config.colors.len();
    config.colors.clear();
    config.save()?;
    println!("Reset {count} color override(s)");
    Ok(())
}

/// List all preset colors with any active overrides.
pub fn list_colors() -> Result<()> {
    let config = Config::load()?;

    println!("{:<15}{:<12}OVERRIDE", "NAME", "DEFAULT");
    println!("{}", "-".repeat(40));
    for preset in Preset::all() {
        let default_hex = preset.color().to_hex();
        let override_hex = config.colors.get(preset.name());
        let override_display = override_hex.map(|h| h.as_str()).unwrap_or("-");
        println!(
            "{:<15}{:<12}{}",
            preset.name(),
            default_hex,
            override_display
        );
    }
    Ok(())
}
