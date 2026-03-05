//! Blocking update-check for the CLI (uses `ureq`).

use anyhow::{Context, Result};
use chrono::Utc;
use semver::Version;
use slicky_core::Config;

const RELEASES_URL: &str = "https://api.github.com/repos/wu-hongjun/OpenSilcky/releases/latest";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_is_valid_semver() {
        let v = current_version().expect("should parse CARGO_PKG_VERSION");
        // Version 0.1.0 as defined in Cargo.toml.
        assert_eq!(v, Version::new(0, 1, 0));
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
}
