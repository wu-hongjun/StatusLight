//! Persistent configuration stored at `~/.config/openslicky/config.toml`.

use std::collections::HashMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Top-level configuration.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub slack: SlackConfig,
    #[serde(default)]
    pub startup: StartupConfig,
    #[serde(default)]
    pub updates: UpdateConfig,
    /// Override built-in preset colors (preset name → hex string).
    #[serde(default)]
    pub colors: HashMap<String, String>,
    /// User-created custom presets.
    #[serde(default)]
    pub custom_presets: Vec<CustomPreset>,
}

/// A user-defined preset with optional animation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomPreset {
    /// Display name (e.g. "focus", "meeting-pulse").
    pub name: String,
    /// Hex color string (e.g. "#6A0DAD").
    pub color: String,
    /// Optional animation type (e.g. "breathing", "flash").
    #[serde(default)]
    pub animation: Option<String>,
    /// Animation speed multiplier (default 1.0).
    #[serde(default = "default_speed")]
    pub speed: f64,
}

fn default_speed() -> f64 {
    1.0
}

/// Slack integration settings.
#[derive(Debug, Serialize, Deserialize)]
pub struct SlackConfig {
    /// OAuth user token (`xoxp-...`).
    pub token: Option<String>,
    /// Polling interval in seconds (default 30).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
}

fn default_poll_interval() -> u64 {
    30
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            token: None,
            poll_interval_secs: 30,
        }
    }
}

/// Startup / launchd settings.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct StartupConfig {
    /// Whether the LaunchAgent is installed.
    #[serde(default)]
    pub enabled: bool,
}

/// Auto-update settings.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// Whether the daemon should check for updates on startup.
    #[serde(default = "default_auto_check")]
    pub auto_check: bool,
    /// Timestamp of the last update check.
    pub last_check: Option<DateTime<Utc>>,
    /// Latest version seen from GitHub releases.
    pub latest_version: Option<String>,
}

fn default_auto_check() -> bool {
    true
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            auto_check: true,
            last_check: None,
            latest_version: None,
        }
    }
}

impl Config {
    /// Returns `~/.config/openslicky/config.toml`.
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("openslicky").join("config.toml"))
    }

    /// Load config from disk, returning `Default` if the file doesn't exist.
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("cannot determine config dir"))?;
        Self::load_from(&path)
    }

    /// Load config from a specific path, returning `Default` if the file doesn't exist.
    pub fn load_from(path: &PathBuf) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Write config to disk, creating parent directories as needed.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("cannot determine config dir"))?;
        self.save_to(&path)
    }

    /// Write config to a specific path, creating parent directories as needed.
    pub fn save_to(&self, path: &PathBuf) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        fs::write(path, contents)?;
        // Restrict permissions since the file may contain secrets.
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = Config::default();
        assert!(config.slack.token.is_none());
        assert_eq!(config.slack.poll_interval_secs, 30);
        assert!(!config.startup.enabled);
        assert!(config.updates.auto_check);
        assert!(config.updates.last_check.is_none());
        assert!(config.updates.latest_version.is_none());
    }

    #[test]
    fn config_path_is_under_config_dir() {
        if let Some(path) = Config::path() {
            assert!(path.ends_with("openslicky/config.toml"));
        }
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("openslicky-test-round-trip");
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);

        let mut config = Config::default();
        config.slack.token = Some("xoxp-test-token-123".to_string());
        config.slack.poll_interval_secs = 60;
        config.startup.enabled = true;
        config.updates.auto_check = false;
        config.updates.latest_version = Some("1.2.3".to_string());

        config.save_to(&path).expect("save failed");
        let loaded = Config::load_from(&path).expect("load failed");

        assert_eq!(loaded.slack.token.as_deref(), Some("xoxp-test-token-123"));
        assert_eq!(loaded.slack.poll_interval_secs, 60);
        assert!(loaded.startup.enabled);
        assert!(!loaded.updates.auto_check);
        assert_eq!(loaded.updates.latest_version.as_deref(), Some("1.2.3"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let path = std::env::temp_dir()
            .join("openslicky-test-missing")
            .join("config.toml");
        let _ = std::fs::remove_file(&path);

        let config = Config::load_from(&path).expect("load failed");
        assert!(config.slack.token.is_none());
    }

    #[test]
    fn deserialize_partial_toml_with_token() {
        let toml_str = r#"
[slack]
token = "xoxp-partial"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert_eq!(config.slack.token.as_deref(), Some("xoxp-partial"));
        // poll_interval_secs gets its serde default of 30.
        assert_eq!(config.slack.poll_interval_secs, 30);
        assert!(!config.startup.enabled);
    }

    #[test]
    fn deserialize_partial_toml_with_updates() {
        let toml_str = r#"
[updates]
auto_check = false
"#;
        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert!(!config.updates.auto_check);
    }

    #[test]
    fn deserialize_empty_toml() {
        let config: Config = toml::from_str("").expect("parse failed");
        assert!(config.slack.token.is_none());
        // Serde defaults apply when deserializing from TOML.
        assert_eq!(config.slack.poll_interval_secs, 30);
        assert!(config.updates.auto_check);
    }

    #[test]
    fn serialize_produces_valid_toml() {
        let mut config = Config::default();
        config.slack.token = Some("xoxp-serialize-test".to_string());
        config.slack.poll_interval_secs = 45;

        let toml_str = toml::to_string_pretty(&config).expect("serialize failed");
        assert!(toml_str.contains("xoxp-serialize-test"));
        assert!(toml_str.contains("45"));

        // Verify round-trip through serialization.
        let parsed: Config = toml::from_str(&toml_str).expect("re-parse failed");
        assert_eq!(parsed.slack.token.as_deref(), Some("xoxp-serialize-test"));
        assert_eq!(parsed.slack.poll_interval_secs, 45);
    }

    #[cfg(unix)]
    #[test]
    fn save_sets_restrictive_permissions() {
        let dir = std::env::temp_dir().join("openslicky-test-perms");
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);

        let config = Config::default();
        config.save_to(&path).expect("save failed");

        let metadata = std::fs::metadata(&path).expect("metadata failed");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config file should be owner-only readable");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = std::env::temp_dir()
            .join("openslicky-test-mkdir")
            .join("nested");
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("openslicky-test-mkdir"));

        let config = Config::default();
        config.save_to(&path).expect("save failed");
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("openslicky-test-mkdir"));
    }

    #[test]
    fn default_config_has_empty_colors_and_presets() {
        let config = Config::default();
        assert!(config.colors.is_empty());
        assert!(config.custom_presets.is_empty());
    }

    #[test]
    fn deserialize_color_overrides() {
        let toml_str = r##"
[colors]
red = "#FF4444"
busy = "#CC0000"
"##;
        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert_eq!(config.colors.get("red").unwrap(), "#FF4444");
        assert_eq!(config.colors.get("busy").unwrap(), "#CC0000");
    }

    #[test]
    fn deserialize_custom_presets() {
        let toml_str = r##"
[[custom_presets]]
name = "focus"
color = "#6A0DAD"

[[custom_presets]]
name = "meeting-pulse"
color = "#FF4500"
animation = "breathing"
speed = 1.5
"##;
        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert_eq!(config.custom_presets.len(), 2);
        assert_eq!(config.custom_presets[0].name, "focus");
        assert_eq!(config.custom_presets[0].color, "#6A0DAD");
        assert!(config.custom_presets[0].animation.is_none());
        assert_eq!(config.custom_presets[0].speed, 1.0); // default
        assert_eq!(config.custom_presets[1].name, "meeting-pulse");
        assert_eq!(
            config.custom_presets[1].animation.as_deref(),
            Some("breathing")
        );
        assert_eq!(config.custom_presets[1].speed, 1.5);
    }

    #[test]
    fn custom_preset_round_trip() {
        let dir = std::env::temp_dir().join("openslicky-test-custom-presets");
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);

        let mut config = Config::default();
        config
            .colors
            .insert("red".to_string(), "#FF4444".to_string());
        config.custom_presets.push(super::CustomPreset {
            name: "focus".to_string(),
            color: "#6A0DAD".to_string(),
            animation: Some("breathing".to_string()),
            speed: 2.0,
        });

        config.save_to(&path).expect("save failed");
        let loaded = Config::load_from(&path).expect("load failed");

        assert_eq!(loaded.colors.get("red").unwrap(), "#FF4444");
        assert_eq!(loaded.custom_presets.len(), 1);
        assert_eq!(loaded.custom_presets[0].name, "focus");
        assert_eq!(
            loaded.custom_presets[0].animation.as_deref(),
            Some("breathing")
        );
        assert_eq!(loaded.custom_presets[0].speed, 2.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deserialize_with_chrono_timestamp() {
        let toml_str = r#"
[updates]
auto_check = true
last_check = "2026-03-05T12:00:00Z"
latest_version = "0.2.0"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert!(config.updates.last_check.is_some());
        assert_eq!(config.updates.latest_version.as_deref(), Some("0.2.0"));
    }
}
