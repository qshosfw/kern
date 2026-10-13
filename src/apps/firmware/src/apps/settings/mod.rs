//! Industrial System Configuration & Settings Application
//! Provides modular submodules for Display, Audio, Buttons, Battery, and System.

pub mod app;
pub mod apps;
pub mod asset_chooser;
pub mod audio;
pub mod battery;
pub mod buttons;
pub mod device;
pub mod display;
pub mod options;
pub mod page;
pub mod security;
pub mod system;

pub use app::SettingsApp;
pub use page::*;
