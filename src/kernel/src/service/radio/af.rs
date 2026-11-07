//! Audio Frequency (AF) Response Shaping, Microphone Preamp and Tone Generation.

/// A single musical tone note or timed silence period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToneNote {
    pub freq_hz: u16,
    pub duration_ms: u16,
}

impl ToneNote {
    pub const fn new(freq_hz: u16, duration_ms: u16) -> Self {
        Self {
            freq_hz,
            duration_ms,
        }
    }
}

/// 4-Band Audio Frequency (AF) equalization and emphasis response.
/// Directly mirrors the BK4819 DSP filtering model from Fagci-K1 and DeltaFW.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AfResponse {
    /// RX 300 Hz response (-4..+4 dB, 0 = 0 dB).
    pub rx_300: i8,
    /// RX 3 kHz deemphasis response (-4..+4 dB).
    pub rx_3k: i8,
    /// TX 300 Hz high-pass response (-4..+4 dB).
    pub tx_300: i8,
    /// TX 3 kHz preemphasis response (-4..+4 dB).
    pub tx_3k: i8,
}

/// Declarative interface for RF baseband audio filtering, mic gain, and tone synthesis.
pub trait RfAudioPath {
    fn set_af_response(&mut self, response: &AfResponse);
    fn set_mic_gain(&mut self, gain_db: u8);
    fn set_rf_volume(&mut self, level: u8);
    fn set_tone_gain(&mut self, gain: u8);
    fn play_tone(&mut self, freq_hz: u16, duration_ms: u16);
    fn play_tone_sequence(&mut self, notes: &[ToneNote]);
    fn play_tone_sequence_with_gain(&mut self, notes: &[ToneNote], gain: u8);
    /// Mute or unmute the baseband audio output path (speaker/earpiece).
    fn mute_audio(&mut self, mute: bool) {
        self.set_rf_volume(if mute { 0 } else { 5 });
    }
    /// Mute or unmute the microphone audio path.
    fn mute_mic(&mut self, _mute: bool) {}

    /// Read instantaneous voice/modulator amplitude from silicon (`Reg 0x64`).
    fn get_voice_amplitude(&mut self) -> u16 {
        0
    }

    /// Read AF TX/RX input amplitude in dB from silicon (`Reg 0x6F` bits `[6:0]`).
    fn get_af_amplitude_db(&mut self) -> u8 {
        0
    }
}
