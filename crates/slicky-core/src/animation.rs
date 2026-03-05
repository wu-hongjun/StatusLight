//! Software-driven animation types and frame computation.
//!
//! All animations are pure functions: given elapsed time and parameters,
//! they return a [`Color`] for the current frame. The host drives the
//! animation loop by calling [`AnimationType::frame()`] at ~30 FPS and
//! writing each result to the device via HID.

use serde::{Deserialize, Serialize};

use crate::color::Color;

/// Available animation patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationType {
    /// Smooth sine-wave breathing (4 s period).
    Breathing,
    /// Hard on/off blink (1 s period).
    Flash,
    /// Morse code SOS pattern (... --- ...) with 3 s pause.
    Sos,
    /// Sharp rise then exponential decay (2 s period).
    Pulse,
    /// Cycle through the full hue spectrum (6 s period).
    Rainbow,
    /// Smooth transition between two colors (4 s period).
    Transition,
}

impl AnimationType {
    /// Parse an animation type from a string (case-insensitive).
    pub fn from_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "breathing" => Some(Self::Breathing),
            "flash" => Some(Self::Flash),
            "sos" => Some(Self::Sos),
            "pulse" => Some(Self::Pulse),
            "rainbow" => Some(Self::Rainbow),
            "transition" => Some(Self::Transition),
            _ => None,
        }
    }

    /// The lowercase name of this animation type.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Breathing => "breathing",
            Self::Flash => "flash",
            Self::Sos => "sos",
            Self::Pulse => "pulse",
            Self::Rainbow => "rainbow",
            Self::Transition => "transition",
        }
    }

    /// All available animation types.
    pub fn all() -> &'static [AnimationType] {
        &[
            Self::Breathing,
            Self::Flash,
            Self::Sos,
            Self::Pulse,
            Self::Rainbow,
            Self::Transition,
        ]
    }

    /// Compute the color for one animation frame.
    ///
    /// - `elapsed_secs`: wall-clock seconds since animation started
    /// - `speed`: multiplier (1.0 = normal speed)
    /// - `color`: base color (used by most types except Rainbow)
    /// - `color2`: second color (used by Transition only)
    pub fn frame(&self, elapsed_secs: f64, speed: f64, color: Color, color2: Color) -> Color {
        let t = elapsed_secs * speed;
        match self {
            Self::Breathing => breathing_frame(t, color),
            Self::Flash => flash_frame(t, color),
            Self::Sos => sos_frame(t, color),
            Self::Pulse => pulse_frame(t, color),
            Self::Rainbow => rainbow_frame(t),
            Self::Transition => transition_frame(t, color, color2),
        }
    }
}

/// Breathing: `brightness = (1 - cos(2*pi*t/4)) / 2`, min 0.05.
fn breathing_frame(t: f64, color: Color) -> Color {
    let period = 4.0;
    let brightness = (1.0 - (2.0 * std::f64::consts::PI * t / period).cos()) / 2.0;
    let brightness = brightness.max(0.05);
    color.scale_brightness(brightness)
}

/// Flash: on for first half of each 1 s period, off for second half.
fn flash_frame(t: f64, color: Color) -> Color {
    let phase = t % 1.0;
    if phase < 0.5 {
        color
    } else {
        Color::off()
    }
}

/// SOS: Morse `... --- ...` then 3 s pause.
///
/// Dot = 0.2 s on, dash = 0.6 s on, inter-element gap = 0.2 s,
/// inter-letter gap = 0.6 s, word gap = 3.0 s.
fn sos_frame(t: f64, color: Color) -> Color {
    // Total cycle: S(1.4) + gap(0.6) + O(2.2) + gap(0.6) + S(1.4) + pause(3.0) = 9.2s
    // S = dot(0.2) gap(0.2) dot(0.2) gap(0.2) dot(0.2) = 1.0s of elements, but last gap omitted = 0.8s+0.2=1.0
    // Actually: dot gap dot gap dot = 0.2+0.2+0.2+0.2+0.2 = 1.0, but last element has no trailing gap before letter gap
    // Let's define precise timings:
    //   S: [on 0.2][off 0.2][on 0.2][off 0.2][on 0.2] = 1.0s
    //   letter gap: 0.6s
    //   O: [on 0.6][off 0.2][on 0.6][off 0.2][on 0.6] = 2.2s
    //   letter gap: 0.6s
    //   S: [on 0.2][off 0.2][on 0.2][off 0.2][on 0.2] = 1.0s
    //   word gap: 3.0s
    //   Total = 1.0 + 0.6 + 2.2 + 0.6 + 1.0 + 3.0 = 8.4s

    let total = 8.4;
    let phase = t % total;

    // Build a timeline of (end_time, is_on) segments
    let segments: &[(f64, bool)] = &[
        // S
        (0.2, true),
        (0.4, false),
        (0.6, true),
        (0.8, false),
        (1.0, true),
        // letter gap
        (1.6, false),
        // O
        (2.2, true),
        (2.4, false),
        (3.0, true),
        (3.2, false),
        (3.8, true),
        // letter gap
        (4.4, false),
        // S
        (4.6, true),
        (4.8, false),
        (5.0, true),
        (5.2, false),
        (5.4, true),
        // word gap
        (8.4, false),
    ];

    for &(end, on) in segments {
        if phase < end {
            return if on { color } else { Color::off() };
        }
    }
    Color::off()
}

/// Pulse: sharp rise 0->1 in first 20%, exponential decay in remaining 80%.
fn pulse_frame(t: f64, color: Color) -> Color {
    let period = 2.0;
    let phase = (t % period) / period; // 0..1

    let brightness = if phase < 0.2 {
        // Linear rise
        phase / 0.2
    } else {
        // Exponential decay: e^(-4 * normalized_decay_position)
        let decay_pos = (phase - 0.2) / 0.8;
        (-4.0 * decay_pos).exp()
    };

    color.scale_brightness(brightness.max(0.0))
}

/// Rainbow: cycle through full hue spectrum over 6 seconds.
fn rainbow_frame(t: f64) -> Color {
    let period = 6.0;
    let hue = ((t % period) / period) * 360.0;
    Color::from_hsv(hue, 1.0, 1.0)
}

/// Transition: smooth oscillation between two colors using cosine interpolation.
fn transition_frame(t: f64, c1: Color, c2: Color) -> Color {
    let period = 4.0;
    let factor = (1.0 - (2.0 * std::f64::consts::PI * t / period).cos()) / 2.0;
    c1.lerp(c2, factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breathing_starts_dim() {
        let c = AnimationType::Breathing.frame(0.0, 1.0, Color::new(255, 255, 255), Color::off());
        // At t=0, cos(0)=1 so brightness = (1-1)/2 = 0, clamped to 0.05
        assert_eq!(c, Color::new(255, 255, 255).scale_brightness(0.05));
    }

    #[test]
    fn breathing_peaks_at_half_period() {
        let c = AnimationType::Breathing.frame(2.0, 1.0, Color::new(255, 255, 255), Color::off());
        // At t=2 (half of 4s period), cos(pi) = -1, brightness = (1+1)/2 = 1.0
        assert_eq!(c, Color::new(255, 255, 255));
    }

    #[test]
    fn flash_on_then_off() {
        let white = Color::new(255, 255, 255);
        let on = AnimationType::Flash.frame(0.0, 1.0, white, Color::off());
        assert_eq!(on, white);
        let off = AnimationType::Flash.frame(0.6, 1.0, white, Color::off());
        assert_eq!(off, Color::off());
    }

    #[test]
    fn sos_starts_on() {
        let c = AnimationType::Sos.frame(0.0, 1.0, Color::new(255, 255, 255), Color::off());
        assert_eq!(c, Color::new(255, 255, 255));
    }

    #[test]
    fn sos_gap_is_off() {
        let c = AnimationType::Sos.frame(0.3, 1.0, Color::new(255, 255, 255), Color::off());
        assert_eq!(c, Color::off());
    }

    #[test]
    fn pulse_starts_dark() {
        let c = AnimationType::Pulse.frame(0.0, 1.0, Color::new(255, 255, 255), Color::off());
        assert_eq!(c, Color::off());
    }

    #[test]
    fn pulse_peaks_at_rise_end() {
        // At phase=0.2 of period 2s -> t=0.4s
        let c = AnimationType::Pulse.frame(0.4, 1.0, Color::new(255, 0, 0), Color::off());
        // brightness = 0.2/0.2 = 1.0 (just at the boundary, starts decay)
        // Actually at 0.4 exactly, phase = 0.4/2.0 = 0.2, which hits the else branch
        // decay_pos = 0.0, exp(0) = 1.0
        assert_eq!(c, Color::new(255, 0, 0));
    }

    #[test]
    fn rainbow_red_at_start() {
        let c = AnimationType::Rainbow.frame(0.0, 1.0, Color::off(), Color::off());
        assert_eq!(c, Color::new(255, 0, 0));
    }

    #[test]
    fn rainbow_varies_over_time() {
        let c1 = AnimationType::Rainbow.frame(0.0, 1.0, Color::off(), Color::off());
        let c2 = AnimationType::Rainbow.frame(2.0, 1.0, Color::off(), Color::off());
        assert_ne!(
            c1, c2,
            "rainbow should produce different colors at different times"
        );
    }

    #[test]
    fn transition_starts_at_first_color() {
        let a = Color::new(255, 0, 0);
        let b = Color::new(0, 0, 255);
        let c = AnimationType::Transition.frame(0.0, 1.0, a, b);
        // At t=0, factor = (1-cos(0))/2 = 0, so result = a
        assert_eq!(c, a);
    }

    #[test]
    fn transition_midpoint_is_blend() {
        let a = Color::new(255, 0, 0);
        let b = Color::new(0, 0, 255);
        let c = AnimationType::Transition.frame(2.0, 1.0, a, b);
        // At t=2 (half of 4s), factor = (1-cos(pi))/2 = 1.0, so result = b
        assert_eq!(c, b);
    }

    #[test]
    fn speed_multiplier_affects_timing() {
        let white = Color::new(255, 255, 255);
        // Flash at 2x speed: period effectively becomes 0.5s
        let on = AnimationType::Flash.frame(0.0, 2.0, white, Color::off());
        assert_eq!(on, white);
        // At 0.3s with 2x speed, effective t=0.6, phase=0.6 -> off
        let off = AnimationType::Flash.frame(0.3, 2.0, white, Color::off());
        assert_eq!(off, Color::off());
    }

    #[test]
    fn from_name_round_trip() {
        for anim in AnimationType::all() {
            let parsed = AnimationType::from_name(anim.name()).unwrap();
            assert_eq!(*anim, parsed);
        }
    }

    #[test]
    fn from_name_case_insensitive() {
        assert_eq!(
            AnimationType::from_name("BREATHING"),
            Some(AnimationType::Breathing)
        );
        assert_eq!(
            AnimationType::from_name("Flash"),
            Some(AnimationType::Flash)
        );
    }

    #[test]
    fn from_name_unknown() {
        assert_eq!(AnimationType::from_name("nonexistent"), None);
    }
}
