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

fn resolve_icon_id(title: &str, icon_id: u16) -> u16 {
    if icon_id >= 22 {
        return icon_id;
    }
    const BY_CHAR: [(u8, u8); 5] = [(b'T', 28), (b'S', 30), (b'R', 25), (b'C', 1), (b'B', 29)];
    let b = title.as_bytes();
    let c0 = b.first().copied().unwrap_or(0) & !0x20;
    if c0 == b'B' && (b.get(2).copied().unwrap_or(0) & !0x20) == b'A' {
        return 0xFF;
    }
    for &(ch, id) in &BY_CHAR {
        if c0 == ch {
            return id as u16;
        }
    }
    46
}

#[inline(always)]
fn icon_8_from_eff(eff: u16) -> &'static [u8; 8] {
    if eff == 0xFF {
        return gfx::icons::ICON_ANTENNA;
    }
    if eff == 1 {
        return gfx::icons::ICON_KEYPAD;
    }
    if eff >= 22 {
        if let Some(bmp) = gfx::icons::SYSTEM_ICONS_8.get((eff - 22) as usize) {
            return bmp;
        }
    }
    gfx::icons::ICON_APPS
}

/// Resolves only the 8x8 monochrome icon bitmap without referencing 16x16 atlases.
#[inline(never)]
pub fn package_icon_8(title: &str, icon_id: u16) -> &'static [u8; 8] {
    icon_8_from_eff(resolve_icon_id(title, icon_id))
}

/// Resolves 8x8 and 16x16 icons for a packaged `.krx` entry using its embedded
/// `icon_id` (`SYSTEM_ICONS_8` offset `22 + idx`, e.g. `30` = `ICON_APP_ABOUT`),
/// falling back to title matching for packages built with the default `icon_id <= 1`.
pub fn package_icons(title: &str, icon_id: u16) -> (&'static [u8; 8], &'static [u8; 32]) {
    const MAP_16: [(u8, &'static [u8; 32]); 7] = [
        (0xFF, &gfx::icons::ICON_16_SCANNER),
        (25, &gfx::icons::ICON_16_RADIO),
        (26, &gfx::icons::ICON_16_SETTINGS),
        (38, &gfx::icons::ICON_16_SETTINGS),
        (28, &gfx::icons::ICON_16_FLASHLIGHT),
        (29, &gfx::icons::ICON_16_DIAGNOSTICS),
        (30, &gfx::icons::ICON_16_INFO),
    ];
    let eff = resolve_icon_id(title, icon_id);
    let mut icon_16: &'static [u8; 32] = &gfx::icons::ICON_16_APPS;
    for &(id, bmp) in &MAP_16 {
        if eff == id as u16 {
            icon_16 = bmp;
            break;
        }
    }
    (icon_8_from_eff(eff), icon_16)
}
