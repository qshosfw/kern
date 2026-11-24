use crate::icons::FALLBACK_BOX_5;
use crate::{Canvas, Color, GfxFont};
pub use utils::format::*;

/// External Unicode BMP glyph draw callback `(canvas, x, y, cp, min_x, max_x, color) -> Option<adv>`.
pub type ExtGlyphDrawFn = fn(&mut Canvas, i16, i16, u16, i16, i16, Color) -> Option<i16>;
/// External Unicode BMP glyph width callback `(cp) -> Option<adv>`.
pub type ExtGlyphWidthFn = fn(u16) -> Option<i16>;
/// Optional runtime localization callback `(english_key, prefer_short, out_buf) -> Option<&str>`.
pub type LocalizeHookFn = for<'a> fn(&str, bool, &'a mut [u8]) -> Option<&'a str>;

static mut EXT_GLYPH_DRAW: Option<ExtGlyphDrawFn> = None;
static mut EXT_GLYPH_WIDTH: Option<ExtGlyphWidthFn> = None;
static mut LOCALIZE_HOOK: Option<LocalizeHookFn> = None;
/// Single shared Thread-mode scratch buffer for localized UI labels, eliminating
/// 32-64 B stack buffers across every widget, menu row, dialog, and GfxText syscall.
static mut LOC_SCRATCH: [u8; 64] = [0u8; 64];

/// Register optional external `.fnt` hooks for non-ASCII UTF-8 codepoints (Polish, Portuguese,
/// Italian, Cyrillic, Japanese, Chinese, etc.). ASCII characters always stay zero-cost in ROM.
#[inline(always)]
pub fn set_ext_font_hooks(draw: Option<ExtGlyphDrawFn>, width: Option<ExtGlyphWidthFn>) {
    unsafe {
        EXT_GLYPH_DRAW = draw;
        EXT_GLYPH_WIDTH = width;
    }
}

/// Register or clear the OS runtime localization hook. When `None` (English default),
/// [`localize`] returns the English literal immediately with zero I/O or copying.
#[inline(always)]
pub fn set_localize_hook(hook: Option<LocalizeHookFn>) {
    unsafe {
        LOCALIZE_HOOK = hook;
    }
}

/// Returns `true` when an external language pack localization hook is active.
#[inline(always)]
pub fn has_localize_hook() -> bool {
    unsafe { (*core::ptr::addr_of!(LOCALIZE_HOOK)).is_some() }
}

/// Translate an English UI key via the active language pack hook (if registered),
/// falling back to `key` untouched on miss, empty input, or numeric strings.
#[inline(never)]
pub fn localize<'a>(key: &'a str, prefer_short: bool, out: &'a mut [u8]) -> &'a str {
    let bytes = key.as_bytes();
    if bytes.is_empty() || bytes[0].is_ascii_digit() {
        return key;
    }
    if let Some(hook) = unsafe { LOCALIZE_HOOK }
        && let Some(s) = hook(key, prefer_short, out)
    {
        return s;
    }
    key
}

/// Zero-stack-buffer localization into the shared Thread-mode `LOC_SCRATCH` buffer.
/// Callers must finish using the returned `&str` before invoking `localize_scratch` again.
#[inline(never)]
pub fn localize_scratch<'a>(key: &'a str, prefer_short: bool) -> &'a str {
    let out = unsafe { &mut *core::ptr::addr_of_mut!(LOC_SCRATCH) };
    localize(key, prefer_short, out)
}

/// Decode one UTF-8 character starting at `bytes[i]`, returning `(codepoint_u32, byte_len)`.
#[inline(never)]
fn decode_utf8_at(bytes: &[u8], i: usize) -> (u32, usize) {
    let b0 = unsafe { *bytes.get_unchecked(i) };
    if b0 < 0x80 {
        return (b0 as u32, 1);
    }
    let rem = bytes.len() - i;
    if (b0 & 0xE0) == 0xC0 && rem >= 2 {
        let b1 = unsafe { *bytes.get_unchecked(i + 1) };
        return ((((b0 & 0x1F) as u32) << 6) | ((b1 & 0x3F) as u32), 2);
    }
    if (b0 & 0xF0) == 0xE0 && rem >= 3 {
        let b1 = unsafe { *bytes.get_unchecked(i + 1) };
        let b2 = unsafe { *bytes.get_unchecked(i + 2) };
        let cp = (((b0 & 0x0F) as u32) << 12) | (((b1 & 0x3F) as u32) << 6) | ((b2 & 0x3F) as u32);
        return (cp, 3);
    }
    if (b0 & 0xF8) == 0xF0 && rem >= 4 {
        let b1 = unsafe { *bytes.get_unchecked(i + 1) };
        let b2 = unsafe { *bytes.get_unchecked(i + 2) };
        let b3 = unsafe { *bytes.get_unchecked(i + 3) };
        let cp = (((b0 & 0x07) as u32) << 18)
            | (((b1 & 0x3F) as u32) << 12)
            | (((b2 & 0x3F) as u32) << 6)
            | ((b3 & 0x3F) as u32);
        return (cp, 4);
    }
    (0xFFFD, 1)
}

/// Draw a single ASCII character clipped horizontally to `[min_x, max_x)`.
#[inline(never)]
pub fn draw_char_clipped(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    c: char,
    font: &GfxFont,
    color: Color,
    min_x: i16,
    max_x: i16,
) -> i16 {
    let code = c as u32;

    // Render degree symbol '°' (U+00B0)
    if c == '°' || code == 0xB0 {
        if y >= 4 {
            for dx in 0..2i16 {
                let px = x + dx;
                if px >= min_x && px < max_x && (px as u16) < 128 {
                    canvas.draw_pixel(px as u8, (y - 4) as u8, color);
                    canvas.draw_pixel(px as u8, (y - 3) as u8, color);
                }
            }
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

    let glyph = unsafe { font.glyphs.get_unchecked(glyph_idx) };
    let adv = glyph.x_advance() as i16;
    let width = glyph.width() as usize;
    let x_base = x + glyph.x_offset() as i16;
    if x_base + (width as i16) <= min_x || x_base >= max_x {
        return adv;
    }

    let bitmap = font.bitmap;
    let height = glyph.height() as usize;
    let y_base = y + glyph.y_offset() as i16;
    let mut bit_idx = (glyph.bitmap_offset as usize) * 8;

    for gy in 0..height {
        let py = y_base + gy as i16;
        for gx in 0..width {
            let byte_idx = bit_idx >> 3;
            let bit_shift = 7 - (bit_idx & 7);

            if byte_idx < bitmap.len()
                && ((unsafe { *bitmap.get_unchecked(byte_idx) } >> bit_shift) & 1) != 0
            {
                let px = x_base + gx as i16;
                if px >= min_x && px < max_x && (px as u16) < 128 && (py as u16) < 64 {
                    canvas.draw_pixel(px as u8, py as u8, color);
                }
            }

            bit_idx += 1;
        }
    }

    adv
}

/// Draw a single ASCII character using the provided GfxFont.
#[inline(never)]
pub fn draw_char(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    c: char,
    font: &GfxFont,
    color: Color,
) -> i16 {
    draw_char_clipped(canvas, x, y, c, font, color, 0, 128)
}

/// Draw a string horizontally scrolled by `scroll_x` pixels and clipped strictly to `[min_x, max_x)`.
/// Enables smooth live marquee scrolling without bleeding over adjacent icons or values.
#[inline(never)]
pub fn draw_str_clipped(
    canvas: &mut Canvas,
    min_x: i16,
    max_x: i16,
    scroll_x: i16,
    y: i16,
    text: &str,
    font: &GfxFont,
    color: Color,
) -> i16 {
    let mut x = min_x - scroll_x;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let (adv, step) = if b < 0x80 {
            (
                draw_char_clipped(canvas, x, y, b as char, font, color, min_x, max_x),
                1,
            )
        } else {
            let (cp, step) = decode_utf8_at(bytes, i);
            let adv = if cp == 0xB0 {
                draw_char_clipped(canvas, x, y, '°', font, color, min_x, max_x)
            } else if cp <= u16::MAX as u32
                && let Some(hook) = unsafe { EXT_GLYPH_DRAW }
                && let Some(ext_adv) = hook(canvas, x, y, cp as u16, min_x, max_x, color)
            {
                ext_adv
            } else {
                crate::media::draw_bitmap_clipped(
                    canvas,
                    x,
                    y - 5,
                    5,
                    5,
                    &FALLBACK_BOX_5,
                    min_x,
                    max_x,
                    color,
                );
                6
            };
            (adv, step)
        };
        x += adv;
        if x >= max_x {
            break;
        }
        i += step;
    }
    x
}

/// Draw a string clipped to `max_x`, appending `".."` when truncated so overflow is legible.
#[inline(never)]
pub fn draw_str_ellipsis(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    text: &str,
    font: &GfxFont,
    color: Color,
    max_x: i16,
) -> i16 {
    let avail = max_x - x;
    if avail <= 0 {
        return x;
    }
    if str_width(text, font) <= avail {
        return draw_str_clipped(canvas, x, max_x, 0, y, text, font, color);
    }
    let dots_w = str_width("..", font);
    let body_max = (max_x - dots_w).max(x);
    draw_str_clipped(canvas, x, body_max, 0, y, text, font, color);
    draw_str_clipped(canvas, body_max, max_x, 0, y, "..", font, color)
}

/// Draw a string onto the canvas using the provided GfxFont.
#[inline(never)]
pub fn draw_str(canvas: &mut Canvas, x: i16, y: i16, text: &str, font: &GfxFont, color: Color) {
    draw_str_clipped(canvas, x, 128, 0, y, text, font, color);
}

/// Draw a string onto the canvas up to a maximum X coordinate. Returns the advanced X.
#[inline(never)]
pub fn draw_str_max_x(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    text: &str,
    font: &GfxFont,
    color: Color,
    max_x: i16,
) -> i16 {
    draw_str_clipped(canvas, x, max_x, 0, y, text, font, color)
}

/// Calculate the total pixel width of a string formatted with the specified GfxFont.
#[inline(never)]
pub fn str_width(text: &str, font: &GfxFont) -> i16 {
    let mut width = 0i16;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            let code = b as u32;
            if code >= font.first as u32 && code <= font.last as u32 {
                let idx = (code - font.first as u32) as usize;
                if idx < font.glyphs.len() {
                    width += font.glyphs[idx].x_advance() as i16;
                }
            }
            i += 1;
        } else {
            let (cp, step) = decode_utf8_at(bytes, i);
            width += if cp == 0xB0 {
                3
            } else if cp <= u16::MAX as u32
                && let Some(hook) = unsafe { EXT_GLYPH_WIDTH }
                && let Some(ext_w) = hook(cp as u16)
            {
                ext_w
            } else {
                6
            };
            i += step;
        }
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
        let line = match find_byte(self.rest, b'\n') {
            Some(i) => unsafe {
                let head = self.rest.get_unchecked(..i);
                self.rest = self.rest.get_unchecked(i + 1..);
                head
            },
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
