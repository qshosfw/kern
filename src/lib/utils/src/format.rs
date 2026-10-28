//! Zero-allocation `#![no_std]` string formatting utilities.
//!
//! Provides deterministic formatting without linking `core::fmt` tables or heap allocations:
//! - `format_u32`, `format_i32`, `format_int_fixed`
//! - `format_hex16`, `format_hex32`, `format_hex8`
//! - `format_mac_address` (EUI-48)
//! - `format_uptime_hms` (HH:MM:SS)
//! - `ascii_eq_ignore_case`, `trim_trailing_slashes`

pub const HEX_CHARS: &[u8; 16] = b"0123456789ABCDEF";

/// Format a 32-bit unsigned integer into decimal ASCII digits.
pub fn format_u32<'a>(mut val: u32, out: &'a mut [u8]) -> &'a str {
    if val == 0 {
        if !out.is_empty() {
            out[0] = b'0';
            return unsafe { core::str::from_utf8_unchecked(&out[..1]) };
        }
        return "0";
    }

    let mut buf = [0u8; 10];
    let mut pos = 10;
    while val > 0 && pos > 0 {
        pos -= 1;
        buf[pos] = b'0' + (val % 10) as u8;
        val /= 10;
    }

    let digits = &buf[pos..];
    let copy_len = digits.len().min(out.len());
    out[..copy_len].copy_from_slice(&digits[..copy_len]);
    unsafe { core::str::from_utf8_unchecked(&out[..copy_len]) }
}

/// Format a 32-bit signed integer into decimal ASCII with negative sign.
pub fn format_i32<'a>(val: i32, out: &'a mut [u8]) -> &'a str {
    if val < 0 {
        if out.len() < 2 {
            return "-";
        }
        out[0] = b'-';
        let uval = val.unsigned_abs();
        let sub = format_u32(uval, &mut out[1..]);
        let total_len = 1 + sub.len();
        unsafe { core::str::from_utf8_unchecked(&out[..total_len]) }
    } else {
        format_u32(val as u32, out)
    }
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
    let plen = prefix.len().min(out.len());
    out[idx..idx + plen].copy_from_slice(prefix.as_bytes());
    idx += plen;

    // 2. Negative sign
    let is_neg = val < 0;
    let mut uval = val.unsigned_abs();
    if is_neg && idx < out.len() {
        out[idx] = b'-';
        idx += 1;
    }

    // 3. Digits & decimal point
    let mut digits = [0u8; 12];
    let mut dpos = 12;

    if decimals == 0 {
        if uval == 0 {
            dpos -= 1;
            digits[dpos] = b'0';
        } else {
            while uval > 0 && dpos > 0 {
                dpos -= 1;
                digits[dpos] = b'0' + (uval % 10) as u8;
                uval /= 10;
            }
        }
    } else {
        let mut d_count = 0;
        loop {
            dpos -= 1;
            digits[dpos] = b'0' + (uval % 10) as u8;
            uval /= 10;
            d_count += 1;

            if d_count == decimals {
                dpos -= 1;
                digits[dpos] = b'.';
            }

            if d_count >= decimals && uval == 0 {
                if digits[dpos] == b'.' {
                    dpos -= 1;
                    digits[dpos] = b'0';
                }
                break;
            }
            if dpos == 0 {
                break;
            }
        }
    }

    let d_slice = &digits[dpos..];
    let dlen = d_slice.len().min(out.len() - idx);
    out[idx..idx + dlen].copy_from_slice(&d_slice[..dlen]);
    idx += dlen;

    // 4. Suffix
    let slen = suffix.len().min(out.len() - idx);
    out[idx..idx + slen].copy_from_slice(suffix.as_bytes());
    idx += slen;

    unsafe { core::str::from_utf8_unchecked(&out[..idx]) }
}

/// Format a 32-bit integer into 8 uppercase hex digits.
pub fn format_hex32<'a>(val: u32, out: &'a mut [u8; 8]) -> &'a str {
    for i in 0..8 {
        out[7 - i] = HEX_CHARS[((val >> (i * 4)) & 0x0F) as usize];
    }
    unsafe { core::str::from_utf8_unchecked(out) }
}

/// Format a 16-bit integer into 4 uppercase hex digits.
pub fn format_hex16<'a>(val: u16, out: &'a mut [u8; 4]) -> &'a str {
    for i in 0..4 {
        out[3 - i] = HEX_CHARS[((val >> (i * 4)) & 0x0F) as usize];
    }
    unsafe { core::str::from_utf8_unchecked(out) }
}

/// Format a 6-byte MAC address into "XX:XX:XX:XX:XX:XX" uppercase hex string.
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

/// Format uptime seconds into "HH:MM:SS" (hours clamped at 99).
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

/// ASCII case-insensitive string equality comparison.
pub fn ascii_eq_ignore_case(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
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

/// Strip trailing slashes from path without core pattern iterator.
pub fn trim_trailing_slashes(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == b'/' {
        end -= 1;
    }
    &s[..end]
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
}
