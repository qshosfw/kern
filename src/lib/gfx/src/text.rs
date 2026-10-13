//! Text rendering engine for GfxFont bitmaps.

use crate::{Canvas, Color, GfxFont};

/// Draw a single ASCII character using the provided GfxFont.
pub fn draw_char(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    c: char,
    font: &GfxFont,
    color: Color,
) -> i16 {
    let code = c as u32;

    // Render degree symbol '°' (U+00B0)
    if c == '°' || code == 0xB0 {
        if x >= 0 && y >= 4 {
            canvas.fill_rect(x as u8, (y - 4) as u8, 2, 2, color);
        }
        return 3;
    }

    if code < font.first as u32 || code > font.last as u32 {
        return 0;
    }

    let glyph_idx = (code - font.first as u32) as usize;
    if glyph_idx >= font.glyphs.len() {
        return 0;
    }

    let glyph = &font.glyphs[glyph_idx];
    let bitmap = font.bitmap;

    let width = glyph.width() as usize;
    let height = glyph.height() as usize;
    let x_base = x + glyph.x_offset() as i16;
    let y_base = y + glyph.y_offset() as i16;

    let mut bit_idx = (glyph.bitmap_offset as usize) * 8;

    for gy in 0..height {
        let py = y_base + gy as i16;
        for gx in 0..width {
            let byte_idx = bit_idx >> 3;
            let bit_shift = 7 - (bit_idx & 7);

            if byte_idx < bitmap.len() && ((bitmap[byte_idx] >> bit_shift) & 1) != 0 {
                let px = x_base + gx as i16;
                if (px as u16) < 128 && (py as u16) < 64 {
                    canvas.draw_pixel(px as u8, py as u8, color);
                }
            }

            bit_idx += 1;
        }
    }

    glyph.x_advance() as i16
}

/// Draw a string onto the canvas using the provided GfxFont.
pub fn draw_str(canvas: &mut Canvas, x: i16, y: i16, text: &str, font: &GfxFont, color: Color) {
    draw_str_max_x(canvas, x, y, text, font, color, 128);
}

/// Draw a string onto the canvas up to a maximum X coordinate.
pub fn draw_str_max_x(
    canvas: &mut Canvas,
    mut x: i16,
    y: i16,
    text: &str,
    font: &GfxFont,
    color: Color,
    max_x: i16,
) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let (c, step) = if b == 0xC2 && i + 1 < bytes.len() && bytes[i + 1] == 0xB0 {
            ('°', 2)
        } else {
            (b as char, 1)
        };
        let adv = draw_char(canvas, x, y, c, font, color);
        if x + adv > max_x {
            break;
        }
        x += adv;
        i += step;
    }
}

/// Calculate the total pixel width of a string formatted with the specified GfxFont.
pub fn str_width(text: &str, font: &GfxFont) -> i16 {
    let mut width = 0i16;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0xC2 && i + 1 < bytes.len() && bytes[i + 1] == 0xB0 {
            width += 3;
            i += 2;
            continue;
        }
        let code = b as u32;
        if code >= font.first as u32 && code <= font.last as u32 {
            let idx = (code - font.first as u32) as usize;
            if idx < font.glyphs.len() {
                width += font.glyphs[idx].x_advance() as i16;
            }
        }
        i += 1;
    }
    width
}

/// Text Alignment Option
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Draw aligned text (Left, Center, or Right) onto the canvas.
pub fn draw_str_align(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    text: &str,
    font: &GfxFont,
    color: Color,
    align: Align,
) {
    let w = str_width(text, font);
    let start_x = match align {
        Align::Left => x,
        Align::Center => x - (w / 2),
        Align::Right => x - w,
    };
    draw_str(canvas, start_x, y, text, font, color);
}

/// Stack-allocated zero-copy u32 formatter for `no_std` text rendering.
#[inline(always)]
pub fn format_u32(val: u32, buf: &mut [u8]) -> &str {
    format_int_fixed(val as i32, 0, "", "", buf)
}

/// Stack-allocated zero-copy u8 formatter for `no_std` text rendering.
#[inline(always)]
pub fn format_u8(val: u8, buf: &mut [u8]) -> &str {
    format_int_fixed(val as i32, 0, "", "", buf)
}

/// Stack-allocated zero-copy i32 formatter for `no_std` text rendering.
#[inline(always)]
pub fn format_i32(val: i32, buf: &mut [u8]) -> &str {
    format_int_fixed(val, 0, "", "", buf)
}

/// Stack-allocated zero-copy i32 formatter for range values.
/// When `min < 0` and `val > 0`, prepends a `+` symbol for clear signed indication.
#[inline(always)]
pub fn format_i32_range(val: i32, min: i32, buf: &mut [u8]) -> &str {
    let pfx = if min < 0 && val > 0 { "+" } else { "" };
    format_int_fixed(val, 0, pfx, "", buf)
}

/// Reverse find a byte in a string slice without unicode iterator bloat.
#[inline(always)]
pub fn rfind_byte(s: &str, target: u8) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 {
        i -= 1;
        if bytes[i] == target {
            return Some(i);
        }
    }
    None
}

/// Find the first occurrence of a byte in a slice without iterator closure machinery.
#[inline(always)]
pub fn find_byte(bytes: &[u8], target: u8) -> Option<usize> {
    for i in 0..bytes.len() {
        if bytes[i] == target {
            return Some(i);
        }
    }
    None
}

/// Count lines in a string slice without pulling in Split<char> iterator machinery.
#[inline(always)]
pub fn count_lines(s: &str) -> usize {
    if s.is_empty() {
        0
    } else {
        s.as_bytes().iter().filter(|&&b| b == b'\n').count() + 1
    }
}

/// Iterate over lines of text without pulling in Split<char> iterator machinery.
pub fn for_each_line<F: FnMut(&str)>(s: &str, mut f: F) {
    let bytes = s.as_bytes();
    let mut start = 0;
    for i in 0..bytes.len() {
        if bytes[i] == b'\n' {
            f(&s[start..i]);
            start = i + 1;
        }
    }
    if start <= bytes.len() {
        f(&s[start..]);
    }
}

/// Safe ASCII-only string slice conversion avoiding full UTF-8 table validation.
#[inline(always)]
pub fn from_ascii_bytes(bytes: &[u8]) -> Option<&str> {
    if bytes.iter().all(|&b| b < 128) {
        Some(unsafe { core::str::from_utf8_unchecked(bytes) })
    } else {
        None
    }
}

/// Standard uppercase hexadecimal characters (0-9, A-F)
pub const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

/// Format u32 as 8-character uppercase hexadecimal string into `buf`.
pub fn format_hex32(val: u32, buf: &mut [u8; 8]) -> &str {
    for i in 0..8 {
        buf[7 - i] = HEX_CHARS[((val >> (i * 4)) & 0x0F) as usize];
    }
    unsafe { core::str::from_utf8_unchecked(buf) }
}

/// Unified zero-alloc integer formatter supporting fixed-point decimals, prefix, and suffix.
pub fn format_int_fixed<'a>(
    val: i32,
    decimals: u8,
    prefix: &str,
    suffix: &str,
    buf: &'a mut [u8],
) -> &'a str {
    let mut i = buf.len();

    let s_bytes = suffix.as_bytes();
    if s_bytes.len() <= i {
        i -= s_bytes.len();
        buf[i..i + s_bytes.len()].copy_from_slice(s_bytes);
    }

    let is_negative = val < 0;
    let mut n = val.unsigned_abs();
    let mut digit_count = 0u8;

    loop {
        if decimals > 0 && digit_count == decimals {
            if i > 0 {
                i -= 1;
                buf[i] = b'.';
            }
        }
        let digit = (n % 10) as u8;
        n /= 10;
        if i > 0 {
            i -= 1;
            buf[i] = b'0' + digit;
        }
        digit_count += 1;
        if n == 0 && digit_count >= decimals {
            if digit_count == decimals && i > 0 {
                i -= 1;
                buf[i] = b'.';
                if i > 0 {
                    i -= 1;
                    buf[i] = b'0';
                }
            }
            break;
        }
    }

    if is_negative && i > 0 {
        i -= 1;
        buf[i] = b'-';
    }
    let p_bytes = prefix.as_bytes();
    if p_bytes.len() <= i {
        i -= p_bytes.len();
        buf[i..i + p_bytes.len()].copy_from_slice(p_bytes);
    }

    unsafe { core::str::from_utf8_unchecked(&buf[i..]) }
}

/// Format a message with prefix, number, and suffix without heap or formatting machinery.
pub fn format_num_msg<'a>(buf: &'a mut [u8], prefix: &str, val: u32, suffix: &str) -> &'a str {
    format_int_fixed(val as i32, 0, prefix, suffix, buf)
}

/// Format voltage in millivolts with 1 or 2 decimal digits (e.g. "8.24v" or "8.2v").
pub fn format_voltage<'a>(mv: u16, digits: u8, buf: &'a mut [u8; 12]) -> &'a str {
    let val = if digits == 2 { mv / 10 } else { mv / 100 };
    format_int_fixed(val as i32, digits, "", "v", buf)
}

/// Format a u32 with a single-byte suffix (e.g. "85%", "24C").
pub fn format_suffix_u32<'a>(val: u32, suffix: u8, buf: &'a mut [u8; 12]) -> &'a str {
    let s = [suffix];
    let s_str = unsafe { core::str::from_utf8_unchecked(&s) };
    format_int_fixed(val as i32, 0, "", s_str, buf)
}

/// Format a fraction (e.g. "0/16", "5/12").
pub fn format_fraction_u32<'a>(n: u32, d: u32, buf: &'a mut [u8; 12]) -> &'a str {
    let mut tmp = [0u8; 6];
    let s1 = format_u32(n, &mut tmp);
    let l1 = s1.len();
    buf[..l1].copy_from_slice(s1.as_bytes());
    buf[l1] = b'/';
    let mut tmp2 = [0u8; 6];
    let s2 = format_u32(d, &mut tmp2);
    let l2 = s2.len();
    buf[l1 + 1..l1 + 1 + l2].copy_from_slice(s2.as_bytes());
    unsafe { core::str::from_utf8_unchecked(&buf[..l1 + 1 + l2]) }
}

/// Format a range value with optional unit into buffer.
pub fn format_range_val<'a>(val: i32, min: i32, unit: &str, buf: &'a mut [u8]) -> &'a str {
    let pfx = if min < 0 && val > 0 { "+" } else { "" };
    format_int_fixed(val, 0, pfx, unit, buf)
}

/// Lightweight ASCII case-insensitive string equality without core unicode tables
pub fn ascii_eq_ignore_case(a: &str, b: &str) -> bool {
    let (a_bytes, b_bytes) = (a.as_bytes(), b.as_bytes());
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    for i in 0..a_bytes.len() {
        let ba = if (b'A'..=b'Z').contains(&a_bytes[i]) {
            a_bytes[i] + 32
        } else {
            a_bytes[i]
        };
        let bb = if (b'A'..=b'Z').contains(&b_bytes[i]) {
            b_bytes[i] + 32
        } else {
            b_bytes[i]
        };
        if ba != bb {
            return false;
        }
    }
    true
}

/// Strip trailing slashes from path without core pattern iterator
pub fn trim_trailing_slashes(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'/' {
        end -= 1;
    }
    &s[..end]
}

/// Format 6-byte MAC address into "XX:XX:XX:XX:XX:XX" uppercase hex string without formatting tables.
pub fn format_mac_address<'a>(mac: &[u8; 6], out: &'a mut [u8; 17]) -> &'a str {
    let mut idx = 0;
    for (i, &b) in mac.iter().enumerate() {
        out[idx] = HEX_CHARS[(b >> 4) as usize];
        out[idx + 1] = HEX_CHARS[(b & 0x0F) as usize];
        idx += 2;
        if i < 5 {
            out[idx] = b':';
            idx += 1;
        }
    }
    unsafe { core::str::from_utf8_unchecked(out) }
}

/// Format uptime seconds into "HH:MM:SS" (hours clamped at 99) without core formatting tables.
pub fn format_uptime_hms<'a>(secs: u32, out: &'a mut [u8; 8]) -> &'a str {
    let s = (secs % 60) as u8;
    let mins = secs / 60;
    let m = (mins % 60) as u8;
    let h = (mins / 60).min(99) as u8;
    let d0 = h / 10;
    out[0] = b'0' + d0;
    out[1] = b'0' + (h - d0 * 10);
    out[2] = b':';
    let d1 = m / 10;
    out[3] = b'0' + d1;
    out[4] = b'0' + (m - d1 * 10);
    out[5] = b':';
    let d2 = s / 10;
    out[6] = b'0' + d2;
    out[7] = b'0' + (s - d2 * 10);
    unsafe { core::str::from_utf8_unchecked(out) }
}
