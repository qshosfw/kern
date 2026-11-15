use crate::{Canvas, Color, GfxFont};
pub use utils::format::*;

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

/// Draw a string onto the canvas up to a maximum X coordinate. Returns the advanced X.
pub fn draw_str_max_x(
    canvas: &mut Canvas,
    mut x: i16,
    y: i16,
    text: &str,
    font: &GfxFont,
    color: Color,
    max_x: i16,
) -> i16 {
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
    x
}

/// Calculate the total pixel width of a string formatted with the specified GfxFont.
#[inline(never)]
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
#[inline(never)]
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
        Align::Center => x - (w >> 1),
        Align::Right => x - w,
    };
    draw_str(canvas, start_x, y, text, font, color);
}

/// Stack-allocated zero-copy u8 formatter for `no_std` text rendering.
#[inline(always)]
pub fn format_u8(val: u8, buf: &mut [u8]) -> &str {
    format_u32(val as u32, buf)
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
#[inline(never)]
pub fn count_lines(s: &str) -> usize {
    if s.is_empty() {
        return 0;
    }
    let mut n = 1;
    for &b in s.as_bytes() {
        if b == b'\n' {
            n += 1;
        }
    }
    n
}

/// Zero-allocation line iterator splitting on ASCII `'\n'`.
pub struct Lines<'a> {
    rest: &'a [u8],
    done: bool,
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<&'a str> {
        if self.done {
            return None;
        }
        let line = match self.rest.iter().position(|&b| b == b'\n') {
            Some(i) => {
                let head = &self.rest[..i];
                self.rest = &self.rest[i + 1..];
                head
            }
            None => {
                self.done = true;
                self.rest
            }
        };
        // SAFETY: `b'\n'` (0x0A) is never a UTF-8 continuation byte, so splitting on it preserves UTF-8.
        Some(unsafe { core::str::from_utf8_unchecked(line) })
    }
}

/// Iterate over lines of text without pulling in `Split<char>` or generic closures.
#[inline(always)]
pub fn lines(s: &str) -> Lines<'_> {
    Lines {
        rest: s.as_bytes(),
        done: false,
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

/// Format a message with prefix, number, and suffix without heap or formatting machinery.
pub fn format_num_msg<'a>(buf: &'a mut [u8], prefix: &str, val: u32, suffix: &str) -> &'a str {
    format_int_fixed(val as i32, 0, prefix, suffix, buf)
}

/// Format voltage in millivolts with 1 or 2 decimal digits (e.g. "8.24v" or "8.2v").
pub fn format_voltage(mv: u16, digits: u8, buf: &mut [u8; 12]) -> &str {
    let val = if digits == 2 {
        utils::math::div10_u16(mv).0
    } else {
        utils::math::div100_u16(mv).0
    };
    format_int_fixed(val as i32, digits, "", "v", buf)
}

/// Format a u32 with a single-byte suffix (e.g. "85%", "24C").
pub fn format_suffix_u32(val: u32, suffix: u8, buf: &mut [u8; 12]) -> &str {
    let s = [suffix];
    let s_str = unsafe { core::str::from_utf8_unchecked(&s) };
    format_int_fixed(val as i32, 0, "", s_str, buf)
}

/// Format a fraction (e.g. "0/16", "5/12").
pub fn format_fraction_u32(n: u32, d: u32, buf: &mut [u8; 12]) -> &str {
    let l1 = format_u32(n, &mut buf[..6]).len();
    buf[l1] = b'/';
    let l2 = format_u32(d, &mut buf[l1 + 1..]).len();
    unsafe { core::str::from_utf8_unchecked(&buf[..l1 + 1 + l2]) }
}

/// Format a range value with optional unit into buffer.
pub fn format_range_val<'a>(val: i32, min: i32, unit: &str, buf: &'a mut [u8]) -> &'a str {
    let pfx = if min < 0 && val > 0 { "+" } else { "" };
    format_int_fixed(val, 0, pfx, unit, buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_u32_and_i32() {
        let mut buf = [0u8; 16];
        assert_eq!(format_u32(0, &mut buf), "0");
        assert_eq!(format_u32(145000, &mut buf), "145000");
        assert_eq!(format_u32(4294967295, &mut buf), "4294967295");

        assert_eq!(format_i32(0, &mut buf), "0");
        assert_eq!(format_i32(-42, &mut buf), "-42");
        assert_eq!(format_i32(1234, &mut buf), "1234");

        assert_eq!(format_i32_range(5, -10, &mut buf), "+5");
        assert_eq!(format_i32_range(0, -10, &mut buf), "0");
        assert_eq!(format_i32_range(-5, -10, &mut buf), "-5");
    }

    #[test]
    fn test_format_int_fixed_decimals() {
        let mut buf = [0u8; 16];
        assert_eq!(format_int_fixed(824, 2, "", "v", &mut buf), "8.24v");
        assert_eq!(format_int_fixed(50, 1, "", " MHz", &mut buf), "5.0 MHz");
        assert_eq!(format_int_fixed(-125, 2, "dB ", "m", &mut buf), "dB -1.25m");
    }

    #[test]
    fn test_format_hex() {
        let mut b8 = [0u8; 8];
        assert_eq!(format_hex32(0x1234ABCD, &mut b8), "1234ABCD");
        assert_eq!(format_hex32(0, &mut b8), "00000000");

        let mut b4 = [0u8; 4];
        assert_eq!(format_hex16(0x00FF, &mut b4), "00FF");
        assert_eq!(format_hex16(0xBEEF, &mut b4), "BEEF");
    }

    #[test]
    fn test_format_mac_and_uptime() {
        let mac = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC];
        let mut mac_buf = [0u8; 17];
        assert_eq!(format_mac_address(&mac, &mut mac_buf), "12:34:56:78:9A:BC");

        let mut up_buf = [0u8; 8];
        assert_eq!(format_uptime_hms(0, &mut up_buf), "00:00:00");
        assert_eq!(format_uptime_hms(3661, &mut up_buf), "01:01:01");
        assert_eq!(format_uptime_hms(359999, &mut up_buf), "99:59:59");
    }

    #[test]
    fn test_ascii_eq_ignore_case() {
        assert!(ascii_eq_ignore_case("RADIO", "radio"));
        assert!(ascii_eq_ignore_case("Vfo_A.vfo", "vfo_a.vfo"));
        assert!(!ascii_eq_ignore_case("abc", "abd"));
        assert!(!ascii_eq_ignore_case("abc", "abcd"));
        assert!(!ascii_eq_ignore_case("abcd", "abc"));
    }

    #[test]
    fn test_trim_trailing_slashes() {
        assert_eq!(trim_trailing_slashes("/apps///"), "/apps");
        assert_eq!(trim_trailing_slashes("/apps/"), "/apps");
        assert_eq!(trim_trailing_slashes("test.krx"), "test.krx");
        assert_eq!(trim_trailing_slashes("///"), "");
    }
}
