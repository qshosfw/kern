//! Hardware Dual Tone Generator and Audio Modulation Subsystem.
//!
//! Provides direct, hardware-level control over independent tone synthesizers
//! (Tone1 and Tone2 on BK4819 / BK4829, registers 0x70, 0x71, 0x72).
//!
//! Designed for arbitrary tone signaling beyond subaudible squelch:
//! - Audio Frequency Shift Keying (AFSK 1200 / 2200 Hz Bell 202, APRS)
//! - European 1750 Hz Repeater Access Bursts
//! - Dual-Tone Multi-Frequency (DTMF) dialing
//! - Selective Calling (SelCall CCIR, ZVEI, EEA tone bursts)
//! - CW / Morse code sidetone and RF tone modulation
//! - Continuous or stepped frequency sweeps for filter and audio probing

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
    (((freq_hz as u32 * 1032444) + 50000) / 100000) as u16
}

/// Convert a 16-bit DDS frequency control word back into frequency in Hertz.
#[inline(always)]
pub const fn tone_word_to_hz(word: u16) -> u16 {
    (((word as u32 * 100000) + 516222) / 1032444) as u16
}

/// Standardized well-known signaling and modem audio tone frequencies and DDS words.
pub struct CommonTones;

impl CommonTones {
    /// European repeater access tone burst (1750 Hz).
    pub const REPEATER_1750_HZ: u16 = 1750;
    pub const REPEATER_1750_HZ_WORD: u16 = calc_tone_word(1750);

    /// Bell 202 AFSK Mark tone (1200 Hz).
    pub const BELL202_MARK_1200_HZ: u16 = 1200;
    pub const BELL202_MARK_1200_HZ_WORD: u16 = calc_tone_word(1200);

    /// Bell 202 AFSK Space tone (2200 Hz).
    pub const BELL202_SPACE_2200_HZ: u16 = 2200;
    pub const BELL202_SPACE_2200_HZ_WORD: u16 = calc_tone_word(2200);

    /// Fast FSK audio tone 1 (1200 Hz).
    pub const FFSK_TONE1_1200_HZ: u16 = 1200;
    pub const FFSK_TONE1_1200_HZ_WORD: u16 = calc_tone_word(1200);

    /// Fast FSK audio tone 2 (1800 Hz).
    pub const FFSK_TONE2_1800_HZ: u16 = 1800;
    pub const FFSK_TONE2_1800_HZ_WORD: u16 = calc_tone_word(1800);

    /// Fast FSK audio tone 2 alternative (2400 Hz).
    pub const FFSK_TONE2_2400_HZ: u16 = 2400;
    pub const FFSK_TONE2_2400_HZ_WORD: u16 = calc_tone_word(2400);

    /// NOAA SAME alert subcarrier tone (1050 Hz).
    pub const NOAA_SAME_1050_HZ: u16 = 1050;
    pub const NOAA_SAME_1050_HZ_WORD: u16 = calc_tone_word(1050);

    /// Standard CW sidetone reference frequency (700 Hz).
    pub const CW_SIDETONE_700_HZ: u16 = 700;
    pub const CW_SIDETONE_700_HZ_WORD: u16 = calc_tone_word(700);
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

    /// Simultaneously configure both tone synthesizers for dual-tone signaling (e.g. DTMF).
    fn set_dual_tones(&mut self, tone1_hz: u16, tone1_gain: u8, tone2_hz: u16, tone2_gain: u8) {
        self.set_dual_tones_raw(
            calc_tone_word(tone1_hz),
            tone1_gain,
            calc_tone_word(tone2_hz),
            tone2_gain,
        );
    }

    /// Instantaneous continuous-phase FSK (CPFSK / AFSK) frequency shift step on Tone 1.
    #[inline(always)]
    fn set_cp_afsk_bit(&mut self, bit: bool, mark_word: u16, space_word: u16) {
        self.set_tone1_freq_word(if bit { mark_word } else { space_word });
    }

    /// Fast single-tone burst for repeater opening (1750 Hz).
    fn burst_1750hz(&mut self, gain: u8) {
        self.set_tone1_raw(CommonTones::REPEATER_1750_HZ_WORD, gain, true);
    }

    /// Fast Bell 202 AFSK bit modulation step on Tone 1 (true = 1200 Hz Mark, false = 2200 Hz Space).
    #[inline(always)]
    fn set_afsk_bell202_bit(&mut self, bit: bool, gain: u8) {
        let word = if bit {
            CommonTones::BELL202_MARK_1200_HZ_WORD
        } else {
            CommonTones::BELL202_SPACE_2200_HZ_WORD
        };
        self.set_tone1_raw(word, gain, true);
    }
}
