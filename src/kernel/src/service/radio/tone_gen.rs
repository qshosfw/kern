//! Hardware Dual Tone Generator and Audio Modulation Subsystem.
//!
//! Provides direct, hardware-level control over independent tone synthesizers
//! (Tone1 and Tone2 on BK4819 / BK4829, registers 0x70, 0x71, 0x72) so that
//! userland `.krx` applications and services can implement signaling protocols
//! without embedding protocol tables in the microkernel.

/// Configuration profile for a single hardware tone synthesizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToneConfig {
    /// Audio frequency in Hertz (0 disables frequency output).
    pub frequency_hz: u16,
    /// Amplitude gain level (0..=127).
    pub gain: u8,
    /// Tone generator active state.
    pub enabled: bool,
}

impl ToneConfig {
    pub const fn new(frequency_hz: u16, gain: u8) -> Self {
        Self {
            frequency_hz,
            gain,
            enabled: true,
        }
    }

    pub const DISABLED: Self = Self {
        frequency_hz: 0,
        gain: 0,
        enabled: false,
    };
}

/// Calculate the 16-bit BK4819 / BK4829 DDS frequency control word for a given frequency in Hertz.
///
/// Formula: `round((freq_hz * 1032444) / 100000)` with 26 MHz master crystal.
#[inline(always)]
pub const fn calc_tone_word(freq_hz: u16) -> u16 {
    // 32-bit fixed-point reciprocal: 42525 / 2^17 = 0.3244400024 (exact to 9 decimal places)
    let frac = (freq_hz as u32 * 42_525 + 65_536) >> 17;
    (freq_hz as u32 * 10 + frac) as u16
}

/// Convert a 16-bit DDS frequency control word back into frequency in Hertz.
#[inline(always)]
pub const fn tone_word_to_hz(word: u16) -> u16 {
    // 32-bit fixed-point reciprocal: 50781 / 2^19 = 1 / 10.32444
    ((word as u32 * 50_781 + 262_144) >> 19) as u16
}

/// Granular, low-level hardware contract for independent dual tone generation.
pub trait RfToneGenerator {
    /// Configure Tone 1 directly via raw 16-bit DDS frequency word and amplitude gain (0..=127).
    fn set_tone1_raw(&mut self, freq_word: u16, gain: u8, enabled: bool);

    /// Configure Tone 2 directly via raw 16-bit DDS frequency word and amplitude gain (0..=127).
    fn set_tone2_raw(&mut self, freq_word: u16, gain: u8, enabled: bool);

    /// Atomic, continuous-phase single-register frequency update on Tone 1 (Reg 0x71).
    ///
    /// Changes the DDS frequency step without resetting the oscillator phase or altering gain.
    fn set_tone1_freq_word(&mut self, freq_word: u16);

    /// Atomic, continuous-phase single-register frequency update on Tone 2 (Reg 0x72).
    ///
    /// Changes the DDS frequency step without resetting the oscillator phase or altering gain.
    fn set_tone2_freq_word(&mut self, freq_word: u16);

    /// Adjust Tone 1 amplitude gain (0..=127) without altering frequency.
    fn set_tone1_gain(&mut self, gain: u8);

    /// Adjust Tone 2 amplitude gain (0..=127) without altering frequency.
    fn set_tone2_gain(&mut self, gain: u8);

    /// Assert or deassert Tone 1 output.
    fn enable_tone1(&mut self, enable: bool);

    /// Assert or deassert Tone 2 output.
    fn enable_tone2(&mut self, enable: bool);

    /// Immediately silence and mute all tone synthesizers (Reg 0x70 = 0).
    fn mute_tones(&mut self);

    // --- Higher-level convenience defaults below ---

    /// Set Tone 1 frequency in Hertz and amplitude gain (0..=127).
    fn set_tone1(&mut self, freq_hz: u16, gain: u8) {
        self.set_tone1_raw(calc_tone_word(freq_hz), gain, freq_hz > 0);
    }

    /// Set Tone 2 frequency in Hertz and amplitude gain (0..=127).
    fn set_tone2(&mut self, freq_hz: u16, gain: u8) {
        self.set_tone2_raw(calc_tone_word(freq_hz), gain, freq_hz > 0);
    }

    /// Set Tone 1 frequency in Hertz.
    fn set_tone1_hz(&mut self, freq_hz: u16) {
        self.set_tone1_freq_word(calc_tone_word(freq_hz));
    }

    /// Set Tone 2 frequency in Hertz.
    fn set_tone2_hz(&mut self, freq_hz: u16) {
        self.set_tone2_freq_word(calc_tone_word(freq_hz));
    }

    /// Simultaneously configure both tone synthesizers using raw DDS frequency words.
    fn set_dual_tones_raw(
        &mut self,
        tone1_word: u16,
        tone1_gain: u8,
        tone2_word: u16,
        tone2_gain: u8,
    ) {
        self.set_tone1_raw(tone1_word, tone1_gain, tone1_word > 0);
        self.set_tone2_raw(tone2_word, tone2_gain, tone2_word > 0);
    }

    /// Simultaneously configure both tone synthesizers using frequencies in Hertz.
    fn set_dual_tones(&mut self, tone1_hz: u16, tone1_gain: u8, tone2_hz: u16, tone2_gain: u8) {
        self.set_dual_tones_raw(
            calc_tone_word(tone1_hz),
            tone1_gain,
            calc_tone_word(tone2_hz),
            tone2_gain,
        );
    }

    /// Instantaneous continuous-phase frequency shift step on Tone 1.
    #[inline(always)]
    fn set_cp_afsk_bit(&mut self, bit: bool, mark_word: u16, space_word: u16) {
        self.set_tone1_freq_word(if bit { mark_word } else { space_word });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tone_word_calculation_and_roundtrip() {
        // Standard musical tones used in key and navigation beeps
        let test_freqs = [440, 650, 880, 1046, 1175, 1200, 1318];
        for &freq in &test_freqs {
            let word = calc_tone_word(freq);
            assert!(word > 0, "Tone word for {} Hz must be non-zero", freq);
            let roundtrip = tone_word_to_hz(word);
            let diff = (roundtrip as i32 - freq as i32).abs();
            assert!(
                diff <= 1,
                "Tone {} Hz roundtrip diff {} too large",
                freq,
                diff
            );
        }
    }
}
