//! Vernier Sub-Hz Frequency Synthesis Algorithm.
//!
//! Inspired by `armel/uv-k1-k5v3-firmware-custom` (`dsp/vernier.c`) and `uvk5cec`.
//! Solves sub-10Hz frequency offsets using 26MHz crystal trim (BK4829 Reg 0x3B)
//! combined with PLL 10-Hz steps (Reg 0x38/0x39).

/// Result of a Vernier sub-Hz PLL and crystal trim solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VernierResult {
    /// Crystal trim register 0x3B offset.
    pub xtal_trim: u16,
    /// PLL 10-Hz compensation word for registers 0x38/0x39.
    pub pll_comp_10hz: i16,
    /// Residual frequency error in milli-Hertz (mHz).
    pub error_mhz: i16,
}

impl VernierResult {
    pub const fn new(xtal_trim: u16, pll_comp_10hz: i16, error_mhz: i16) -> Self {
        Self {
            xtal_trim,
            pll_comp_10hz,
            error_mhz,
        }
    }
}

/// Solves sub-10Hz frequency offsets using crystal trim and PLL steps.
/// `delta_f_mhz`: requested frequency offset in milli-Hertz (e.g. +6250 for FT8 tone 1).
/// `alpha_mhz_per_lsb`: crystal sensitivity in mHz per trim LSB (empirically ~750 mHz/LSB on BK4819 at 144MHz).
pub fn solve_vernier(delta_f_mhz: i32, alpha_mhz_per_lsb: u32) -> VernierResult {
    if alpha_mhz_per_lsb == 0 {
        let pll_steps = (delta_f_mhz / 10_000) as i16;
        let rem = (delta_f_mhz % 10_000) as i16;
        return VernierResult::new(0, pll_steps, rem);
    }

    // Target PLL 10 Hz coarse steps (10,000 mHz)
    let coarse_pll = (delta_f_mhz / 10_000) as i16;
    let remainder_mhz = delta_f_mhz - (coarse_pll as i32 * 10_000);

    // Calculate crystal trim LSB steps
    let trim_steps = (remainder_mhz / alpha_mhz_per_lsb as i32) as i16;
    let clamped_trim = trim_steps.clamp(-300, 300);
    let residual_error = remainder_mhz - (clamped_trim as i32 * alpha_mhz_per_lsb as i32);

    VernierResult::new(clamped_trim as u16, coarse_pll, residual_error as i16)
}
