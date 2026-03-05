//! `slicky-core` — Core library for controlling Slicky USB status lights.
//!
//! Provides color definitions, HID protocol encoding, and device communication
//! for Lexcelon Slicky-1.0 USB status lights (VID 0x04D8, PID 0xEC24).

pub mod animation;
pub mod color;
pub mod config;
pub mod device;
pub mod error;
pub mod protocol;

pub use animation::AnimationType;
pub use color::{Color, Preset};
pub use config::{Config, CustomPreset};
pub use device::{DeviceInfo, HidSlickyDevice, SlickyDevice};
pub use error::{Result, SlickyError};
