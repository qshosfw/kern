//! CTCSS / DCS Subaudible Tone Synthesis and Tail Elimination.

/// Calculate CTCSS frequency register value from deci-Hz.
#[inline(always)]
pub const fn ctcss_deci_hz_to_reg(deci_hz: u16) -> u16 {
    ((deci_hz as u32 * 103) / 10) as u16
}

/// Calculate 55 Hz reverse burst frequency register value for squelch tail elimination.
pub const CSS_TAIL_55HZ_REG: u16 = ctcss_deci_hz_to_reg(550);
