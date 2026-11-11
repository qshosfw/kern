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
}
