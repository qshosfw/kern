//! Frequency Synthesis, Band Limits and PLL Tuning for BK4829.

use utils::units::{Frequency, Hertz};

/// Calculate 16-bit low and high divider words for register 0x38 and 0x39.
#[inline(always)]
pub fn calculate_pll_dividers(freq: Frequency<Hertz>) -> (u16, u16) {
    let hz = freq.raw();
    let freq_10hz = (hz / 10) as u32;
    let low = (freq_10hz & 0xFFFF) as u16;
    let high = ((freq_10hz >> 16) & 0xFFFF) as u16;
    (low, high)
}

/// Crystal trim register 0x37 calculation.
#[inline(always)]
pub fn calculate_xtal_trim(current_reg: u16, trim: i16) -> u16 {
    let p2 = (trim & 0x00FF) as u16;
    (current_reg & !0x00FF) | p2
}
