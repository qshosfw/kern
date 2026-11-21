//! Applications Settings Page & Package Icon Resolution for OS Settings.

use super::page::{ChoiceId, SettingDef, SettingField, setting_page};

pub const LAUNCHER_STYLE_OPTIONS: &str = "Grid\0List\0";
pub const STARTUP_APP_OPTIONS: &str = "Home\0Radio\0";

setting_page!(pub APPS_PAGE = [
    ("Style", SettingDef::choice(ChoiceId::LauncherStyle, SettingField::LAUNCHER_STYLE)),
    ("Key Badges", SettingDef::toggle(SettingField::SHOW_NUMBERS)),
    ("App Titles", SettingDef::toggle(SettingField::SHOW_LABELS)),
    ("Startup App", SettingDef::choice(ChoiceId::StartupApp, SettingField::STARTUP_APP)),
]);

#[inline(always)]
fn resolve_icon_id(icon_id: u16) -> u8 {
    u8::try_from(icon_id).unwrap_or(::abi::icon::APPS)
}

/// Resolves only the 8x8 monochrome fallback icon bitmap without referencing 16x16 atlases.
#[inline(never)]
pub fn package_icon_8(_title: &str, icon_id: u16) -> &'static [u8; 8] {
    gfx::icons::system_icon(resolve_icon_id(icon_id)).unwrap_or(gfx::icons::ICON_APPS)
}

/// Resolves fallback 8x8 and 16x16 icons for a packaged `.krx` entry when `SECTION_ASSETS`
/// does not embed custom bitmaps; unknown ids show the generic puzzle piece.
pub fn package_icons(title: &str, icon_id: u16) -> (&'static [u8; 8], &'static [u8; 32]) {
    let eff = resolve_icon_id(icon_id);
    (package_icon_8(title, icon_id), gfx::icons::icon_16(eff))
}
