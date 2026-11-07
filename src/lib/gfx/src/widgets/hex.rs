use crate::text;
use crate::{Canvas, Color, GfxFont};

/// Memory / Packet Inspection Hex Viewer (4-byte hex row + ASCII)
pub fn draw_hex_view(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    addr: u16,
    bytes: &[u8; 4],
    font: &GfxFont,
) {
    let mut buf = [b' '; 24];
    let hex_chars = crate::text::HEX_CHARS;

    // Addr: 0x0000
    buf[0] = hex_chars[((addr >> 12) & 0xF) as usize];
    buf[1] = hex_chars[((addr >> 8) & 0xF) as usize];
    buf[2] = hex_chars[((addr >> 4) & 0xF) as usize];
    buf[3] = hex_chars[(addr & 0xF) as usize];
    buf[4] = b':';
    buf[5] = b' ';

    let mut idx = 6;
    for &b in bytes {
        buf[idx] = hex_chars[((b >> 4) & 0xF) as usize];
        buf[idx + 1] = hex_chars[(b & 0xF) as usize];
        buf[idx + 2] = b' ';
        idx += 3;
    }

    buf[idx] = b'|';
    idx += 1;
    for &b in bytes {
        buf[idx] = if (32..=126).contains(&b) { b } else { b'.' };
        idx += 1;
    }

    let s = unsafe { core::str::from_utf8_unchecked(&buf[..idx]) };
    text::draw_str(canvas, x, y, s, font, Color::Fill);
}
