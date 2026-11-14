//! Vernier Fractional-N Sub-PLL Solver for Sub-Hz Frequency Synthesis.
//!
//! Grounded in the silicon architecture of the BK4819/BK4829 transceiver:
//! Combines fractional crystal reference trimming (varactor load capacitance via register 0x3B)
//! with discrete 10 Hz integer PLL steps (registers 0x38/0x39) to achieve sub-Hz (and sub-0.1 Hz)
//! carrier frequency precision for narrowband digital modes (FT8, WSPR, JS8Call, narrow FSK, CW).
//!
//! Math:
//! - Nominal reference crystal: 26 MHz.
//! - PLL step: 10 Hz (10,000 mHz).
//! - Crystal trim sensitivity: alpha = (5000 * f_carrier_hz) / 26_000_000 (mHz per LSB).
//! - Sub-Hz synthesis equation:
//!     actual_offset = (xtal_trim * alpha) - (pll_comp * 10_000) mHz
//!
//! Strictly `#![no_std]`, zero heap allocation, bounded O(1) runtime.

pub const PLL_STEP_MHZ: i32 = 10_000; // 10 Hz expressed in millihertz
pub const MAX_XTAL_TRIM: u16 = 600;

/// Output of the Vernier solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VernierResult {
    /// Signed offset to apply to the calibrated crystal trim register.
    pub xtal_trim_offset: i16,
    /// Signed 10-Hz steps to apply to the PLL dividers.
    pub pll_comp_10hz: i16,
    /// Residual synthesis error in millihertz (mHz).
    pub residual_error_mhz: i16,
}

impl VernierResult {
    pub const fn zero() -> Self {
        Self {
            xtal_trim_offset: 0,
            pll_comp_10hz: 0,
            residual_error_mhz: 0,
        }
    }
}

/// Compute crystal trim sensitivity alpha in millihertz per LSB of the crystal trim register.
///
/// For a 26 MHz nominal reference crystal:
/// alpha = (5000 * f_carrier_hz) / 26_000_000 (mHz per LSB).
#[inline]
pub fn compute_vernier_alpha(carrier_hz: u32) -> u32 {
    carrier_hz / 5200
}

/// Solve for the optimal `(xtal_trim_offset, pll_comp_10hz)` pair to achieve `delta_dhz`.
///
/// - `carrier_hz`: Nominal RF carrier frequency in Hertz (e.g. 144_000_000).
/// - `delta_dhz`: Target frequency offset in deci-Hertz (0.1 Hz increments, e.g. +62 = +6.2 Hz, -1700 = -170 Hz).
pub fn solve_vernier(carrier_hz: u32, delta_dhz: i16) -> VernierResult {
    if delta_dhz == 0 {
        return VernierResult::zero();
    }

    let alpha_mhz = compute_vernier_alpha(carrier_hz) as i32;
    if alpha_mhz == 0 {
        return VernierResult::zero();
    }

    let negative = delta_dhz < 0;
    let target_mhz = (delta_dhz as i32).abs() * 100;

    let mut best_xt: u16 = 0;
    let mut best_pc: u16 = 0;
    let mut min_err: i32 = i32::MAX;
    let mut numer = (PLL_STEP_MHZ / 2) - target_mhz;
    let mut pc: u16 = 0;

    for xt in 0..=MAX_XTAL_TRIM {
        if numer >= 0 {
            while numer >= PLL_STEP_MHZ {
                numer -= PLL_STEP_MHZ;
                pc += 1;
            }
            let err = (numer - (PLL_STEP_MHZ / 2)).abs();
            if err < min_err {
                min_err = err;
                best_xt = xt;
                best_pc = pc;
                if err == 0 {
                    break;
                }
            }
        }
        numer += alpha_mhz;
    }

    let (xtal_trim_offset, pll_comp_10hz) = if negative {
        (-(best_xt as i16), -(best_pc as i16))
    } else {
        (best_xt as i16, best_pc as i16)
    };
    VernierResult {
        xtal_trim_offset,
        pll_comp_10hz,
        residual_error_mhz: min_err as i16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vernier_alpha_calculation() {
        // 144 MHz: 5000 * 144_000_000 / 26_000_000 = 720_000_000_000 / 26_000_000 = 27692 mHz/LSB (~27.69 Hz/LSB)
        let alpha_144 = compute_vernier_alpha(144_000_000);
        assert_eq!(alpha_144, 27692);

        // 433 MHz: 5000 * 433_000_000 / 26_000_000 = 2_165_000_000_000 / 26_000_000 = 83269 mHz/LSB (~83.27 Hz/LSB)
        let alpha_433 = compute_vernier_alpha(433_000_000);
        assert_eq!(alpha_433, 83269);
    }

    #[test]
    fn test_vernier_zero_offset() {
        let res = solve_vernier(144_000_000, 0);
        assert_eq!(res, VernierResult::zero());
    }

    #[test]
    fn test_vernier_sub_hz_precision() {
        // Target: +6.25 Hz offset (62 deci-Hz = 6200 mHz, FT8 tone step) on 144 MHz
        let res = solve_vernier(144_000_000, 62);
        // Error should be less than 50 mHz (0.05 Hz)
        assert!(res.residual_error_mhz < 50);

        // Negative offset: -170 Hz (RTTY shift)
        let res_neg = solve_vernier(144_000_000, -1700);
        assert!(res_neg.residual_error_mhz < 50);
        assert!(res_neg.xtal_trim_offset <= 0);
        assert!(res_neg.pll_comp_10hz <= 0);
    }
}
