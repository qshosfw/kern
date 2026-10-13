//! Audio Frequency, Tone Generation and Volume Attenuation for BK4829.

/// Converts tone frequency (Hz) into register 0x71 frequency divider.
#[inline(always)]
pub fn tone_freq_to_reg(freq_hz: u16) -> u16 {
    (((freq_hz as u32 * 1032444) + 50000) / 100000) as u16
}
