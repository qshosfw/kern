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

/// Effective `abi::icon` identifier for a package: its declared 8x8 system glyph,
/// or a title-initial guess for packages built with a status-range default.
fn resolve_icon_id(title: &str, icon_id: u16) -> u8 {
    use ::abi::icon;
    if icon_id >= icon::SYSTEM_BASE as u16 {
        return u8::try_from(icon_id).unwrap_or(u8::MAX);
    }
    const BY_CHAR: [(u8, u8); 4] = [
        (b'T', icon::APP_TORCH),
        (b'S', icon::APP_ABOUT),
        (b'R', icon::APP_RADIO),
        (b'B', icon::APP_DIAG),
    ];
    let b = title.as_bytes();
    let c0 = b.first().copied().unwrap_or(0) & !0x20;
    if c0 == b'B' && (b.get(2).copied().unwrap_or(0) & !0x20) == b'A' {
        return icon::BEACON_TOWER;
    }
    for &(ch, id) in &BY_CHAR {
        if c0 == ch {
            return id;
        }
    }
    icon::APPS
}

#[inline(always)]
fn icon_8_from_eff(eff: u8) -> &'static [u8; 8] {
    gfx::icons::system_icon(eff).unwrap_or(gfx::icons::ICON_APPS)
}

/// Resolves only the 8x8 monochrome icon bitmap without referencing 16x16 atlases.
#[inline(never)]
pub fn package_icon_8(title: &str, icon_id: u16) -> &'static [u8; 8] {
    icon_8_from_eff(resolve_icon_id(title, icon_id))
}

/// Resolves 8x8 and 16x16 icons for a packaged `.krx` entry from its embedded
/// `abi::icon` identifier (e.g. `APP_ABOUT`), falling back to title matching for
/// packages built with a status-range default; unknown ids show the puzzle piece.
pub fn package_icons(title: &str, icon_id: u16) -> (&'static [u8; 8], &'static [u8; 32]) {
    let eff = resolve_icon_id(title, icon_id);
    (icon_8_from_eff(eff), gfx::icons::icon_16(eff))
}
