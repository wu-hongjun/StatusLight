//! Blocking update-check for the CLI (uses `ureq`).

use anyhow::{Context, Result};
use chrono::Utc;
use semver::Version;
use serde::Serialize;
use slicky_core::Config;
use std::process::Command;

const RELEASES_URL: &str = "https://api.github.com/repos/wu-hongjun/OpenSilcky/releases/latest";

/// JSON output for `slicky update status`.
#[derive(Debug, Serialize)]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub last_check: Option<String>,
    pub download_url: Option<String>,
}

/// JSON output for `slicky update install`.
#[derive(Debug, Serialize)]
pub struct InstallResult {
    pub status: String,
    pub version: Option<String>,
    pub error: Option<String>,
}

/// `slicky update check` — manual check, always hits API.
pub fn check() -> Result<()> {
    let mut config = Config::load()?;
    let current = current_version()?;

    println!("Current version: {current}");
    println!("Checking for updates...");

    match fetch_latest()? {
        Some(latest) => {
            config.updates.last_check = Some(Utc::now());
            config.updates.latest_version = Some(latest.to_string());
            config.save()?;

            if latest > current {
                println!("New version available: {latest}");
                println!(
                    "Download: https://github.com/wu-hongjun/OpenSilcky/releases/tag/v{latest}"
                );
            } else {
                println!("You are up to date.");
            }
        }
        None => {
            println!("No releases found.");
        }
    }

    Ok(())
}

/// `slicky update status` — reads cached config, outputs JSON (no network).
pub fn status() -> Result<()> {
    let config = Config::load()?;
    let current = current_version()?;

    let latest_str = config.updates.latest_version.as_deref();
    let update_available = match latest_str {
        Some(v) => Version::parse(v).map(|l| l > current).unwrap_or(false),
        None => false,
    };

    let download_url = if update_available {
        latest_str.map(|v| {
            format!(
                "https://github.com/wu-hongjun/OpenSilcky/releases/download/v{v}/OpenSlicky.dmg"
            )
        })
    } else {
        None
    };

    let status = UpdateStatus {
        current_version: current.to_string(),
        latest_version: config.updates.latest_version.clone(),
        update_available,
        last_check: config.updates.last_check.map(|t| t.to_rfc3339()),
        download_url,
    };

    println!("{}", serde_json::to_string(&status)?);
    Ok(())
}

/// `slicky update install` — downloads DMG, replaces app, restarts daemon.
pub fn install() -> Result<()> {
    let current = current_version()?;

    // Fetch latest from GitHub API.
    let latest = match fetch_latest()? {
        Some(v) => v,
        None => {
            let result = InstallResult {
                status: "error".into(),
                version: None,
                error: Some("No releases found on GitHub".into()),
            };
            println!("{}", serde_json::to_string(&result)?);
            return Ok(());
        }
    };

    if latest <= current {
        let result = InstallResult {
            status: "up_to_date".into(),
            version: Some(current.to_string()),
            error: None,
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    let version_str = latest.to_string();
    let dmg_url = format!(
        "https://github.com/wu-hongjun/OpenSilcky/releases/download/v{version_str}/OpenSlicky.dmg"
    );
    let dmg_path = format!("/tmp/OpenSlicky-update-{version_str}.dmg");

    // Download DMG.
    if let Err(e) = download_file(&dmg_url, &dmg_path) {
        let result = InstallResult {
            status: "error".into(),
            version: Some(version_str),
            error: Some(format!("Download failed: {e}")),
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    // Mount DMG.
    let mount_output = Command::new("hdiutil")
        .args(["attach", &dmg_path, "-nobrowse", "-plist"])
        .output()
        .context("failed to run hdiutil")?;

    if !mount_output.status.success() {
        let _ = std::fs::remove_file(&dmg_path);
        let result = InstallResult {
            status: "error".into(),
            version: Some(version_str),
            error: Some("Failed to mount DMG".into()),
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    let plist_str = String::from_utf8_lossy(&mount_output.stdout);
    let mount_point = match parse_mount_point(&plist_str) {
        Some(mp) => mp,
        None => {
            let _ = std::fs::remove_file(&dmg_path);
            let result = InstallResult {
                status: "error".into(),
                version: Some(version_str),
                error: Some("Failed to parse DMG mount point".into()),
            };
            println!("{}", serde_json::to_string(&result)?);
            return Ok(());
        }
    };

    // Validate mount point starts with /Volumes/ to prevent path traversal.
    if !mount_point.starts_with("/Volumes/") {
        let _ = Command::new("hdiutil")
            .args(["detach", &mount_point, "-quiet"])
            .status();
        let _ = std::fs::remove_file(&dmg_path);
        let result = InstallResult {
            status: "error".into(),
            version: Some(version_str),
            error: Some("Invalid DMG mount point".into()),
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    // Copy new app from mounted volume using atomic swap with rollback.
    let source_app = format!("{mount_point}/OpenSlicky.app");
    let dest_app = "/Applications/OpenSlicky.app";
    let backup_app = "/Applications/OpenSlicky.app.bak";
    let staging_app = "/Applications/OpenSlicky.app.new";

    // 1. Copy new app to staging location.
    let _ = Command::new("rm").args(["-rf", staging_app]).status();
    let cp_status = Command::new("cp")
        .args(["-R", &source_app, staging_app])
        .status()
        .context("failed to copy new app to staging")?;

    if !cp_status.success() {
        let _ = Command::new("hdiutil")
            .args(["detach", &mount_point, "-quiet"])
            .status();
        let _ = std::fs::remove_file(&dmg_path);
        let result = InstallResult {
            status: "error".into(),
            version: Some(version_str),
            error: Some("permission_denied".into()),
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    // 2. Rename existing app to backup.
    let dest_exists = std::path::Path::new(dest_app).exists();
    if dest_exists && std::fs::rename(dest_app, backup_app).is_err() {
        // rename failed — clean up staging and report.
        let _ = Command::new("rm").args(["-rf", staging_app]).status();
        let _ = Command::new("hdiutil")
            .args(["detach", &mount_point, "-quiet"])
            .status();
        let _ = std::fs::remove_file(&dmg_path);
        let result = InstallResult {
            status: "error".into(),
            version: Some(version_str),
            error: Some("permission_denied".into()),
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    // 3. Rename staging to final destination.
    if std::fs::rename(staging_app, dest_app).is_err() {
        // Restore backup if we moved the original.
        if dest_exists {
            let _ = std::fs::rename(backup_app, dest_app);
        }
        let _ = Command::new("hdiutil")
            .args(["detach", &mount_point, "-quiet"])
            .status();
        let _ = std::fs::remove_file(&dmg_path);
        let result = InstallResult {
            status: "error".into(),
            version: Some(version_str),
            error: Some("permission_denied".into()),
        };
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }

    // 4. Remove backup (best effort).
    let _ = Command::new("rm").args(["-rf", backup_app]).status();

    // Unmount and clean up.
    let _ = Command::new("hdiutil")
        .args(["detach", &mount_point, "-quiet"])
        .status();
    let _ = std::fs::remove_file(&dmg_path);

    // Restart daemon (LaunchAgent KeepAlive will restart it with the new binary).
    let _ = Command::new("launchctl")
        .args(["stop", "com.openslicky.daemon"])
        .status();

    // Update config with the new version info.
    if let Ok(mut config) = Config::load() {
        config.updates.last_check = Some(Utc::now());
        config.updates.latest_version = Some(version_str.clone());
        let _ = config.save();
    }

    let result = InstallResult {
        status: "installed".into(),
        version: Some(version_str),
        error: None,
    };
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

/// Parse `CARGO_PKG_VERSION` as a semver `Version`.
pub fn current_version() -> Result<Version> {
    Version::parse(env!("CARGO_PKG_VERSION")).context("failed to parse CARGO_PKG_VERSION")
}

/// Fetch the latest release tag from GitHub (blocking).
pub fn fetch_latest() -> Result<Option<Version>> {
    let resp = ureq::get(RELEASES_URL)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "openslicky-cli")
        .call()
        .context("failed to fetch latest release")?;

    let json: serde_json::Value = serde_json::from_reader(resp.into_body().into_reader())
        .context("failed to parse release JSON")?;

    let tag: &str = match json["tag_name"].as_str() {
        Some(t) => t,
        None => return Ok(None),
    };

    // Strip leading 'v' if present.
    let version_str = tag.strip_prefix('v').unwrap_or(tag);
    match Version::parse(version_str) {
        Ok(v) => Ok(Some(v)),
        Err(_) => Ok(None),
    }
}

/// Download a file from `url` to `dest` path (streams to disk).
fn download_file(url: &str, dest: &str) -> Result<()> {
    let resp = ureq::get(url)
        .header("User-Agent", "openslicky-cli")
        .call()
        .context("failed to download file")?;

    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(dest).context("failed to create download destination")?;
    std::io::copy(&mut reader, &mut file).context("failed to write downloaded file")?;
    Ok(())
}

/// Parse the mount point from `hdiutil attach -plist` output.
///
/// The plist contains a `system-entities` array; we look for the entry
/// with a `mount-point` key.
fn parse_mount_point(plist_output: &str) -> Option<String> {
    // Look for <key>mount-point</key> followed by <string>...</string>
    let mut lines = plist_output.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed == "<key>mount-point</key>" {
            if let Some(next_line) = lines.next() {
                let next_trimmed = next_line.trim();
                if let Some(rest) = next_trimmed.strip_prefix("<string>") {
                    if let Some(value) = rest.strip_suffix("</string>") {
                        return Some(value.to_string());
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_valid_semver() {
        let v = current_version().expect("should parse CARGO_PKG_VERSION");
        // Should match the version in Cargo.toml.
        assert_eq!(v, Version::new(0, 1, 5));
    }

    #[test]
    fn version_comparison_newer() {
        let current = Version::new(0, 1, 0);
        let latest = Version::new(0, 2, 0);
        assert!(latest > current);
    }

    #[test]
    fn version_comparison_same() {
        let current = Version::new(0, 1, 0);
        let latest = Version::new(0, 1, 0);
        assert!(!(latest > current));
    }

    #[test]
    fn version_comparison_older() {
        let current = Version::new(1, 0, 0);
        let latest = Version::new(0, 9, 0);
        assert!(!(latest > current));
    }

    #[test]
    fn strip_v_prefix() {
        let tag = "v1.2.3";
        let version_str = tag.strip_prefix('v').unwrap_or(tag);
        let v = Version::parse(version_str).unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn no_v_prefix() {
        let tag = "1.2.3";
        let version_str = tag.strip_prefix('v').unwrap_or(tag);
        let v = Version::parse(version_str).unwrap();
        assert_eq!(v, Version::new(1, 2, 3));
    }

    #[test]
    fn invalid_version_tag() {
        let tag = "not-a-version";
        let version_str = tag.strip_prefix('v').unwrap_or(tag);
        assert!(Version::parse(version_str).is_err());
    }

    #[test]
    fn parse_mount_point_from_plist() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>system-entities</key>
    <array>
        <dict>
            <key>content-hint</key>
            <string>Apple_HFS</string>
            <key>dev-entry</key>
            <string>/dev/disk4s2</string>
            <key>mount-point</key>
            <string>/Volumes/OpenSlicky</string>
            <key>potentially-mountable</key>
            <true/>
        </dict>
    </array>
</dict>
</plist>"#;
        assert_eq!(
            parse_mount_point(plist),
            Some("/Volumes/OpenSlicky".to_string())
        );
    }

    #[test]
    fn parse_mount_point_missing() {
        let plist = r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
<dict>
    <key>system-entities</key>
    <array>
        <dict>
            <key>dev-entry</key>
            <string>/dev/disk4s2</string>
        </dict>
    </array>
</dict>
</plist>"#;
        assert_eq!(parse_mount_point(plist), None);
    }

    #[test]
    fn update_status_json_serialization() {
        let status = UpdateStatus {
            current_version: "0.1.4".into(),
            latest_version: Some("0.2.0".into()),
            update_available: true,
            last_check: Some("2026-01-01T00:00:00+00:00".into()),
            download_url: Some("https://example.com/OpenSlicky.dmg".into()),
        };
        let json = serde_json::to_string(&status).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["current_version"], "0.1.4");
        assert_eq!(parsed["update_available"], true);
        assert_eq!(parsed["latest_version"], "0.2.0");
    }

    #[test]
    fn install_result_json_serialization() {
        let result = InstallResult {
            status: "installed".into(),
            version: Some("0.2.0".into()),
            error: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "installed");
        assert_eq!(parsed["version"], "0.2.0");
        assert!(parsed["error"].is_null());
    }

    #[test]
    fn install_result_error_json() {
        let result = InstallResult {
            status: "error".into(),
            version: None,
            error: Some("Download failed".into()),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "error");
        assert!(parsed["version"].is_null());
        assert_eq!(parsed["error"], "Download failed");
    }
}
