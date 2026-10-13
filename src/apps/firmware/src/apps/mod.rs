//! Modular OS Application Registry & Domain Exports

pub mod digmode;
#[cfg(feature = "filesystem")]
pub mod explorer;
pub mod launcher;
pub mod settings;
pub mod shortcut;
pub mod vfo;

pub mod registry;

pub use crate::data::app_bundle::{AppHeader, FILE_ID_APP_BUNDLE, FILE_ID_KRX};
#[cfg(feature = "filesystem")]
pub use explorer::ExplorerApp;
pub use launcher::LauncherApp;
pub use registry::*;
pub use settings::SettingsApp;
pub use shortcut::Shortcut;
pub use vfo::VfoApp;

/// Constants for well-known app IDs to allow for quick shortcut routing
#[allow(dead_code)]
pub const APP_ID_MIGRATION: usize = 4;
