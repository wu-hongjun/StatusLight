//! Async update-check for the daemon (uses `reqwest`).

use chrono::Utc;
use semver::Version;
use statuslight_core::Config;

const RELEASES_URL: &str = "https://api.github.com/repos/wu-hongjun/StatusLight/releases/latest";

/// Spawn a non-blocking update check if `auto_check` is enabled and
/// at least 24 hours have passed since the last check.
pub fn spawn_check_if_due() {
    tokio::spawn(async {
        if let Err(e) = check_once().await {
            log::warn!("Update check failed: {e}");
        }
    });
}

async fn check_once() -> anyhow::Result<()> {
    let mut config = tokio::task::spawn_blocking(Config::load).await??;

    if !config.updates.auto_check {
        return Ok(());
    }

    // Rate-limit: at most once per 24 hours.
    if let Some(last) = config.updates.last_check {
        let elapsed = Utc::now().signed_duration_since(last);
        if elapsed.num_hours() < 24 {
            return Ok(());
        }
    }

    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;

    let client = reqwest::Client::new();
    let resp = client
        .get(RELEASES_URL)
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "statuslight-daemon")
        .send()
        .await?;

    let json: serde_json::Value = resp.json().await?;

    let tag: &str = match json["tag_name"].as_str() {
        Some(t) => t,
        None => return Ok(()),
    };

    let version_str = tag.strip_prefix('v').unwrap_or(tag);

    config.updates.last_check = Some(Utc::now());
    if let Ok(latest) = Version::parse(version_str) {
        config.updates.latest_version = Some(latest.to_string());
        if latest > current {
            log::info!(
                "New version available: {latest} (current: {current}). \
                 Download: https://github.com/wu-hongjun/StatusLight/releases/tag/v{latest}"
            );
        }
    }

    tokio::task::spawn_blocking(move || config.save()).await??;

    Ok(())
}
