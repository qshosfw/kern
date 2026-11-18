//! Zero-allocation `#![no_std]` string formatting utilities.
//!
//! Provides deterministic formatting without linking `core::fmt` tables or heap allocations:
//! - `format_u32`, `format_i32`, `format_int_fixed`
//! - `format_hex16`, `format_hex32`, `format_hex8`
//! - `format_mac_address` (EUI-48)
//! - `format_uptime_hms` (HH:MM:SS)
//! - `ascii_eq_ignore_case`, `trim_trailing_slashes`

pub const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

#[inline(never)]
fn push_bytes(out: &mut [u8], idx: &mut usize, src: &[u8]) {
    let n = src.len().min(out.len().saturating_sub(*idx));
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), out.as_mut_ptr().add(*idx), n);
    }
    *idx += n;
}

/// Format a 32-bit unsigned integer into decimal ASCII digits.
pub fn format_u32(mut val: u32, out: &mut [u8]) -> &str {
    let mut buf = [0u8; 10];
    let mut pos = 10;
    loop {
        pos -= 1;
        let (q, r) = crate::math::div10_u32(val);
        buf[pos] = b'0' + r;
        val = q;
        if val == 0 || pos == 0 {
            break;
        }
    }

    let mut idx = 0;
    push_bytes(out, &mut idx, unsafe { buf.get_unchecked(pos..) });
    unsafe { core::str::from_utf8_unchecked(out.get_unchecked(..idx)) }
}

/// Format a 32-bit signed integer into decimal ASCII with negative sign.
pub fn format_i32(val: i32, out: &mut [u8]) -> &str {
    format_int_fixed(val, 0, "", "", out)
}

/// Format a signed value with fixed decimal places and optional prefix/suffix.
pub fn format_int_fixed<'a>(
    val: i32,
    decimals: u8,
    prefix: &str,
    suffix: &str,
    out: &'a mut [u8],
) -> &'a str {
    let mut idx = 0;

    // 1. Prefix
    push_bytes(out, &mut idx, prefix.as_bytes());

    // 2. Negative sign
    let mut uval = val.unsigned_abs();
    if val < 0 {
        push_bytes(out, &mut idx, b"-");
    }

    // 3. Digits & decimal point
    let mut digits = [0u8; 12];
    let mut dpos = 12usize;
    let mut d_count = 0u8;
    loop {
        dpos -= 1;
        let (q, r) = crate::math::div10_u32(uval);
        digits[dpos] = b'0' + r;
        uval = q;
        d_count += 1;

        if d_count == decimals && dpos > 0 {
            dpos -= 1;
            digits[dpos] = b'.';
        }

        if d_count >= decimals && uval == 0 {
            if digits[dpos] == b'.' && dpos > 0 {
                dpos -= 1;
                digits[dpos] = b'0';
            }
            break;
        }
        if dpos == 0 {
            break;
        }
    }

    push_bytes(out, &mut idx, unsafe { digits.get_unchecked(dpos..) });

    // 4. Suffix
    push_bytes(out, &mut idx, suffix.as_bytes());

    unsafe { core::str::from_utf8_unchecked(out.get_unchecked(..idx)) }
}

/// Format a 32-bit integer into 8 uppercase hex digits.
pub fn format_hex32(val: u32, out: &mut [u8; 8]) -> &str {
    for i in 0..8 {
        out[7 - i] = HEX_CHARS[((val >> (i * 4)) & 0x0F) as usize];
    }
    unsafe { core::str::from_utf8_unchecked(out) }
}

/// Format a 16-bit integer into 4 uppercase hex digits.
pub fn format_hex16(val: u16, out: &mut [u8; 4]) -> &str {
    for i in 0..4 {
        out[3 - i] = HEX_CHARS[((val >> (i * 4)) & 0x0F) as usize];
    }
    unsafe { core::str::from_utf8_unchecked(out) }
}

/// Format a 6-byte MAC address into "XX:XX:XX:XX:XX:XX" uppercase hex string.
pub fn format_mac_address<'a>(_mac: &[u8; 6], out: &'a mut [u8; 17]) -> &'a str {
    let mut idx = 0;
    for (i, &b) in _mac.iter().enumerate() {
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

#[inline(never)]
fn put_2digit(out: &mut [u8; 8], off: usize, val: u8) {
    let (d, r) = crate::math::div10_u16(val as u16);
    out[off] = b'0' + d as u8;
    out[off + 1] = b'0' + r;
}

/// Format uptime seconds into "HH:MM:SS" (hours clamped at 99).
pub fn format_uptime_hms(secs: u32, out: &mut [u8; 8]) -> &str {
    let mins = secs / 60;
    let s = (secs - mins * 60) as u8;
    let (h, m) = crate::math::div60_u16(mins.min(5999) as u16);
    put_2digit(out, 0, h as u8);
    out[2] = b':';
    put_2digit(out, 3, m);
    out[5] = b':';
    put_2digit(out, 6, s);
    unsafe { core::str::from_utf8_unchecked(out) }
}

/// ASCII case-insensitive string equality comparison.
pub fn ascii_eq_ignore_case(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    for i in 0..a_bytes.len() {
        let ba = if a_bytes[i].is_ascii_uppercase() {
            a_bytes[i] + 32
        } else {
            a_bytes[i]
        };
        let bb = if b_bytes[i].is_ascii_uppercase() {
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

/// Strip trailing slashes from path without core pattern iterator.
pub fn trim_trailing_slashes(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'/' {
        end -= 1;
    }
    &s[..end]
}

/// Validate that `bytes` is pure 7-bit ASCII and return it as `&str` without pulling
/// `core::str::from_utf8` (368 B) into Cortex-M0+ flash.
#[inline(never)]
pub fn ascii_str(bytes: &[u8]) -> Option<&str> {
    if bytes.iter().all(|&b| b < 0x80) {
        // Safety: all bytes are < 0x80, which is valid UTF-8.
        Some(unsafe { core::str::from_utf8_unchecked(bytes) })
    } else {
        None
    }
}

#[inline(never)]
pub fn trim_ascii_bytes(mut b: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = b {
        if !first.is_ascii_whitespace() {
            break;
        }
        b = rest;
    }
    while let [rest @ .., last] = b {
        if !last.is_ascii_whitespace() {
            break;
        }
        b = rest;
    }
    b
}

/// Extract the `idx`-th NUL-delimited UTF-8 string from `packed`.
///
/// Packing string enum tables into one NUL-delimited byte slice avoids per-variant
/// `(ptr, len)` descriptor tables in `.rodata` (8 bytes saved per entry).
#[inline(never)]
pub const fn nth_cstr<'a>(packed: &'a [u8], mut idx: u8) -> &'a str {
    let mut start = 0usize;
    let mut i = 0usize;
    while i < packed.len() {
        if packed[i] == 0 {
            if idx == 0 {
                let slice =
                    unsafe { core::slice::from_raw_parts(packed.as_ptr().add(start), i - start) };
                return unsafe { core::str::from_utf8_unchecked(slice) };
            }
            idx -= 1;
            start = i + 1;
        }
        i += 1;
    }
    if idx == 0 && start < packed.len() {
        let slice = unsafe {
            core::slice::from_raw_parts(packed.as_ptr().add(start), packed.len() - start)
        };
        return unsafe { core::str::from_utf8_unchecked(slice) };
    }
    ""
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
    }

    #[test]
    fn test_nth_cstr() {
        const TABLE: &[u8] = b"alpha\0beta\0\0delta\0";
        assert_eq!(nth_cstr(TABLE, 0), "alpha");
        assert_eq!(nth_cstr(TABLE, 1), "beta");
        assert_eq!(nth_cstr(TABLE, 2), "");
        assert_eq!(nth_cstr(TABLE, 3), "delta");
        assert_eq!(nth_cstr(TABLE, 4), "");

        const UNTERM: &[u8] = b"System\0Device\0Status\0Legal";
        assert_eq!(nth_cstr(UNTERM, 3), "Legal");
        assert_eq!(nth_cstr(UNTERM, 4), "");
    }
}
