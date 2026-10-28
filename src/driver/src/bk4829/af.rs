//! Audio Frequency, Tone Generation and Volume Attenuation for BK4829.

/// Converts tone frequency (Hz) into register 0x71 frequency divider.
#[inline(always)]
pub fn tone_freq_to_reg(freq_hz: u16) -> u16 {
    kernel::service::radio::tone_gen::calc_tone_word(freq_hz)
}
