//! Monochromatic 8x8 & 16x16 icon glyph bitmaps for radio status bars & apps.

use crate::draw;
use crate::{Canvas, Color};

pub const ICON_WIDTH: u8 = 8;
pub const ICON_HEIGHT: u8 = 8;

// 8x8 Bitmaps
pub const ICON_ANTENNA: [u8; 8] = [0x00, 0x55, 0x2A, 0x1C, 0x08, 0x08, 0x08, 0x00];
pub const ICON_LOCK: [u8; 8] = [0x00, 0x1C, 0x22, 0x22, 0x7F, 0x7F, 0x7F, 0x00];
pub const ICON_UNLOCK: [u8; 8] = [0x00, 0x1C, 0x20, 0x20, 0x7F, 0x7F, 0x7F, 0x00];
pub const ICON_SPEAKER: [u8; 8] = [0x00, 0x12, 0x36, 0x7E, 0x7E, 0x36, 0x12, 0x00];
pub const ICON_SHIELD: [u8; 8] = [0x00, 0x7E, 0x7E, 0x3C, 0x3C, 0x18, 0x00, 0x00];
pub const ICON_KEYPAD: [u8; 8] = [0x00, 0x6E, 0x6E, 0x00, 0x6E, 0x6E, 0x00, 0x00];
pub const ICON_FLASHLIGHT: [u8; 8] = [0x00, 0x1C, 0x3E, 0x1C, 0x1C, 0x1C, 0x1C, 0x00];
pub const ICON_MESH: [u8; 8] = [0x00, 0x24, 0x66, 0xE7, 0xE7, 0x66, 0x24, 0x00];
pub const ICON_PLUG: [u8; 8] = [0x00, 0x36, 0x36, 0x7F, 0x7F, 0x3E, 0x08, 0x08];
pub const ICON_BOLT: [u8; 8] = [0x0C, 0x18, 0x30, 0x7E, 0x0C, 0x18, 0x10, 0x00];

// 5x5 Compact Status Bar Glyph Bitmaps (1 byte per row, top 5 bits used)
pub const ICON_LOCK_5X5: [u8; 5] = [0b01110000, 0b10001000, 0b11111000, 0b11111000, 0b11111000];
pub const ICON_UNLOCK_5X5: [u8; 5] = [0b01110000, 0b10000000, 0b11111000, 0b11111000, 0b11111000];
pub const ICON_ANTENNA_5X5: [u8; 5] = [0b10101000, 0b01110000, 0b00100000, 0b00100000, 0b00100000];
pub const ICON_SPEAKER_5X5: [u8; 5] = [0b00100000, 0b01101000, 0b11111000, 0b01101000, 0b00100000];
pub const ICON_FLASHLIGHT_5X5: [u8; 5] =
    [0b11111000, 0b01110000, 0b00100000, 0b00100000, 0b00100000];
pub const ICON_FUNCTION_5X5: [u8; 5] = [0b11111000, 0b10000000, 0b11100000, 0b10000000, 0b10000000];

// 5x5 Charging lightning bolt: ..██. / .██.. / █████ / ..██. / .█...
pub const ICON_BOLT_5X5: [u8; 5] = [0b00110000, 0b01100000, 0b11111000, 0b00110000, 0b01000000];

// 7x5 USB plug icon (idle / host connected)
// XXXOOOO, XXXXXXO, XXXOOOX, OOOOOOX, OOOOOXO
pub const ICON_PLUG_7X5: [u8; 5] = [0b11100000, 0b11111100, 0b11100010, 0b00000010, 0b00000100];

// 7x5 USB plug with dynamic packet activity pulse (active serial / RigWire / protocol traffic)
// XXXOOOO, XXXXXXO, XXXOOOX, OXOOOOX, XXXOOXO
pub const ICON_SERIAL_ACTIVE_7X5: [u8; 5] =
    [0b11100000, 0b11111100, 0b11100010, 0b01000010, 0b11100100];

// Symbols imported from fagci-k1-fw Symbols font:
// 5x5 Broadcast FM receiver / tower waves
pub const ICON_BROADCAST_5X5: [u8; 5] =
    [0b11100000, 0b00010000, 0b11001000, 0b00101000, 0b10101000];

// 7x5 Storage write / Flash write busy indicator
pub const ICON_STORAGE_7X5: [u8; 5] = [0b10101010, 0b11111110, 0b11111110, 0b11111110, 0b10101010];

// 5x5 Frequency / channel scanner active
pub const ICON_SCAN_5X5: [u8; 5] = [0b01110000, 0b11001000, 0b10101000, 0b10001000, 0b01110000];

// 6x5 Radio beacon / transmitter antenna waves
pub const ICON_BEACON_6X5: [u8; 5] = [0b00001000, 0b00100100, 0b10010100, 0b00100100, 0b00001000];

// 5x5 Monitor mode (squelch open / bypass)
pub const ICON_MONITOR_5X5: [u8; 5] = [0b10001000, 0b11011000, 0b10101000, 0b10001000, 0b10001000];

// 5x5 Upconverter / transverter active
pub const ICON_CONVERTER_5X5: [u8; 5] =
    [0b01000000, 0b01010000, 0b11111000, 0b11111000, 0b11111000];

// 5x5 Roger beep active
pub const ICON_ROGER_5X5: [u8; 5] = [0b01110000, 0b01001000, 0b01001000, 0b01001000, 0b01110000];

// 5x5 Audio beep enabled
pub const ICON_BEEP_5X5: [u8; 5] = [0b00011000, 0b01111000, 0b01111000, 0b01111000, 0b00011000];

// 7x5 Listen inhibit / no listen
pub const ICON_NO_LISTEN_7X5: [u8; 5] =
    [0b00100000, 0b11101010, 0b11100100, 0b11101010, 0b00100000];

// 8x8 Special Symbols (from fagci-k1-fw):
pub const ICON_MELODY: [u8; 8] = [0x04, 0x06, 0x04, 0x0C, 0x0C, 0x04, 0x06, 0x06];
pub const ICON_FOLDER: [u8; 8] = [0xE0, 0x9F, 0x81, 0x81, 0x81, 0x81, 0x81, 0xFF];
pub const ICON_CH: [u8; 8] = [0x88, 0xD8, 0xA8, 0x88, 0x06, 0x05, 0x06, 0x05];
pub const ICON_VFO: [u8; 8] = [0xA0, 0xA0, 0x48, 0x10, 0x1A, 0x15, 0x05, 0x02];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Antenna,
    Lock,
    Unlock,
    Speaker,
    Shield,
    Keypad,
    Flashlight,
    Mesh,
    Function,
    Plug,
    SerialActive,
    Bolt,
    Broadcast,
    Storage,
    Scan,
    Beacon,
    Monitor,
    Converter,
    Roger,
    Beep,
    NoListen,
}

impl Icon {
    pub const fn width(self) -> u8 {
        match self {
            Icon::Plug | Icon::SerialActive | Icon::Storage | Icon::NoListen => 7,
            Icon::Beacon => 6,
            _ => 5,
        }
    }
}

pub fn draw_icon(canvas: &mut Canvas, x: i16, y: i16, icon: Icon, color: Color) {
    let bmp = match icon {
        Icon::Antenna => &ICON_ANTENNA,
        Icon::Lock => &ICON_LOCK,
        Icon::Unlock => &ICON_UNLOCK,
        Icon::Speaker => &ICON_SPEAKER,
        Icon::Shield => &ICON_SHIELD,
        Icon::Keypad => &ICON_KEYPAD,
        Icon::Flashlight => &ICON_FLASHLIGHT,
        Icon::Mesh => &ICON_MESH,
        Icon::Function => &ICON_KEYPAD,
        Icon::Plug | Icon::SerialActive => &ICON_PLUG,
        Icon::Bolt => &ICON_BOLT,
        _ => &ICON_ANTENNA,
    };
    draw::draw_bitmap(canvas, x, y, 8, 8, bmp, color);
}

pub fn draw_icon_5x5(canvas: &mut Canvas, x: i16, y: i16, icon: Icon, color: Color) {
    let (bmp, w) = match icon {
        Icon::Antenna => (&ICON_ANTENNA_5X5[..], 5),
        Icon::Lock => (&ICON_LOCK_5X5[..], 5),
        Icon::Unlock => (&ICON_UNLOCK_5X5[..], 5),
        Icon::Speaker => (&ICON_SPEAKER_5X5[..], 5),
        Icon::Flashlight => (&ICON_FLASHLIGHT_5X5[..], 5),
        Icon::Function => (&ICON_FUNCTION_5X5[..], 5),
        Icon::Plug => (&ICON_PLUG_7X5[..], 7),
        Icon::SerialActive => (&ICON_SERIAL_ACTIVE_7X5[..], 7),
        Icon::Bolt => (&ICON_BOLT_5X5[..], 5),
        Icon::Broadcast => (&ICON_BROADCAST_5X5[..], 5),
        Icon::Storage => (&ICON_STORAGE_7X5[..], 7),
        Icon::Scan => (&ICON_SCAN_5X5[..], 5),
        Icon::Beacon => (&ICON_BEACON_6X5[..], 6),
        Icon::Monitor => (&ICON_MONITOR_5X5[..], 5),
        Icon::Converter => (&ICON_CONVERTER_5X5[..], 5),
        Icon::Roger => (&ICON_ROGER_5X5[..], 5),
        Icon::Beep => (&ICON_BEEP_5X5[..], 5),
        Icon::NoListen => (&ICON_NO_LISTEN_7X5[..], 7),
        _ => (&ICON_LOCK_5X5[..], 5),
    };
    draw::draw_bitmap(canvas, x, y, w, 5, bmp, color);
}

// ── 16x16 App Icon Bitmaps (32 bytes per icon: 2 bytes per row * 16 rows) ──────

pub const ICON_16_RADIO: [u8; 32] = [
    0x02, 0x00, //      █
    0x02, 0x00, //      █
    0x02, 0x20, //      █   █
    0x02, 0x70, //      █  ███
    0x1F, 0xF0, //    ████ ████
    0x1F, 0xF0, //    ████ ████
    0x18, 0x10, //    ██      █
    0x39, 0x90, //   ███  ██  █  (PTT button + LCD display)
    0x38, 0x10, //   ███      █
    0x1F, 0xF0, //    ████ ████
    0x18, 0x10, //    ██      █
    0x19, 0x90, //    ██  ██  █  (Speaker grille)
    0x18, 0x10, //    ██      █
    0x19, 0x90, //    ██  ██  █
    0x1F, 0xF0, //    ████ ████
    0x0F, 0xE0, //     ███████
];

pub const ICON_16_SETTINGS: [u8; 32] = [
    0x07, 0xE0, //      ██████
    0x07, 0xE0, //      ██████
    0x67, 0xE6, //  ██  ██████  ██
    0x7F, 0xFE, //  ██████████████
    0x3F, 0xFC, //   ████████████
    0x7E, 0x7E, //  ██████  ██████
    0xF0, 0x0F, // ████        ████
    0xF1, 0x8F, // ████  ████  ████
    0xF1, 0x8F, // ████  ████  ████
    0xF0, 0x0F, // ████        ████
    0x7E, 0x7E, //  ██████  ██████
    0x3F, 0xFC, //   ████████████
    0x7F, 0xFE, //  ██████████████
    0x67, 0xE6, //  ██  ██████  ██
    0x07, 0xE0, //      ██████
    0x07, 0xE0, //      ██████
];

pub const ICON_16_SCANNER: [u8; 32] = [
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

pub const ICON_16_FLASHLIGHT: [u8; 32] = [
    0x48, 0x24, //  █  █     █  █ (Light beam rays)
    0x23, 0xC4, //   █   ████   █
    0x1F, 0xF8, //    █████████
    0x1F, 0xF8, //    █████████
    0x0F, 0xF0, //     ████████
    0x07, 0xE0, //      ██████
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x07, 0xC0, //      █████      (Switch button)
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x03, 0xC0, //       ████
    0x01, 0x80, //        ██
];

pub const ICON_16_DIAGNOSTICS: [u8; 32] = [
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

pub const ICON_16_INFO: [u8; 32] = [
    0x07, 0xE0, //      ██████
    0x1F, 0xF8, //    █████████
    0x3F, 0xFC, //   ████████████
    0x73, 0xCE, //  ███  ██  ███  (Dot of i)
    0x73, 0xCE, //  ███  ██  ███
    0xF0, 0x0F, // ████        ████
    0xF3, 0x8F, // ████  ███   ████ (Top serif)
    0xF1, 0x8F, // ████   ██   ████
    0xF1, 0x8F, // ████   ██   ████
    0xF1, 0x8F, // ████   ██   ████
    0xF3, 0xCF, // ████  ████  ████ (Bottom serif)
    0x3F, 0xFC, //   ████████████
    0x1F, 0xF8, //    █████████
    0x07, 0xE0, //      ██████
    0x00, 0x00, //
    0x00, 0x00, //
];

pub const ICON_16_APPS: [u8; 32] = [
    0x03, 0xC0, 0x07, 0xE0, 0x07, 0xE0, 0x03, 0xC0, 0x3F, 0xF8, 0x3F, 0xF8, 0x3F, 0xFE, 0x07, 0xFF,
    0x07, 0xFF, 0x3F, 0xFE, 0x3F, 0xF8, 0x3F, 0xF8, 0x38, 0x18, 0x30, 0x08, 0x00, 0x00, 0x00, 0x00,
];

// Dedicated 8x8 App Category Icons (Full 8x8 geometry matching settings icons)
pub const ICON_APP_RADIO: [u8; 8] = [0x63, 0x64, 0xFF, 0xC3, 0xDB, 0xFF, 0xDB, 0xFF];
pub const ICON_APP_SETTINGS: [u8; 8] = [0x66, 0xFF, 0xC3, 0xDB, 0xDB, 0xC3, 0xFF, 0x66];
pub const ICON_APP_EXPLORER: [u8; 8] = [0xF0, 0xFF, 0x81, 0xBD, 0xBD, 0xFF, 0x81, 0xFF];
pub const ICON_APP_TORCH: [u8; 8] = [0xDB, 0x7E, 0x3C, 0x3C, 0x7E, 0x3C, 0x3C, 0x7E];
pub const ICON_APP_DIAG: [u8; 8] = [0xFF, 0x81, 0x91, 0xA9, 0x8F, 0xFF, 0x18, 0x7E];
pub const ICON_APP_ABOUT: [u8; 8] = [0x3C, 0x7E, 0xDB, 0xC3, 0xDB, 0xDB, 0x7E, 0x3C];

pub fn draw_icon_16(canvas: &mut Canvas, x: i16, y: i16, bmp: &[u8; 32], color: Color) {
    draw::draw_bitmap(canvas, x, y, 16, 16, bmp, color);
}

// ── UI Controls & File Icons (8x8) ──────────────────────────────────────────
pub const CHECKBOX_OFF: [u8; 8] = [0x00, 0xfc, 0x84, 0x84, 0x84, 0x84, 0xfc, 0x00];
pub const CHECKBOX_ON: [u8; 8] = [0x02, 0xf4, 0x8c, 0xd4, 0xa4, 0x84, 0xfc, 0x00];

pub const RADIO_OFF: [u8; 8] = [0x00, 0x3c, 0x42, 0x42, 0x42, 0x42, 0x3c, 0x00];
pub const RADIO_ON: [u8; 8] = [0x00, 0x3c, 0x42, 0x5a, 0x5a, 0x42, 0x3c, 0x00];

pub const ARROW_RIGHT: [u8; 8] = [0x00, 0x20, 0x30, 0x38, 0x30, 0x20, 0x00, 0x00];
pub const ARROW_LEFT: [u8; 8] = [0x00, 0x08, 0x18, 0x38, 0x18, 0x08, 0x00, 0x00];
pub const SELECT_ARROWS: [u8; 8] = [0x00, 0x20, 0x70, 0x00, 0x70, 0x20, 0x00, 0x00];

pub const ICON_GEAR: [u8; 8] = [0x18, 0x7e, 0x66, 0xc3, 0xc3, 0x66, 0x7e, 0x18];

pub const FILE_DOC: [u8; 8] = [0x7c, 0x46, 0x5e, 0x42, 0x5e, 0x42, 0x5c, 0x7e];
pub const FILE_FOLDER: [u8; 8] = [0x00, 0x70, 0x48, 0x7f, 0x41, 0x41, 0x7f, 0x00];
pub const FILE_SETTING: [u8; 8] = [0x18, 0x7e, 0x66, 0xc3, 0xc3, 0x66, 0x7e, 0x18];
pub const FILE_VFO: [u8; 8] = [0x18, 0x3c, 0x7e, 0xdb, 0x18, 0x24, 0x7e, 0x00];
pub const FILE_CHANNEL: [u8; 8] = [0x10, 0x10, 0x7c, 0x44, 0x54, 0x44, 0x7c, 0x00];
pub const FILE_LIST: [u8; 8] = [0xcf, 0xc0, 0x00, 0xcf, 0xc0, 0x00, 0xcf, 0xc0];
pub const FILE_LOG: [u8; 8] = [0x7c, 0x4e, 0x5a, 0x6e, 0x5a, 0x76, 0x42, 0x7e];
pub const FILE_KEY: [u8; 8] = [0x00, 0x38, 0x44, 0x38, 0x10, 0x1c, 0x10, 0x00];
pub const FILE_BIN: [u8; 8] = [0x24, 0x5e, 0x7e, 0x00, 0x3c, 0x42, 0x3c, 0x00];
pub const FILE_AUDIO: [u8; 8] = [0x00, 0x0e, 0x0a, 0x0a, 0x0a, 0x1a, 0x3a, 0x38];
pub const FILE_DISPLAY: [u8; 8] = [0x00, 0x7e, 0x42, 0x42, 0x7e, 0x18, 0x3c, 0x00];
pub const FILE_KEYS: [u8; 8] = [0x00, 0x7e, 0x42, 0x5a, 0x42, 0x5a, 0x7e, 0x00];
pub const FILE_SYSTEM: [u8; 8] = [0x00, 0x5a, 0x3c, 0x42, 0x42, 0x3c, 0x5a, 0x00];

// Standard 8x8 Category & Subsystem Icons
pub const ICON_DISPLAY: [u8; 8] = [0xFF, 0x81, 0x81, 0x81, 0x81, 0xFF, 0x18, 0x3C];
pub const ICON_BUTTONS: [u8; 8] = [0xDB, 0xDB, 0x00, 0xDB, 0xDB, 0x00, 0xDB, 0xDB];
pub const ICON_AUDIO: [u8; 8] = [0x30, 0x72, 0xF5, 0xF5, 0xF5, 0xF5, 0x72, 0x30];
pub const ICON_BATTERY: [u8; 8] = [0x3C, 0xFF, 0x81, 0xBD, 0xBD, 0xBD, 0xBD, 0xFF];
pub const ICON_DEVICE: [u8; 8] = [0x40, 0x46, 0x7E, 0x42, 0x42, 0x7E, 0x66, 0x7E];
pub const ICON_APPS: [u8; 8] = [0x30, 0x30, 0xFC, 0x3F, 0x3F, 0xFC, 0xCC, 0x00];
pub const ICON_SECURITY: [u8; 8] = [0x3C, 0x42, 0x42, 0xFF, 0xE7, 0xE7, 0xFF, 0x7E];
pub const ICON_SYSTEM: [u8; 8] = [0x19, 0x33, 0x1E, 0x18, 0x18, 0x3C, 0x3C, 0x3C];

/// Draw an 8x8 standard 1-bit monochrome icon bitmap.
pub fn draw_icon_8(canvas: &mut Canvas, x: i16, y: i16, bits: &[u8; 8], color: Color) {
    for (row, byte) in bits.iter().enumerate() {
        for col in 0..8 {
            if (byte & (0x80 >> col)) != 0 {
                let px = x + col;
                let py = y + row as i16;
                if px >= 0 && px < canvas.width() as i16 && py >= 0 && py < canvas.height() as i16 {
                    canvas.draw_pixel(px as u8, py as u8, color);
                }
            }
        }
    }
}
