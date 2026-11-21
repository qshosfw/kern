//! Monochromatic 5px, 8x8 & 16x16 icon glyph bitmaps for status bars, lists & apps.
//!
//! Drawn for the ST7565 panel whose pixels are ~1.3x taller than wide: round
//! shapes are drawn wider than tall and steep diagonals are avoided.
//!
//! Artwork lives as readable ASCII grids in the submodules and is packed at
//! compile time by [`art`]:
//! - [`status`]: the 5 px status-bar and 8x8 hold-sheet rendition of every [`Icon`].
//! - [`system`]: 8x8 list/menu/file glyphs addressed by `abi::icon` identifiers.
//! - [`app`]: 16x16 launcher artwork addressed by [`Launcher`] slot.

mod app;
mod art;
mod status;
mod system;

use crate::draw;
use crate::{Canvas, Color};
use abi::icon;

pub use app::ICONS_16;
pub use app::Launcher;
pub use kernel::ui::{Icon, StatusGlyph};
pub use system::SYSTEM_ICONS_8;

pub const ICON_WIDTH: u8 = 8;
pub const ICON_HEIGHT: u8 = 8;

pub const ICON_ANTENNA: &[u8; 8] = &status::ICONS_8[Icon::Antenna as usize];
pub const ICON_KEYPAD: &[u8; 8] = &status::ICONS_8[Icon::Keypad as usize];

/// 8x8 system glyph for an `abi::icon` identifier in `SYSTEM_BASE..`.
#[inline(always)]
pub const fn system_icon(id: u8) -> Option<&'static [u8; 8]> {
    let slot = id.wrapping_sub(icon::SYSTEM_BASE) as usize;
    if slot < icon::SYSTEM_COUNT {
        Some(&SYSTEM_ICONS_8[slot])
    } else {
        None
    }
}

/// Compile-time lookup of a named system glyph; an out-of-range id fails the build.
const fn sys(id: u8) -> &'static [u8; 8] {
    match system_icon(id) {
        Some(bits) => bits,
        None => panic!("identifier is not an 8x8 system glyph"),
    }
}

/// Launcher slot that gives an `abi::icon` identifier its 16x16 artwork.
/// Identifiers without dedicated artwork use the puzzle-piece [`Launcher::Apps`].
pub const fn launcher_slot(id: u8) -> Launcher {
    match id {
        icon::APP_RADIO => Launcher::Radio,
        icon::APP_SETTINGS | icon::GEAR => Launcher::Settings,
        icon::APP_EXPLORER | icon::FOLDER | icon::FILE_FOLDER => Launcher::Folder,
        icon::APP_TORCH => Launcher::Flashlight,
        icon::APP_DIAG => Launcher::Diagnostics,
        icon::APP_ABOUT => Launcher::Info,
        icon::BEACON_TOWER => Launcher::Beacon,
        icon::UPLOAD => Launcher::Upload,
        icon::SECURITY => Launcher::Security,
        icon::SYSTEM => Launcher::System,
        icon::SLEEP => Launcher::Sleep,
        icon::MESSAGE => Launcher::Message,
        _ => Launcher::Apps,
    }
}

/// 16x16 launcher artwork for an `abi::icon` identifier.
#[inline(always)]
pub fn icon_16(id: u8) -> &'static [u8; 32] {
    &ICONS_16[launcher_slot(id) as usize]
}

pub fn draw_status_glyph(canvas: &mut Canvas, x: i16, y: i16, glyph: &StatusGlyph, color: Color) {
    match glyph {
        StatusGlyph::Builtin(icon) => draw_icon_5x5(canvas, x, y, *icon, color),
        StatusGlyph::Custom(bmp) => draw::draw_bitmap(canvas, x, y, 5, 5, bmp, color),
    }
}

/// Built-in 5x5 empty tofu box `□` fallback for missing external status glyphs.
pub const FALLBACK_BOX_5: [u8; 5] = [0xF8, 0x88, 0x88, 0x88, 0xF8];
/// Built-in 5x5 `?` fallback for missing external status glyphs.
pub const FALLBACK_QMARK_5: [u8; 5] = [0x70, 0x90, 0x20, 0x00, 0x20];
/// Built-in 8x8 empty tofu box `□` fallback for missing external 8x8 glyphs.
pub const FALLBACK_BOX_8: [u8; 8] = [0xFF, 0x81, 0x81, 0x81, 0x81, 0x81, 0xFF, 0x00];
/// Built-in 8x8 boxed `[?]` fallback for missing external or unknown icons.
pub const FALLBACK_QMARK_8: &[u8; 8] = sys(icon::MISSING);

/// 8x8 system glyph for an `abi::icon` identifier, falling back to `[?]` when absent.
#[inline(always)]
pub const fn system_icon_or_missing(id: u8) -> &'static [u8; 8] {
    match system_icon(id) {
        Some(bits) => bits,
        None => FALLBACK_QMARK_8,
    }
}

pub fn draw_icon(canvas: &mut Canvas, x: i16, y: i16, icon: Icon, color: Color) {
    let idx = icon as usize;
    if idx < Icon::PRIMARY_COUNT {
        draw::draw_bitmap(canvas, x, y, 8, 8, &status::ICONS_8[idx], color);
    } else {
        // Extended status icons have crisp 5px art; center inside the 8x8 cell.
        let w = icon.width();
        let dx = ((8 - w) >> 1) as i16;
        draw::draw_bitmap(canvas, x + dx, y + 1, w, 5, &status::ICONS_5[idx], color);
    }
}

pub fn draw_icon_5x5(canvas: &mut Canvas, x: i16, y: i16, icon: Icon, color: Color) {
    draw::draw_bitmap(
        canvas,
        x,
        y,
        icon.width(),
        5,
        &status::ICONS_5[icon as usize],
        color,
    );
}

pub fn draw_icon_16(canvas: &mut Canvas, x: i16, y: i16, bmp: &[u8; 32], color: Color) {
    draw::draw_bitmap(canvas, x, y, 16, 16, bmp, color);
}

/// Draw an 8x8 standard 1-bit monochrome icon bitmap.
pub fn draw_icon_8(canvas: &mut Canvas, x: i16, y: i16, bits: &[u8; 8], color: Color) {
    draw::draw_bitmap(canvas, x, y, 8, 8, bits, color);
}

// ── 16x16 launcher artwork ───────────────────────────────────────────────────
pub const ICON_16_RADIO: &[u8; 32] = &ICONS_16[Launcher::Radio as usize];
pub const ICON_16_SETTINGS: &[u8; 32] = &ICONS_16[Launcher::Settings as usize];
pub const ICON_16_FOLDER: &[u8; 32] = &ICONS_16[Launcher::Folder as usize];
pub const ICON_16_FLASHLIGHT: &[u8; 32] = &ICONS_16[Launcher::Flashlight as usize];
pub const ICON_16_DIAGNOSTICS: &[u8; 32] = &ICONS_16[Launcher::Diagnostics as usize];
pub const ICON_16_INFO: &[u8; 32] = &ICONS_16[Launcher::Info as usize];
pub const ICON_16_APPS: &[u8; 32] = &ICONS_16[Launcher::Apps as usize];
pub const ICON_16_BEACON: &[u8; 32] = &ICONS_16[Launcher::Beacon as usize];
pub const ICON_16_UPLOAD: &[u8; 32] = &ICONS_16[Launcher::Upload as usize];
pub const ICON_16_SECURITY: &[u8; 32] = &ICONS_16[Launcher::Security as usize];
pub const ICON_16_SYSTEM: &[u8; 32] = &ICONS_16[Launcher::System as usize];
pub const ICON_16_SLEEP: &[u8; 32] = &ICONS_16[Launcher::Sleep as usize];
pub const ICON_16_MESSAGE: &[u8; 32] = &ICONS_16[Launcher::Message as usize];

// ── Named 8x8 system glyphs (resolved by `abi::icon` identifier) ─────────────
pub const ICON_FOLDER: &[u8; 8] = sys(icon::FOLDER);
pub const ICON_CH: &[u8; 8] = sys(icon::CH);
pub const ICON_VFO: &[u8; 8] = sys(icon::VFO);

pub const ICON_APP_RADIO: &[u8; 8] = sys(icon::APP_RADIO);
pub const ICON_APP_SETTINGS: &[u8; 8] = sys(icon::APP_SETTINGS);
pub const ICON_APP_EXPLORER: &[u8; 8] = sys(icon::APP_EXPLORER);
pub const ICON_APP_TORCH: &[u8; 8] = sys(icon::APP_TORCH);
pub const ICON_APP_DIAG: &[u8; 8] = sys(icon::APP_DIAG);
pub const ICON_APP_ABOUT: &[u8; 8] = sys(icon::APP_ABOUT);

pub const CHECKBOX_OFF: &[u8; 8] = sys(icon::CHECKBOX_OFF);
pub const CHECKBOX_ON: &[u8; 8] = sys(icon::CHECKBOX_ON);
pub const RADIO_OFF: &[u8; 8] = sys(icon::RADIO_OFF);
pub const RADIO_ON: &[u8; 8] = sys(icon::RADIO_ON);
pub const ARROW_RIGHT: &[u8; 8] = sys(icon::ARROW_RIGHT);
pub const ARROW_LEFT: &[u8; 8] = sys(icon::ARROW_LEFT);
pub const SELECT_ARROWS: &[u8; 8] = sys(icon::SELECT_ARROWS);
pub const ICON_GEAR: &[u8; 8] = sys(icon::GEAR);
pub const FILE_DOC: &[u8; 8] = sys(icon::FILE_DOC);
pub const FILE_FOLDER: &[u8; 8] = sys(icon::FILE_FOLDER);

pub const ICON_DISPLAY: &[u8; 8] = sys(icon::DISPLAY);
pub const ICON_BUTTONS: &[u8; 8] = sys(icon::BUTTONS);
pub const ICON_AUDIO: &[u8; 8] = sys(icon::AUDIO);
pub const ICON_BATTERY: &[u8; 8] = sys(icon::BATTERY);
pub const ICON_DEVICE: &[u8; 8] = sys(icon::DEVICE);
pub const ICON_APPS: &[u8; 8] = sys(icon::APPS);
pub const ICON_SECURITY: &[u8; 8] = sys(icon::SECURITY);
pub const ICON_SYSTEM: &[u8; 8] = sys(icon::SYSTEM);
pub const ICON_UPLOAD: &[u8; 8] = sys(icon::UPLOAD);
pub const ICON_HOME: &[u8; 8] = sys(icon::HOME);
pub const ICON_MOON: &[u8; 8] = sys(icon::SLEEP);
/// 8x8 aspect-compensated Globe icon for Language / Region settings.
pub const ICON_GLOBE: &[u8; 8] = &[0x3C, 0x5A, 0xA5, 0xFF, 0xA5, 0x5A, 0x3C, 0x00];
pub static ICON_NONE: [u8; 8] = [0u8; 8];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baked_art_matches_reference_bits() {
        assert_eq!(
            *ICON_FOLDER,
            [0x60, 0x9E, 0x81, 0xFF, 0x81, 0x81, 0x7E, 0x00]
        );
        assert_eq!(*ICON_APPS, [0x30, 0x30, 0xFC, 0x3F, 0x3F, 0xFC, 0xCC, 0x00]);
        assert_eq!(
            status::ICONS_8[Icon::Lock as usize],
            [0x38, 0x44, 0x44, 0xFE, 0xEE, 0xEE, 0xFE, 0x00]
        );
    }

    #[test]
    fn status_glyphs_stay_inside_their_width() {
        for n in 0..Icon::COUNT {
            let icon = Icon::from_index(n as u8).unwrap();
            assert_eq!(Icon::from_u8(icon.abi_id()), Some(icon));
            let mask = !(0xFFu8 >> icon.width());
            for row in status::ICONS_5[n] {
                assert_eq!(row & !mask, 0, "{icon:?} spills past its width");
            }
        }
    }

    #[test]
    fn file_type_glyphs_are_distinct() {
        let ids = [
            icon::FILE_DOC,
            icon::FILE_FOLDER,
            icon::FILE_BIN,
            icon::FILE_CONTACTS,
            icon::FILE_BANDPLAN,
            icon::FILE_FM,
            icon::FILE_IMAGE,
            icon::FILE_ZONES,
            icon::FILE_HITS,
            icon::FILE_TONES,
            icon::FILE_CALIB,
            icon::FILE_LOG,
            icon::FILE_KEY,
            icon::FILE_FONT,
            icon::FILE_SCRIPT,
            icon::FILE_UNKNOWN,
            icon::FILE_MSG,
            icon::FILE_CFG,
            icon::FILE_FW,
            icon::FILE_AUDIO,
            icon::FILE_TABLE,
            icon::FILE_CERT,
            icon::FILE_ARCHIVE,
            icon::FILE_MAP,
            icon::FILE_TEXT,
        ];
        for (i, &a) in ids.iter().enumerate() {
            for &b in &ids[i + 1..] {
                assert_ne!(system_icon(a), system_icon(b), "ids {a} and {b} share art");
            }
        }
    }

    #[test]
    fn system_icon_rejects_status_and_unknown_ids() {
        assert!(system_icon(icon::MOON).is_none());
        assert!(system_icon(icon::MISSING + 1).is_none());
        assert_eq!(system_icon(icon::FOLDER), Some(ICON_FOLDER));
        assert_eq!(system_icon_or_missing(0xFE), FALLBACK_QMARK_8);
    }

    #[test]
    fn launcher_slots_cover_dedicated_artwork() {
        assert_eq!(launcher_slot(icon::BEACON_TOWER), Launcher::Beacon);
        assert_eq!(launcher_slot(icon::GEAR), Launcher::Settings);
        assert_eq!(launcher_slot(icon::SLEEP), Launcher::Sleep);
        assert_eq!(launcher_slot(icon::FILE_BIN), Launcher::Apps);
        assert_eq!(icon_16(icon::APP_RADIO), ICON_16_RADIO);
    }
}
