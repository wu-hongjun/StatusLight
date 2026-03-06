//! `statuslight-core` — Core library for controlling USB status lights.
//!
//! Provides color definitions, HID protocol encoding, and device communication
//! for USB status lights. Includes the Slicky driver for Lexcelon Slicky-1.0
//! devices (VID 0x04D8, PID 0xEC24).

pub mod animation;
pub mod color;
pub mod config;
pub mod device;
pub mod error;
pub mod protocol;

pub use animation::AnimationType;
pub use color::{Color, Preset};
pub use config::{Config, CustomPreset, SlackRule};
pub use device::{DeviceInfo, HidSlickyDevice, StatusLightDevice};
pub use error::{Result, StatusLightError};
