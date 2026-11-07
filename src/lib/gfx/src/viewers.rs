//! Mini-App Viewer and File Browser Graphical Primitives.
//!
//! Provides optimized visual rendering routines for the 128×64 LCD screen,
//! including Hex Viewers, In-Place File Editors, and File Manager item lists.

use crate::draw::draw_line;
use crate::fonts::mumatrix_regular::FONT as FONT_REGULAR;
use crate::fonts::tomthumb::FONT as FONT_THUMB;
use crate::text::{draw_char, draw_str};
use crate::{Canvas, Color};

const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

/// Draw a single row in the Hex Viewer (offset, 4 bytes hex, and ASCII representation).
///
/// Layout (Y=row_y): `0020: 7F 4B 52 58 | .KRX`
pub fn draw_hex_row(
    canvas: &mut Canvas,
    y: u8,
    offset: u16,
    bytes: &[u8],
    highlight_byte_idx: Option<usize>,
) {
    let mut x: i16 = 2;
    let yi = y as i16;

    // 1. Render 4-digit hex offset "XXXX:"
    for shift in [12, 8, 4, 0] {
        let nibble = ((offset >> shift) & 0x0F) as usize;
        let c = HEX_CHARS[nibble] as char;
        x += draw_char(canvas, x, yi, c, &FONT_THUMB, Color::Fill);
    }
    x += draw_char(canvas, x, yi, ':', &FONT_THUMB, Color::Fill);
    x += 3; // spacing

    // 2. Render 4 hex bytes
    for i in 0..4 {
        if i < bytes.len() {
            let b = bytes[i];
            let hi = ((b >> 4) & 0x0F) as usize;
            let lo = (b & 0x0F) as usize;

            let is_hl = highlight_byte_idx == Some(i);
            if is_hl {
                canvas.fill_rect((x - 1) as u8, (yi - 5) as u8, 11, 7, Color::Fill);
            }
            let color = if is_hl { Color::Clear } else { Color::Fill };

            x += draw_char(canvas, x, yi, HEX_CHARS[hi] as char, &FONT_THUMB, color);
            x += draw_char(canvas, x, yi, HEX_CHARS[lo] as char, &FONT_THUMB, color);
            x += 2;
        } else {
            // Padding placeholder
            x += draw_char(canvas, x, yi, '-', &FONT_THUMB, Color::Fill);
            x += draw_char(canvas, x, yi, '-', &FONT_THUMB, Color::Fill);
            x += 2;
        }
    }

    // 3. Render divider
    x += 1;
    x += draw_char(canvas, x, yi, '|', &FONT_THUMB, Color::Fill);
    x += 2;

    // 4. Render ASCII printable characters
    for i in 0..4 {
        if i < bytes.len() {
            let b = bytes[i];
            let c = if (0x20..=0x7E).contains(&b) {
                b as char
            } else {
                '.'
            };
            let is_hl = highlight_byte_idx == Some(i);
            let color = if is_hl { Color::Invert } else { Color::Fill };
            x += draw_char(canvas, x, yi, c, &FONT_THUMB, color);
        } else {
            x += draw_char(canvas, x, yi, ' ', &FONT_THUMB, Color::Fill);
        }
    }
}

/// Draw a single file list item row in the File Browser / Database Manager.
///
/// Features reverse-video highlight when `selected == true`.
pub fn draw_file_record_item(
    canvas: &mut Canvas,
    y: u8,
    title: &str,
    subtitle: &str,
    glyph: char,
    selected: bool,
) {
    let row_h = 10u8;
    if selected {
        canvas.fill_rect(0, y, 128, row_h, Color::Fill);
    }

    let text_color = if selected { Color::Clear } else { Color::Fill };
    let yi = (y + 8) as i16;

    // Draw glyph in brackets e.g. "[C]"
    let mut x: i16 = 2;
    x += draw_char(canvas, x, yi, '[', &FONT_THUMB, text_color);
    x += draw_char(canvas, x, yi, glyph, &FONT_THUMB, text_color);
    x += draw_char(canvas, x, yi, ']', &FONT_THUMB, text_color);
    x += 3;

    // Draw title
    draw_str(canvas, x, yi, title, &FONT_REGULAR, text_color);

    // Draw right-aligned subtitle
    let sub_w = (subtitle.len() * 4) as i16;
    let sub_x = 126 - sub_w;
    if sub_x > x + 40 {
        draw_str(canvas, sub_x, yi, subtitle, &FONT_THUMB, text_color);
    }
}

/// Draw standard 8-pixel viewer header with title and status/offset info.
pub fn draw_viewer_header(canvas: &mut Canvas, title: &str, offset_info: &str) {
    canvas.fill_rect(0, 0, 128, 8, Color::Fill);
    draw_str(canvas, 2, 7, title, &FONT_THUMB, Color::Clear);

    let info_w = (offset_info.len() * 4) as i16;
    let info_x = (126 - info_w).max(64);
    draw_str(canvas, info_x, 7, offset_info, &FONT_THUMB, Color::Clear);
}

/// Draw standard 8-pixel viewer footer with navigation shortcuts.
pub fn draw_viewer_footer(canvas: &mut Canvas, shortcuts: &str) {
    draw_line(canvas, 0, 55, 127, 55, Color::Fill);
    draw_str(canvas, 2, 63, shortcuts, &FONT_THUMB, Color::Fill);
}
