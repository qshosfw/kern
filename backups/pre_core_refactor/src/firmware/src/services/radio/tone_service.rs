//! Firmware Low-Level Digital Tone & Modulation Interface.
//!
//! Provides granular, hardware-direct digital tone and frequency-shift synthesis:
//! - Direct DDS frequency word calculations (compiled at const-time)
//! - Atomic continuous-phase frequency shift keying (CPFSK / AFSK Bell 202, APRS)
//! - Pre-computed Dual-Tone Multi-Frequency (DTMF) word pairs
//! - Non-blocking tone generator controls without spinning CPU delays

use kernel::service::radio::{CommonTones, RfToneGenerator, calc_tone_word};

/// Standard DTMF tone pair frequencies and compile-time DDS control words.
/// Low group: 697, 770, 852, 941 Hz. High group: 1209, 1336, 1477, 1633 Hz.
pub struct DtmfToneWords;

impl DtmfToneWords {
    pub const ROW_FREQS_HZ: [u16; 4] = [697, 770, 852, 941];
    pub const COL_FREQS_HZ: [u16; 4] = [1209, 1336, 1477, 1633];

    pub const ROW_WORDS: [u16; 4] = [
        calc_tone_word(697),
        calc_tone_word(770),
        calc_tone_word(852),
        calc_tone_word(941),
    ];

    pub const COL_WORDS: [u16; 4] = [
        calc_tone_word(1209),
        calc_tone_word(1336),
        calc_tone_word(1477),
        calc_tone_word(1633),
    ];

    /// Resolve an ASCII character ('0'..='9', 'A'..='D', '*', '#') to raw DDS (row_word, col_word).
    #[inline]
    pub const fn get_dtmf_words(ch: char) -> Option<(u16, u16)> {
        let (row, col) = match ch {
            '1' => (0, 0),
            '2' => (0, 1),
            '3' => (0, 2),
            'A' | 'a' => (0, 3),
            '4' => (1, 0),
            '5' => (1, 1),
            '6' => (1, 2),
            'B' | 'b' => (1, 3),
            '7' => (2, 0),
            '8' => (2, 1),
            '9' => (2, 2),
            'C' | 'c' => (2, 3),
            '*' => (3, 0),
            '0' => (3, 1),
            '#' => (3, 2),
            'D' | 'd' => (3, 3),
            _ => return None,
        };
        Some((Self::ROW_WORDS[row], Self::COL_WORDS[col]))
    }

    /// Resolve an ASCII character to frequency in Hertz (low_hz, high_hz).
    #[inline]
    pub const fn get_tone_pair(ch: char) -> Option<(u16, u16)> {
        let (row, col) = match ch {
            '1' => (0, 0),
            '2' => (0, 1),
            '3' => (0, 2),
            'A' | 'a' => (0, 3),
            '4' => (1, 0),
            '5' => (1, 1),
            '6' => (1, 2),
            'B' | 'b' => (1, 3),
            '7' => (2, 0),
            '8' => (2, 1),
            '9' => (2, 2),
            'C' | 'c' => (2, 3),
            '*' => (3, 0),
            '0' => (3, 1),
            '#' => (3, 2),
            'D' | 'd' => (3, 3),
            _ => return None,
        };
        Some((Self::ROW_FREQS_HZ[row], Self::COL_FREQS_HZ[col]))
    }
}

/// Continuous-Phase Frequency Shift Keying (CPFSK / AFSK) modulator state.
///
/// Enables seamless symbol generation (e.g. 1200 baud Bell 202 / APRS) where
/// symbol transitions occur with phase continuity via single-SPI-write register updates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpAfskModulator {
    pub mark_word: u16,
    pub space_word: u16,
    pub gain: u8,
}

impl CpAfskModulator {
    pub const BELL202: Self = Self {
        mark_word: CommonTones::BELL202_MARK_1200_HZ_WORD,
        space_word: CommonTones::BELL202_SPACE_2200_HZ_WORD,
        gain: 96,
    };

    pub const FFSK_1200_1800: Self = Self {
        mark_word: CommonTones::FFSK_TONE1_1200_HZ_WORD,
        space_word: CommonTones::FFSK_TONE2_1800_HZ_WORD,
        gain: 96,
    };

    pub const fn new(mark_hz: u16, space_hz: u16, gain: u8) -> Self {
        Self {
            mark_word: calc_tone_word(mark_hz),
            space_word: calc_tone_word(space_hz),
            gain,
        }
    }

    /// Prime the hardware tone synthesizer for AFSK modulation.
    pub fn prime<R: RfToneGenerator>(&self, radio: &mut R) {
        radio.set_tone1_raw(self.mark_word, self.gain, true);
    }

    /// Transmit a single symbol bit with continuous phase (atomic single-register write).
    #[inline(always)]
    pub fn emit_bit<R: RfToneGenerator>(&self, radio: &mut R, bit: bool) {
        let word = if bit { self.mark_word } else { self.space_word };
        radio.set_tone1_freq_word(word);
    }

    /// Mute the modulator output when transmission concludes.
    pub fn mute<R: RfToneGenerator>(&self, radio: &mut R) {
        radio.mute_tones();
    }
}

/// Intuitive, low-level hardware tone driver service.
pub struct ToneDriverService;

impl ToneDriverService {
    /// Configure Tone 1 with raw DDS frequency word and amplitude gain.
    #[inline(always)]
    pub fn tone1_raw<R: RfToneGenerator>(radio: &mut R, freq_word: u16, gain: u8) {
        radio.set_tone1_raw(freq_word, gain, true);
    }

    /// Configure Tone 2 with raw DDS frequency word and amplitude gain.
    #[inline(always)]
    pub fn tone2_raw<R: RfToneGenerator>(radio: &mut R, freq_word: u16, gain: u8) {
        radio.set_tone2_raw(freq_word, gain, true);
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

    /// Configure hardware synthesizers for a DTMF digit using precalculated DDS words.
    pub fn play_dtmf_char<R: RfToneGenerator>(radio: &mut R, ch: char, gain: u8) -> bool {
        if let Some((row_word, col_word)) = DtmfToneWords::get_dtmf_words(ch) {
            radio.set_dual_tones_raw(row_word, gain, col_word, gain);
            true
        } else {
            radio.mute_tones();
            false
        }
    }

    /// Continuous-phase AFSK bit modulation step.
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
