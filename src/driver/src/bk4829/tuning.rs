//! Frequency Synthesis, Band Limits and PLL Tuning for BK4829.

use utils::units::{Frequency, Hertz};

/// Nominal 26 MHz crystal reference low-word baseline for `REG_3B` (`0x5880 = 22656`).
pub const XTAL_BASELINE_26MHZ: i32 = 22656;

/// Calculate 16-bit low and high divider words for register 0x38 and 0x39.
#[inline(always)]
pub fn calculate_pll_dividers(freq: Frequency<Hertz>) -> (u16, u16) {
    let hz = freq.raw();
    let (freq_10hz, _) = utils::math::div10_u32(hz);
    let low = (freq_10hz & 0xFFFF) as u16;
    let high = ((freq_10hz >> 16) & 0xFFFF) as u16;
    (low, high)
}

/// Crystal frequency low register (`REG_3B`) calculation from signed trim offset.
#[inline(always)]
pub fn calculate_xtal_freq_low(trim: i16) -> u16 {
    (XTAL_BASELINE_26MHZ + trim as i32).clamp(0, 65535) as u16
}
