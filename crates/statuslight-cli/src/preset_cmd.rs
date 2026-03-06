//! CLI handlers for the `slicky preset` subcommand.
//!
//! Manages user-created custom presets in the config file.

use anyhow::{bail, Context, Result};
use slicky_core::{AnimationType, Color, Config, CustomPreset};

/// Add a new custom preset.
pub fn add(name: &str, hex: &str, animation: Option<&str>, speed: f64) -> Result<()> {
    // Validate inputs.
    Color::from_hex(hex).context("invalid hex color")?;
    if let Some(anim) = animation {
        if AnimationType::from_name(anim).is_none() {
            bail!("unknown animation type: {anim}");
        }
    }

    let mut config = Config::load()?;

    // Check for duplicates.
    let lower_name = name.to_lowercase();
    if config
        .custom_presets
        .iter()
        .any(|p| p.name.to_lowercase() == lower_name)
    {
        bail!("custom preset '{name}' already exists — remove it first to replace");
    }

    config.custom_presets.push(CustomPreset {
        name: name.to_string(),
        color: hex.to_string(),
        animation: animation.map(|s| s.to_string()),
        speed,
    });
    config.save()?;

    let anim_msg = animation.map_or(String::new(), |a| {
        format!(" (animation: {a}, speed: {speed})")
    });
    println!("Added preset '{name}' → {hex}{anim_msg}");
    Ok(())
}

/// Remove a custom preset by name.
pub fn remove(name: &str) -> Result<()> {
    let mut config = Config::load()?;
    let lower_name = name.to_lowercase();
    let before = config.custom_presets.len();
    config
        .custom_presets
        .retain(|p| p.name.to_lowercase() != lower_name);

    if config.custom_presets.len() == before {
        bail!("custom preset '{name}' not found");
    }

    config.save()?;
    println!("Removed preset '{name}'");
    Ok(())
}

/// List all custom presets (human-readable).
pub fn list() -> Result<()> {
    let config = Config::load()?;

    if config.custom_presets.is_empty() {
        println!("No custom presets defined");
        return Ok(());
    }

    println!("{:<18}{:<12}{:<14}SPEED", "NAME", "COLOR", "ANIMATION");
    println!("{}", "-".repeat(50));
    for p in &config.custom_presets {
        let anim = p.animation.as_deref().unwrap_or("-");
        let speed = if p.animation.is_some() {
            format!("{:.1}x", p.speed)
        } else {
            "-".to_string()
        };
        println!("{:<18}{:<12}{:<14}{}", p.name, p.color, anim, speed);
    }
    Ok(())
}

/// List all custom presets as JSON (for SwiftUI parsing).
pub fn list_json() -> Result<()> {
    let config = Config::load()?;
    let json = serde_json::to_string(&config.custom_presets)?;
    println!("{json}");
    Ok(())
}
