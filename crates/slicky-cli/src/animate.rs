//! CLI handler for the `slicky animate` command.
//!
//! Runs a blocking 30 FPS animation loop that sends color frames to the
//! device via HID. Exits cleanly on Ctrl-C (SIGTERM/SIGINT).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use slicky_core::{AnimationType, Color};

/// Target frame interval (~30 FPS).
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Run a blocking animation loop until interrupted.
pub fn run(animation: AnimationType, color: Color, color2: Color, speed: f64) -> Result<()> {
    let device = crate::daemon_client::DeviceProxy::open()?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .context("failed to set Ctrl-C handler")?;

    println!(
        "Playing {} animation (speed {:.1}x) — press Ctrl-C to stop",
        animation.name(),
        speed
    );

    let start = Instant::now();

    while running.load(Ordering::SeqCst) {
        let elapsed = start.elapsed().as_secs_f64();
        let frame_color = animation.frame(elapsed, speed, color, color2);
        device
            .set_color(frame_color)
            .context("failed to write frame")?;

        std::thread::sleep(FRAME_INTERVAL);
    }

    // Turn off the light on exit for a clean stop.
    let _ = device.off();
    println!("\nAnimation stopped");

    Ok(())
}
