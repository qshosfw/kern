//! Silicon-optimized reciprocal arithmetic primitives for ARMv6-M (Cortex-M0+).
//!
//! Cortex-M0+ lacks a hardware divider. Direct `/` and `%` operators lower to
//! `__aeabi_uidiv` and `__aeabi_uidivmod`, each taking 40–100 clock cycles and
//! bloating flash with call-site register marshalling.
//!
//! These functions replace software division with single-cycle fixed-point
//! reciprocal multiplication and bit shifts.

/// Fast reciprocal division and modulo by 10 for 16-bit unsigned integers.
///
/// Exact for all `x <= 65,535`.
/// Uses a single 32-bit single-cycle multiply: `((x * 0xCCCD) >> 19)`.
#[inline(always)]
pub const fn div10_u16(x: u16) -> (u16, u8) {
    let q = ((x as u32 * 0xCCCD) >> 19) as u16;
    let r = (x - q * 10) as u8;
    (q, r)
}

/// Fast reciprocal division and modulo by 10 for 32-bit unsigned integers.
///
/// Exact for all `x <= 4,294,967,295`.
/// `((x * 0xCCCC_CCCD) >> 35)`.
#[inline(always)]
pub const fn div10_u32(x: u32) -> (u32, u8) {
    let q = ((x as u64 * 0xCCCC_CCCD) >> 35) as u32;
    let r = (x - q * 10) as u8;
    (q, r)
}

/// Fast reciprocal division and modulo by 100 for 16-bit unsigned integers.
///
/// Exact for all `x <= 65,535` via chained single-cycle `div10_u16`.
#[inline(always)]
pub const fn div100_u16(x: u16) -> (u16, u8) {
    let (q10, _) = div10_u16(x);
    let (q100, _) = div10_u16(q10);
    let r = (x - q100 * 100) as u8;
    (q100, r)
}

/// Fast reciprocal division and modulo by 3 for integers `<= 1024`.
///
/// Exact for all `x <= 1,024`.
#[inline(always)]
pub const fn div3_u16(x: u16) -> (u16, u8) {
    let q = ((x as u32 * 0xAAAB) >> 17) as u16;
    let r = (x - q * 3) as u8;
    (q, r)
}

/// Fast reciprocal division and modulo by 37 for 16-bit unsigned integers.
///
/// Exact for all `x <= 65,535`.
/// Uses a single 32-bit single-cycle multiply: `((x * 0x1BAD) >> 18)`.
#[inline(always)]
pub const fn div37_u16(x: u16) -> (u16, u8) {
    let q = ((x as u32 * 0x1BAD) >> 18) as u16;
    let r = (x - q * 37) as u8;
    (q, r)
}

/// Fast modulo by 37 for 32-bit unsigned integers without 64-bit multiplication.
///
/// Exact for all `x <= 4,294,967,295`.
/// Decomposes 32-bit `x` into two 16-bit halves using `2^16 = 65536 ≡ 9 (mod 37)`.
#[inline(always)]
pub const fn rem37_u32(x: u32) -> u8 {
    let hi = (x >> 16) as u16;
    let lo = (x & 0xFFFF) as u16;
    let r_hi = div37_u16(hi).1 as u16;
    let r_lo = div37_u16(lo).1 as u16;
    let sum_r = r_hi * 9 + r_lo;
    div37_u16(sum_r).1
}

/// Fast reciprocal division and modulo by 60 for 16-bit unsigned integers.
///
/// Exact for all `x <= 65,535`.
/// Uses single-cycle 32-bit multiply: `((x * 0x8889) >> 21)`.
#[inline(always)]
pub const fn div60_u16(x: u16) -> (u16, u8) {
    let q = ((x as u32 * 0x8889) >> 21) as u16;
    let r = (x - q * 60) as u8;
    (q, r)
}

/// Fast reciprocal division and modulo by 60 for 32-bit unsigned integers.
///
/// Exact for all `x <= 50,000,000` (over 1.5 years of uptime seconds).
#[inline(always)]
pub const fn div60_u32(x: u32) -> (u32, u8) {
    let q = ((x as u64 * 0x0444_4445) >> 32) as u32;
    let r = (x - q * 60) as u8;
    (q, r)
}

/// Fast reciprocal division by 7600 for unsigned 32-bit integers.
///
/// Exact for all `x <= 52,500,000` (battery millivolt / calibration conversions).
#[inline(always)]
pub const fn div7600_u32(x: u32) -> u32 {
    ((x as u64 * 0x227E_1D1B) >> 42) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_div10_u16_all_values() {
        for x in 0..=65535u16 {
            let (q, r) = div10_u16(x);
            assert_eq!(q, x / 10, "q mismatch at {}", x);
            assert_eq!(r as u16, x % 10, "r mismatch at {}", x);
        }
    }

    #[test]
    fn test_div100_u16_all_values() {
        for x in 0..=65535u16 {
            let (q, r) = div100_u16(x);
            assert_eq!(q, x / 100, "q mismatch at {}", x);
            assert_eq!(r as u16, x % 100, "r mismatch at {}", x);
        }
    }

    #[test]
    fn test_div10_u32_selected_values() {
        let test_cases = [
            0, 1, 9, 10, 11, 99, 100, 1234, 65535, 145000, 1000000, 429496729, 4294967290,
            4294967295,
        ];
        for &x in &test_cases {
            let (q, r) = div10_u32(x);
            assert_eq!(q, x / 10, "q mismatch at {}", x);
            assert_eq!(r as u32, x % 10, "r mismatch at {}", x);
        }
    }

    #[test]
    fn test_div3_u16_range() {
        for x in 0..=1024u16 {
            let (q, r) = div3_u16(x);
            assert_eq!(q, x / 3, "q mismatch at {}", x);
            assert_eq!(r as u16, x % 3, "r mismatch at {}", x);
        }
    }

    #[test]
    fn test_div37_u16_all_values() {
        for x in 0..=65535u16 {
            let (q, r) = div37_u16(x);
            assert_eq!(q, x / 37, "q mismatch at {}", x);
            assert_eq!(r as u16, x % 37, "r mismatch at {}", x);
        }
    }

    #[test]
    fn test_rem37_u32_selected_values() {
        let test_cases = [
            0, 1, 36, 37, 38, 73, 74, 999, 65535, 65536, 1000000, 2147483647, 4294967295,
        ];
        for &x in &test_cases {
            assert_eq!(rem37_u32(x) as u32, x % 37, "rem37 mismatch at {}", x);
        }
    }

    #[test]
    fn test_div60_u16_all_values() {
        for x in 0..=65535u16 {
            let (q, r) = div60_u16(x);
            assert_eq!(q, x / 60, "q mismatch at {}", x);
            assert_eq!(r as u16, x % 60, "r mismatch at {}", x);
        }
    }

    #[test]
    fn test_div60_u32_selected_values() {
        let test_cases = [
            0, 1, 59, 60, 61, 3599, 3600, 3601, 86400, 360000, 1000000, 10000000, 50000000,
        ];
        for &x in &test_cases {
            let (q, r) = div60_u32(x);
            assert_eq!(q, x / 60, "q mismatch at {}", x);
            assert_eq!(r as u32, x % 60, "r mismatch at {}", x);
        }
    }

    #[test]
    fn test_div7600_u32_selected_values() {
        let test_cases = [
            0, 1, 7599, 7600, 7601, 15200, 1000000, 15000 * 3500, 52500000,
        ];
        for &x in &test_cases {
            assert_eq!(div7600_u32(x), x / 7600, "div7600 mismatch at {}", x);
        }
    }
}
