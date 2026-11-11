//! Industrial System Configuration & Settings Application
//! Provides modular submodules for Display, Audio, Buttons, Battery, and System.

pub mod app;
pub mod apps;
#[cfg(feature = "boot_assets")]
pub mod asset_chooser;
pub mod audio;
pub mod battery;
pub mod buttons;
pub mod device;
pub mod dialog;
pub mod display;
pub mod metric;
pub mod nav;
pub mod options;
pub mod page;
#[cfg(feature = "passcode")]
pub mod security;
pub mod system;

pub use app::SettingsApp;
pub use page::*;
