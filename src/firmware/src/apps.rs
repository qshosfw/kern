//! Modular OS Application Registry & Domain Exports

pub mod launcher;
pub mod settings;
pub mod shortcut;
#[cfg(any(test, feature = "resident_vfo"))]
pub mod vfo;
#[cfg(not(any(test, feature = "resident_vfo")))]
pub mod vfo_fallback;

pub mod registry;

pub use crate::data::file_header::{FILE_ID_APP_BUNDLE, FILE_ID_KRX};
pub use launcher::LauncherApp;
pub use registry::*;
pub use settings::SettingsApp;
pub use shortcut::Shortcut;
#[cfg(any(test, feature = "resident_vfo"))]
pub use vfo::VfoApp;
#[cfg(not(any(test, feature = "resident_vfo")))]
pub use vfo_fallback::VfoApp;
