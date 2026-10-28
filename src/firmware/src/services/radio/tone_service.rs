//! Firmware Low-Level Hardware Tone Driver Interface.
//!
//! Provides granular, hardware-direct dual-tone and continuous-phase frequency-shift
//! primitives (`RfToneGenerator`) for the microkernel and `.krx` runtime packages.

use kernel::service::radio::RfToneGenerator;

/// Low-level hardware tone synthesizer service.
pub struct ToneDriverService;

impl ToneDriverService {
    /// Configure Tone 1 with raw DDS frequency word and amplitude gain.
    #[inline(always)]
    pub fn tone1_raw<R: RfToneGenerator>(radio: &mut R, freq_word: u16, gain: u8) {
        radio.set_tone1_raw(freq_word, gain, freq_word > 0);
    }

    /// Configure Tone 2 with raw DDS frequency word and amplitude gain.
    #[inline(always)]
    pub fn tone2_raw<R: RfToneGenerator>(radio: &mut R, freq_word: u16, gain: u8) {
        radio.set_tone2_raw(freq_word, gain, freq_word > 0);
    }

    /// Configure both Tone 1 and Tone 2 simultaneously using raw DDS frequency words.
    #[inline(always)]
    pub fn dual_tones_raw<R: RfToneGenerator>(
        radio: &mut R,
        tone1_word: u16,
        tone1_gain: u8,
        tone2_word: u16,
        tone2_gain: u8,
    ) {
        radio.set_dual_tones_raw(tone1_word, tone1_gain, tone2_word, tone2_gain);
    }

    /// Continuous-phase frequency shift on Tone 1 (single register write, no phase discontinuity).
    #[inline(always)]
    pub fn shift_tone1<R: RfToneGenerator>(radio: &mut R, freq_word: u16) {
        radio.set_tone1_freq_word(freq_word);
    }

    /// Continuous-phase frequency shift on Tone 2.
    #[inline(always)]
    pub fn shift_tone2<R: RfToneGenerator>(radio: &mut R, freq_word: u16) {
        radio.set_tone2_freq_word(freq_word);
    }

    /// Continuous-phase AFSK bit modulation step on Tone 1.
    #[inline(always)]
    pub fn emit_afsk_bit<R: RfToneGenerator>(
        radio: &mut R,
        bit: bool,
        mark_word: u16,
        space_word: u16,
    ) {
        radio.set_cp_afsk_bit(bit, mark_word, space_word);
    }

    /// Immediately silence all active tone synthesizers.
    #[inline(always)]
    pub fn silence<R: RfToneGenerator>(radio: &mut R) {
        radio.mute_tones();
    }
}
