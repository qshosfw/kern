//! Monochromatic 8x8 & 16x16 icon glyph bitmaps for radio status bars & apps.

use crate::draw;
use crate::{Canvas, Color};

pub const ICON_WIDTH: u8 = 8;
pub const ICON_HEIGHT: u8 = 8;

pub const ICON_ANTENNA: &[u8; 8] = &ICONS_8X8[0];
pub const ICON_KEYPAD: &[u8; 8] = &ICONS_8X8[5];

static ICONS_8X8: [[u8; 8]; 11] = [
    [0x00, 0x55, 0x2A, 0x1C, 0x08, 0x08, 0x08, 0x00], // 0: Antenna
    [0x00, 0x1C, 0x22, 0x22, 0x7F, 0x7F, 0x7F, 0x00], // 1: Lock
    [0x00, 0x1C, 0x20, 0x20, 0x7F, 0x7F, 0x7F, 0x00], // 2: Unlock
    [0x00, 0x12, 0x36, 0x7E, 0x7E, 0x36, 0x12, 0x00], // 3: Speaker
    [0x00, 0x7E, 0x7E, 0x3C, 0x3C, 0x18, 0x00, 0x00], // 4: Shield
    [0x00, 0x6E, 0x6E, 0x00, 0x6E, 0x6E, 0x00, 0x00], // 5: Keypad / Function
    [0x00, 0x1C, 0x3E, 0x1C, 0x1C, 0x1C, 0x1C, 0x00], // 6: Flashlight
    [0x00, 0x24, 0x66, 0xE7, 0xE7, 0x66, 0x24, 0x00], // 7: Mesh
    [0x00, 0x36, 0x36, 0x7F, 0x7F, 0x3E, 0x08, 0x08], // 8: Plug / SerialActive
    [0x0C, 0x18, 0x30, 0x7E, 0x0C, 0x18, 0x10, 0x00], // 9: Bolt
    [0x0C, 0x3C, 0x70, 0xE0, 0xE0, 0x70, 0x3C, 0x0C], // 10: Moon
];

static ICON_8X8_IDX: [u8; 22] = [
    0, 1, 2, 3, 4, 5, 6, 7, 5, 8, 8, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10,
];

static ICONS_5X5: [[u8; 5]; 18] = [
    [0b10101000, 0b01110000, 0b00100000, 0b00100000, 0b00100000], // 0: Antenna
    [0b01110000, 0b10001000, 0b11111000, 0b11111000, 0b11111000], // 1: Lock
    [0b01110000, 0b10000000, 0b11111000, 0b11111000, 0b11111000], // 2: Unlock
    [0b00100000, 0b01101000, 0b11111000, 0b01101000, 0b00100000], // 3: Speaker
    [0b11111000, 0b01110000, 0b00100000, 0b00100000, 0b00100000], // 4: Flashlight
    [0b11111000, 0b10000000, 0b11100000, 0b10000000, 0b10000000], // 5: Function
    [0b11100000, 0b11111100, 0b11100010, 0b00000010, 0b00000100], // 6: Plug (7x5)
    [0b11100000, 0b11111100, 0b11100010, 0b01000010, 0b11100100], // 7: SerialActive (7x5)
    [0b00110000, 0b01100000, 0b11111000, 0b00110000, 0b01000000], // 8: Bolt
    [0b11100000, 0b00010000, 0b11001000, 0b00101000, 0b10101000], // 9: Broadcast
    [0b10101010, 0b11111110, 0b11111110, 0b11111110, 0b10101010], // 10: Storage (7x5)
    [0b01110000, 0b11001000, 0b10101000, 0b10001000, 0b01110000], // 11: Scan
    [0b00001000, 0b00100100, 0b10010100, 0b00100100, 0b00001000], // 12: Beacon (6x5)
    [0b10001000, 0b11011000, 0b10101000, 0b10001000, 0b10001000], // 13: Monitor
    [0b01000000, 0b01010000, 0b11111000, 0b11111000, 0b11111000], // 14: Converter
    [0b01110000, 0b01001000, 0b01001000, 0b01001000, 0b01110000], // 15: Roger
    [0b00011000, 0b01111000, 0b01111000, 0b01111000, 0b00011000], // 16: Beep
    [0b00100000, 0b11101010, 0b11100100, 0b11101010, 0b00100000], // 17: NoListen (7x5)
];

static ICON_5X5_IDX: [u8; 22] = [
    0, 1, 2, 3, 1, 1, 4, 1, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 1,
];

// 8x8 Special Symbols (from fagci-k1-fw):
pub static ICON_MELODY: [u8; 8] = [0x04, 0x06, 0x04, 0x0C, 0x0C, 0x04, 0x06, 0x06];
pub static SYSTEM_ICONS_8: [[u8; 8]; 27] = [
    [0xE0, 0x9F, 0x81, 0x81, 0x81, 0x81, 0x81, 0xFF], // 0: ICON_FOLDER (22)
    [0x88, 0xD8, 0xA8, 0x88, 0x06, 0x05, 0x06, 0x05], // 1: ICON_CH (23)
    [0xA0, 0xA0, 0x48, 0x10, 0x1A, 0x15, 0x05, 0x02], // 2: ICON_VFO (24)
    [0x40, 0x40, 0x7E, 0x42, 0x5A, 0x42, 0x5A, 0x7E], // 3: ICON_APP_RADIO (25)
    [0x18, 0x5A, 0x3C, 0xE7, 0xE7, 0x3C, 0x5A, 0x18], // 4: ICON_APP_SETTINGS (26)
    [0x00, 0xE0, 0xFF, 0x81, 0x81, 0x81, 0xFF, 0x00], // 5: ICON_APP_EXPLORER (27)
    [0x54, 0x00, 0x7E, 0x7E, 0x3C, 0x18, 0x18, 0x18], // 6: ICON_APP_TORCH (28)
    [0xFF, 0x81, 0x91, 0xA9, 0x8F, 0xFF, 0x18, 0x7E], // 7: ICON_APP_DIAG (29)
    [0x3C, 0x7E, 0xE7, 0xFF, 0xE7, 0xE7, 0x7E, 0x3C], // 8: ICON_APP_ABOUT (30)
    [0x00, 0xfc, 0x84, 0x84, 0x84, 0x84, 0xfc, 0x00], // 9: CHECKBOX_OFF (31)
    [0x02, 0xf4, 0x8c, 0xd4, 0xa4, 0x84, 0xfc, 0x00], // 10: CHECKBOX_ON (32)
    [0x00, 0x3c, 0x42, 0x42, 0x42, 0x42, 0x3c, 0x00], // 11: RADIO_OFF (33)
    [0x00, 0x3c, 0x42, 0x5a, 0x5a, 0x42, 0x3c, 0x00], // 12: RADIO_ON (34)
    [0x00, 0x20, 0x30, 0x38, 0x30, 0x20, 0x00, 0x00], // 13: ARROW_RIGHT (35)
    [0x00, 0x08, 0x18, 0x38, 0x18, 0x08, 0x00, 0x00], // 14: ARROW_LEFT (36)
    [0x00, 0x20, 0x70, 0x00, 0x70, 0x20, 0x00, 0x00], // 15: SELECT_ARROWS (37)
    [0x18, 0x7e, 0x66, 0xc3, 0xc3, 0x66, 0x7e, 0x18], // 16: ICON_GEAR (38)
    [0x7c, 0x46, 0x5e, 0x42, 0x5e, 0x42, 0x5c, 0x7e], // 17: FILE_DOC (39)
    [0x00, 0x70, 0x48, 0x7f, 0x41, 0x41, 0x7f, 0x00], // 18: FILE_FOLDER (40)
    [0xFF, 0x81, 0x81, 0x81, 0x81, 0xFF, 0x18, 0x3C], // 19: ICON_DISPLAY (41)
    [0xDB, 0xDB, 0x00, 0xDB, 0xDB, 0x00, 0xDB, 0xDB], // 20: ICON_BUTTONS (42)
    [0x30, 0x72, 0xF5, 0xF5, 0xF5, 0xF5, 0x72, 0x30], // 21: ICON_AUDIO (43)
    [0x3C, 0xFF, 0x81, 0xBD, 0xBD, 0xBD, 0xBD, 0xFF], // 22: ICON_BATTERY (44)
    [0x40, 0x46, 0x7E, 0x42, 0x42, 0x7E, 0x66, 0x7E], // 23: ICON_DEVICE (45)
    [0x30, 0x30, 0xFC, 0x3F, 0x3F, 0xFC, 0xCC, 0x00], // 24: ICON_APPS (46)
    [0x3C, 0x42, 0x42, 0xFF, 0xE7, 0xE7, 0xFF, 0x7E], // 25: ICON_SECURITY (47)
    [0x19, 0x33, 0x1E, 0x18, 0x18, 0x3C, 0x3C, 0x3C], // 26: ICON_SYSTEM (48)
];

pub const ICON_FOLDER: &[u8; 8] = &SYSTEM_ICONS_8[0];
pub const ICON_CH: &[u8; 8] = &SYSTEM_ICONS_8[1];
pub const ICON_VFO: &[u8; 8] = &SYSTEM_ICONS_8[2];

pub use kernel::canvas::{Icon, StatusGlyph};

pub fn draw_status_glyph(canvas: &mut Canvas, x: i16, y: i16, glyph: &StatusGlyph, color: Color) {
    match glyph {
        StatusGlyph::Builtin(icon) => draw_icon_5x5(canvas, x, y, *icon, color),
        StatusGlyph::Custom(bmp) => draw::draw_bitmap(canvas, x, y, 5, 5, bmp, color),
    }
}

pub fn draw_icon(canvas: &mut Canvas, x: i16, y: i16, icon: Icon, color: Color) {
    let idx = ICON_8X8_IDX.get(icon as usize).copied().unwrap_or(0) as usize;
    draw::draw_bitmap(canvas, x, y, 8, 8, &ICONS_8X8[idx], color);
}

pub fn draw_icon_5x5(canvas: &mut Canvas, x: i16, y: i16, icon: Icon, color: Color) {
    let idx = ICON_5X5_IDX.get(icon as usize).copied().unwrap_or(1) as usize;
    draw::draw_bitmap(canvas, x, y, icon.width(), 5, &ICONS_5X5[idx], color);
}

// ── 16x16 App Icon Bitmaps (32 bytes per icon: 2 bytes per row * 16 rows) ──────

pub static ICON_16_RADIO: [u8; 32] = [
    0x04, 0x00, //      █
    0x04, 0x00, //      █
    0x04, 0x00, //      █
    0x04, 0x30, //      █    ██  (Antenna + knob)
    0x1F, 0xF8, //    █████████
    0x3F, 0xFC, //   ████████████
    0x30, 0x0C, //   ██        ██
    0x37, 0xEC, //   ██ ██████ ██ (LCD screen)
    0x34, 0x2C, //   ██ █    █ ██
    0x37, 0xEC, //   ██ ██████ ██
    0x30, 0x0C, //   ██        ██
    0x35, 0xAC, //   ██ █ ██ █ ██ (Speaker grille)
    0x30, 0x0C, //   ██        ██
    0x35, 0xAC, //   ██ █ ██ █ ██
    0x3F, 0xFC, //   ████████████
    0x1F, 0xF8, //    █████████
];

pub static ICON_16_SETTINGS: [u8; 32] = [
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x27, 0xE4, //   █  ██████  █
    0x7F, 0xFE, //  ██████████████
    0x7C, 0x3E, //  █████    █████
    0x38, 0x1C, //   ███      ███
    0xF3, 0xCF, // ████  ████  ████
    0xF2, 0x4F, // ████  █  █  ████
    0xF2, 0x4F, // ████  █  █  ████
    0xF3, 0xCF, // ████  ████  ████
    0x38, 0x1C, //   ███      ███
    0x7C, 0x3E, //  █████    █████
    0x7F, 0xFE, //  ██████████████
    0x27, 0xE4, //   █  ██████  █
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
];

pub static ICON_16_SCANNER: [u8; 32] = [
    0x3C, 0x00, //   ████
    0x7E, 0x00, //  ██████
    0x7F, 0xFC, //  ██████████████
    0x40, 0x04, //  █            █
    0x5F, 0xC4, //  █ ████████   █
    0x50, 0x24, //  █ █      █   █
    0x7F, 0xFC, //  ██████████████
    0x3F, 0xFE, //   █████████████
    0x20, 0x02, //   █           █
    0x26, 0x02, //   █  ██       █
    0x23, 0x02, //   █   ██      █
    0x20, 0x02, //   █           █
    0x20, 0x02, //   █           █
    0x3F, 0xFE, //   █████████████
    0x1F, 0xFC, //    ████████████
    0x00, 0x00, //
];

pub static ICON_16_FLASHLIGHT: [u8; 32] = [
    0x10, 0x88, //    █    █   █ (Beam rays)
    0x08, 0x90, //     █   █  █
    0x00, 0x00, //
    0x1F, 0xF8, //    █████████   (Bezel)
    0x10, 0x08, //    █       █
    0x1F, 0xF8, //    █████████
    0x0F, 0xF0, //     ████████
    0x07, 0xE0, //      ██████
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x03, 0x40, //       ██ █    (Switch)
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x01, 0x80, //        ██
];

pub static ICON_16_DIAGNOSTICS: [u8; 32] = [
    0x7F, 0xFC, //  ██████████████
    0xFF, 0xFE, // ███████████████
    0xC0, 0x03, // ██           ██
    0xC0, 0x23, // ██     █     ██
    0xC0, 0x63, // ██    ██     ██
    0xC2, 0xA3, // ██  █ █ █    ██ (Pulse wave peak)
    0xC5, 0x13, // ██ █ █   █   ██
    0xF9, 0x0F, // █████ █     ████
    0xC0, 0x03, // ██           ██
    0xC0, 0x03, // ██           ██
    0xFF, 0xFE, // ███████████████
    0x7F, 0xFC, //  ██████████████
    0x03, 0xC0, //       ████      (Monitor stand)
    0x07, 0xE0, //      ██████
    0x1F, 0xF8, //    █████████
    0x00, 0x00, //
];

pub static ICON_16_INFO: [u8; 32] = [
    0x07, 0xE0, //      ██████
    0x1F, 0xF8, //    █████████
    0x3F, 0xFC, //   ████████████
    0x7E, 0x7E, //  ██████  ██████
    0x7E, 0x7E, //  ██████  ██████ (Dot of i)
    0xFF, 0xFF, // ████████████████
    0xFC, 0x3F, // ██████    ██████ (Top serif)
    0xFE, 0x7F, // ███████  ███████
    0xFE, 0x7F, // ███████  ███████
    0xFE, 0x7F, // ███████  ███████
    0xFE, 0x7F, // ███████  ███████
    0x7C, 0x3E, //  █████    █████  (Bottom serif)
    0x7F, 0xFE, //  ██████████████
    0x3F, 0xFC, //   ████████████
    0x1F, 0xF8, //    █████████
    0x07, 0xE0, //      ██████
];

pub static ICON_16_APPS: [u8; 32] = [
    0x00, 0x00, //
    0x3E, 0x7C, //   █████  █████
    0x7E, 0x7E, //  ██████  ██████
    0x7E, 0x7E, //  ██████  ██████
    0x7E, 0x7E, //  ██████  ██████
    0x7E, 0x7E, //  ██████  ██████
    0x3E, 0x7C, //   █████  █████
    0x00, 0x00, //
    0x00, 0x00, //
    0x3E, 0x7C, //   █████  █████
    0x7E, 0x7E, //  ██████  ██████
    0x7E, 0x7E, //  ██████  ██████
    0x7E, 0x7E, //  ██████  ██████
    0x7E, 0x7E, //  ██████  ██████
    0x3E, 0x7C, //   █████  █████
    0x00, 0x00, //
];

// Dedicated 8x8 App Category Icons (Full 8x8 pixel art)
pub const ICON_APP_RADIO: &[u8; 8] = &SYSTEM_ICONS_8[3];
pub const ICON_APP_SETTINGS: &[u8; 8] = &SYSTEM_ICONS_8[4];
pub const ICON_APP_EXPLORER: &[u8; 8] = &SYSTEM_ICONS_8[5];
pub const ICON_APP_TORCH: &[u8; 8] = &SYSTEM_ICONS_8[6];
pub const ICON_APP_DIAG: &[u8; 8] = &SYSTEM_ICONS_8[7];
pub const ICON_APP_ABOUT: &[u8; 8] = &SYSTEM_ICONS_8[8];

pub fn draw_icon_16(canvas: &mut Canvas, x: i16, y: i16, bmp: &[u8; 32], color: Color) {
    draw::draw_bitmap(canvas, x, y, 16, 16, bmp, color);
}

// ── UI Controls & File Icons (8x8) ──────────────────────────────────────────
pub const CHECKBOX_OFF: &[u8; 8] = &SYSTEM_ICONS_8[9];
pub const CHECKBOX_ON: &[u8; 8] = &SYSTEM_ICONS_8[10];

pub const RADIO_OFF: &[u8; 8] = &SYSTEM_ICONS_8[11];
pub const RADIO_ON: &[u8; 8] = &SYSTEM_ICONS_8[12];

pub const ARROW_RIGHT: &[u8; 8] = &SYSTEM_ICONS_8[13];
pub const ARROW_LEFT: &[u8; 8] = &SYSTEM_ICONS_8[14];
pub const SELECT_ARROWS: &[u8; 8] = &SYSTEM_ICONS_8[15];

pub const ICON_GEAR: &[u8; 8] = &SYSTEM_ICONS_8[16];

pub const FILE_DOC: &[u8; 8] = &SYSTEM_ICONS_8[17];
pub const FILE_FOLDER: &[u8; 8] = &SYSTEM_ICONS_8[18];
pub const FILE_SETTING: &[u8; 8] = ICON_GEAR;
pub static FILE_VFO: [u8; 8] = [0x18, 0x3c, 0x7e, 0xdb, 0x18, 0x24, 0x7e, 0x00];
pub static FILE_CHANNEL: [u8; 8] = [0x10, 0x10, 0x7c, 0x44, 0x54, 0x44, 0x7c, 0x00];
pub static FILE_LIST: [u8; 8] = [0xcf, 0xc0, 0x00, 0xcf, 0xc0, 0x00, 0xcf, 0xc0];
pub static FILE_LOG: [u8; 8] = [0x7c, 0x4e, 0x5a, 0x6e, 0x5a, 0x76, 0x42, 0x7e];
pub static FILE_KEY: [u8; 8] = [0x00, 0x38, 0x44, 0x38, 0x10, 0x1c, 0x10, 0x00];
pub static FILE_BIN: [u8; 8] = [0x24, 0x5e, 0x7e, 0x00, 0x3c, 0x42, 0x3c, 0x00];
pub static FILE_AUDIO: [u8; 8] = [0x00, 0x0e, 0x0a, 0x0a, 0x0a, 0x1a, 0x3a, 0x38];
pub static FILE_DISPLAY: [u8; 8] = [0x00, 0x7e, 0x42, 0x42, 0x7e, 0x18, 0x3c, 0x00];
pub static FILE_KEYS: [u8; 8] = [0x00, 0x7e, 0x42, 0x5a, 0x42, 0x5a, 0x7e, 0x00];
pub static FILE_SYSTEM: [u8; 8] = [0x00, 0x5a, 0x3c, 0x42, 0x42, 0x3c, 0x5a, 0x00];

// Standard 8x8 Category & Subsystem Icons
pub const ICON_DISPLAY: &[u8; 8] = &SYSTEM_ICONS_8[19];
pub const ICON_BUTTONS: &[u8; 8] = &SYSTEM_ICONS_8[20];
pub const ICON_AUDIO: &[u8; 8] = &SYSTEM_ICONS_8[21];
pub const ICON_BATTERY: &[u8; 8] = &SYSTEM_ICONS_8[22];
pub const ICON_DEVICE: &[u8; 8] = &SYSTEM_ICONS_8[23];
pub const ICON_APPS: &[u8; 8] = &SYSTEM_ICONS_8[24];
pub const ICON_SECURITY: &[u8; 8] = &SYSTEM_ICONS_8[25];
pub const ICON_SYSTEM: &[u8; 8] = &SYSTEM_ICONS_8[26];

/// Draw an 8x8 standard 1-bit monochrome icon bitmap.
pub fn draw_icon_8(canvas: &mut Canvas, x: i16, y: i16, bits: &[u8; 8], color: Color) {
    draw::draw_bitmap(canvas, x, y, 8, 8, bits, color);
}
