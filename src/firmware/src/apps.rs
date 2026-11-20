//! Modular OS Application Registry & Domain Exports

pub mod launcher;
pub mod settings;
pub mod shortcut;
pub mod vfo;

pub mod registry;

pub use crate::data::file_header::{FILE_ID_APP_BUNDLE, FILE_ID_KRX};
pub use launcher::LauncherApp;
pub use registry::*;
pub use settings::SettingsApp;
pub use shortcut::Shortcut;
pub use vfo::VfoApp;
