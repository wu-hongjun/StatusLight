//! Persistent configuration stored at `~/.config/statuslight/config.toml`.

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
    /// App-level token (`xapp-...`) for Socket Mode.
    pub app_token: Option<String>,
    /// Bot token (`xoxb-...`) for API calls.
    pub bot_token: Option<String>,
    /// User token (`xoxp-...`) for profile read/write.
    pub user_token: Option<String>,
    /// Whether Socket Mode event handling is enabled.
    #[serde(default)]
    pub events_enabled: bool,
    /// Emoji-to-color mappings (e.g. `":no_entry:" = "#FF0000"`).
    #[serde(default)]
    pub emoji_colors: HashMap<String, String>,
    /// Event-driven animation rules.
    #[serde(default)]
    pub rules: Vec<SlackRule>,

    // Legacy field — kept for migration, never serialized.
    #[serde(default, skip_serializing)]
    pub token: Option<String>,
    #[serde(default = "default_poll_interval", skip_serializing)]
    pub poll_interval_secs: u64,
}

fn default_poll_interval() -> u64 {
    30
}

/// A rule that maps a Slack event to a device animation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SlackRule {
    /// Human-readable name (e.g. "DM from boss").
    pub name: String,
    /// Slack event type (e.g. "message.im", "app_mention").
    pub event: String,
    /// Optional Slack user ID filter.
    pub from_user: Option<String>,
    /// Optional text substring filter.
    pub contains: Option<String>,
    /// Animation type name (e.g. "flash", "breathing").
    pub animation: String,
    /// Hex color for the animation (e.g. "#00FF00").
    pub color: String,
    /// Speed multiplier (default 1.0).
    #[serde(default = "default_speed")]
    pub speed: f64,
    /// Number of animation repeats (default 1).
    #[serde(default = "default_repeat")]
    pub repeat: u32,
    /// Override duration in seconds (replaces repeat-based duration).
    pub duration_secs: Option<f64>,
}

fn default_repeat() -> u32 {
    1
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            app_token: None,
            bot_token: None,
            user_token: None,
            events_enabled: false,
            emoji_colors: HashMap::new(),
            rules: Vec::new(),
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
    /// Returns `~/.config/statuslight/config.toml`.
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("statuslight").join("config.toml"))
    }

    /// Load config from disk, returning `Default` if the file doesn't exist.
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("cannot determine config dir"))?;
        Self::load_from(&path)
    }

    /// Load config from a specific path, returning `Default` if the file doesn't exist.
    ///
    /// Performs legacy migration: if `slack.token` is set but `slack.user_token`
    /// is not, copies the value over and saves.
    pub fn load_from(path: &PathBuf) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = fs::read_to_string(path)?;
        let mut config: Config = toml::from_str(&contents)?;

        // Migrate legacy single token → user_token.
        if config.slack.token.is_some() && config.slack.user_token.is_none() {
            config.slack.user_token = config.slack.token.take();
            let _ = config.save_to(path);
        }

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
        assert!(config.slack.app_token.is_none());
        assert!(config.slack.bot_token.is_none());
        assert!(config.slack.user_token.is_none());
        assert!(!config.slack.events_enabled);
        assert!(config.slack.emoji_colors.is_empty());
        assert!(config.slack.rules.is_empty());
        assert!(!config.startup.enabled);
        assert!(config.updates.auto_check);
        assert!(config.updates.last_check.is_none());
        assert!(config.updates.latest_version.is_none());
    }

    #[test]
    fn config_path_is_under_config_dir() {
        if let Some(path) = Config::path() {
            assert!(path.ends_with("statuslight/config.toml"));
        }
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = std::env::temp_dir().join("statuslight-test-round-trip");
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);

        let mut config = Config::default();
        config.slack.app_token = Some("xapp-test-token".to_string());
        config.slack.bot_token = Some("xoxb-test-token".to_string());
        config.slack.user_token = Some("xoxp-test-token-123".to_string());
        config.slack.events_enabled = true;
        config
            .slack
            .emoji_colors
            .insert(":no_entry:".to_string(), "#FF0000".to_string());
        config.slack.rules.push(SlackRule {
            name: "test".to_string(),
            event: "message.im".to_string(),
            from_user: None,
            contains: None,
            animation: "flash".to_string(),
            color: "#00FF00".to_string(),
            speed: 2.0,
            repeat: 3,
            duration_secs: None,
        });
        config.startup.enabled = true;
        config.updates.auto_check = false;
        config.updates.latest_version = Some("1.2.3".to_string());

        config.save_to(&path).expect("save failed");
        let loaded = Config::load_from(&path).expect("load failed");

        assert_eq!(
            loaded.slack.user_token.as_deref(),
            Some("xoxp-test-token-123")
        );
        assert_eq!(loaded.slack.app_token.as_deref(), Some("xapp-test-token"));
        assert_eq!(loaded.slack.bot_token.as_deref(), Some("xoxb-test-token"));
        assert!(loaded.slack.events_enabled);
        assert_eq!(
            loaded.slack.emoji_colors.get(":no_entry:").unwrap(),
            "#FF0000"
        );
        assert_eq!(loaded.slack.rules.len(), 1);
        assert_eq!(loaded.slack.rules[0].name, "test");
        assert!(loaded.startup.enabled);
        assert!(!loaded.updates.auto_check);
        assert_eq!(loaded.updates.latest_version.as_deref(), Some("1.2.3"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_returns_default() {
        let path = std::env::temp_dir()
            .join("statuslight-test-missing")
            .join("config.toml");
        let _ = std::fs::remove_file(&path);

        let config = Config::load_from(&path).expect("load failed");
        assert!(config.slack.user_token.is_none());
    }

    #[test]
    fn deserialize_partial_toml_with_token() {
        let toml_str = r#"
[slack]
user_token = "xoxp-partial"
"#;
        let config: Config = toml::from_str(toml_str).expect("parse failed");
        assert_eq!(config.slack.user_token.as_deref(), Some("xoxp-partial"));
        assert!(!config.startup.enabled);
    }

    #[test]
    fn legacy_token_migration() {
        let dir = std::env::temp_dir().join("statuslight-test-migration");
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Write a config with the legacy `token` field.
        std::fs::write(
            &path,
            r#"
[slack]
token = "xoxp-legacy-token"
"#,
        )
        .unwrap();

        let config = Config::load_from(&path).expect("load failed");
        // Legacy token should have been migrated to user_token.
        assert_eq!(
            config.slack.user_token.as_deref(),
            Some("xoxp-legacy-token")
        );
        assert!(config.slack.token.is_none());

        let _ = std::fs::remove_dir_all(&dir);
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
        assert!(config.slack.user_token.is_none());
        assert!(config.slack.app_token.is_none());
        assert!(config.slack.bot_token.is_none());
        assert!(config.updates.auto_check);
    }

    #[test]
    fn serialize_produces_valid_toml() {
        let mut config = Config::default();
        config.slack.user_token = Some("xoxp-serialize-test".to_string());

        let toml_str = toml::to_string_pretty(&config).expect("serialize failed");
        assert!(toml_str.contains("xoxp-serialize-test"));
        // Legacy fields (token, poll_interval_secs) should not be serialized.
        assert!(!toml_str.contains("poll_interval_secs"));

        // Verify round-trip through serialization.
        let parsed: Config = toml::from_str(&toml_str).expect("re-parse failed");
        assert_eq!(
            parsed.slack.user_token.as_deref(),
            Some("xoxp-serialize-test")
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_sets_restrictive_permissions() {
        let dir = std::env::temp_dir().join("statuslight-test-perms");
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
            .join("statuslight-test-mkdir")
            .join("nested");
        let path = dir.join("config.toml");
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("statuslight-test-mkdir"));

        let config = Config::default();
        config.save_to(&path).expect("save failed");
        assert!(path.exists());

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("statuslight-test-mkdir"));
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
        let dir = std::env::temp_dir().join("statuslight-test-custom-presets");
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
