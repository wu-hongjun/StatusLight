//! macOS LaunchAgent management for the Slicky daemon.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};
use slicky_core::Config;

const PLIST_LABEL: &str = "com.openslicky.daemon";

/// Return `~/Library/LaunchAgents/<label>.plist`.
fn plist_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("cannot determine home dir"))?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{PLIST_LABEL}.plist")))
}

/// Find the `slickyd` binary — check sibling of current exe first, then PATH.
fn find_slickyd() -> Result<PathBuf> {
    // Sibling of current executable.
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe.with_file_name("slickyd");
        if sibling.exists() {
            return Ok(sibling);
        }
    }

    // Fall back to `which slickyd`.
    let output = Command::new("which")
        .arg("slickyd")
        .output()
        .context("failed to run `which slickyd`")?;

    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Ok(PathBuf::from(path));
        }
    }

    anyhow::bail!("cannot find slickyd binary — install it first")
}

/// Escape a string for safe inclusion in XML text content.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Generate the LaunchAgent plist XML.
fn plist_contents(slickyd_path: &str) -> String {
    let escaped_path = xml_escape(slickyd_path);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{PLIST_LABEL}</string>

  <key>ProgramArguments</key>
  <array>
    <string>{escaped_path}</string>
  </array>

  <key>RunAtLoad</key>
  <true/>

  <key>KeepAlive</key>
  <true/>

  <key>StandardOutPath</key>
  <string>/tmp/slicky-daemon.log</string>

  <key>StandardErrorPath</key>
  <string>/tmp/slicky-daemon.log</string>
</dict>
</plist>
"#
    )
}

/// `slicky startup enable` — install LaunchAgent and start daemon.
pub fn enable() -> Result<()> {
    let slickyd = find_slickyd()?;
    let plist = plist_path()?;

    // Create LaunchAgents directory if needed.
    if let Some(parent) = plist.parent() {
        fs::create_dir_all(parent).context("failed to create LaunchAgents directory")?;
    }

    let contents = plist_contents(&slickyd.to_string_lossy());
    fs::write(&plist, contents).context("failed to write LaunchAgent plist")?;

    // Load and start.
    let status = Command::new("launchctl")
        .args(["load", "-w"])
        .arg(&plist)
        .status()
        .context("failed to run launchctl load")?;

    if !status.success() {
        anyhow::bail!("launchctl load failed (exit {})", status);
    }

    // Update config.
    let mut config = Config::load()?;
    config.startup.enabled = true;
    config.save()?;

    println!("Startup enabled — slickyd will start automatically on login.");
    println!("Plist: {}", plist.display());
    Ok(())
}

/// `slicky startup disable` — stop daemon and remove LaunchAgent.
pub fn disable() -> Result<()> {
    let plist = plist_path()?;

    if plist.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", "-w"])
            .arg(&plist)
            .status();
        fs::remove_file(&plist).context("failed to remove plist")?;
    }

    let mut config = Config::load()?;
    config.startup.enabled = false;
    config.save()?;

    println!("Startup disabled — LaunchAgent removed.");
    Ok(())
}

/// `slicky startup status` — show if enabled and if daemon is running.
pub fn status() -> Result<()> {
    let config = Config::load()?;
    let plist = plist_path()?;

    let installed = plist.exists();
    println!(
        "Startup: {}",
        if config.startup.enabled && installed {
            "enabled"
        } else {
            "disabled"
        }
    );

    // Check if daemon is running via launchctl.
    let output = Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output();

    let running = output.is_ok_and(|o| o.status.success());
    println!("Daemon: {}", if running { "running" } else { "stopped" });

    if !config.startup.enabled && installed {
        println!(
            "(plist exists but config says disabled — run `slicky startup enable` to reconcile)"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_escape_no_special_chars() {
        assert_eq!(
            xml_escape("/usr/local/bin/slickyd"),
            "/usr/local/bin/slickyd"
        );
    }

    #[test]
    fn xml_escape_ampersand() {
        assert_eq!(xml_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn xml_escape_angle_brackets() {
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
    }

    #[test]
    fn xml_escape_quotes() {
        assert_eq!(xml_escape(r#"say "hello""#), "say &quot;hello&quot;");
    }

    #[test]
    fn xml_escape_mixed() {
        assert_eq!(
            xml_escape(r#"a & b < c > d "e""#),
            "a &amp; b &lt; c &gt; d &quot;e&quot;"
        );
    }

    #[test]
    fn plist_contains_label() {
        let plist = plist_contents("/usr/local/bin/slickyd");
        assert!(plist.contains(PLIST_LABEL));
    }

    #[test]
    fn plist_contains_path() {
        let plist = plist_contents("/opt/slickyd");
        assert!(plist.contains("/opt/slickyd"));
    }

    #[test]
    fn plist_escapes_special_path() {
        let plist = plist_contents("/path/with <special> & \"chars\"");
        assert!(plist.contains("&lt;special&gt;"));
        assert!(plist.contains("&amp;"));
        assert!(plist.contains("&quot;chars&quot;"));
        assert!(!plist.contains("<special>"));
    }

    #[test]
    fn plist_has_run_at_load() {
        let plist = plist_contents("/usr/local/bin/slickyd");
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<true/>"));
    }

    #[test]
    fn plist_has_keep_alive() {
        let plist = plist_contents("/usr/local/bin/slickyd");
        assert!(plist.contains("<key>KeepAlive</key>"));
    }

    #[test]
    fn plist_has_log_paths() {
        let plist = plist_contents("/usr/local/bin/slickyd");
        assert!(plist.contains("/tmp/slicky-daemon.log"));
    }

    #[test]
    fn plist_is_valid_xml_declaration() {
        let plist = plist_contents("/usr/local/bin/slickyd");
        assert!(plist.starts_with("<?xml version=\"1.0\""));
    }

    #[test]
    fn plist_path_is_under_launch_agents() {
        if let Ok(path) = plist_path() {
            assert!(path.to_string_lossy().contains("Library/LaunchAgents"));
            assert!(path.to_string_lossy().ends_with(".plist"));
        }
    }
}
