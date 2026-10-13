//! Automatic Gain Control (AGC) and Front-End Attenuation.

/// Listening mode presets for AGC tuning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgcListeningMode {
    Fm,
    AmListening,
    Ssb,
}

/// Dynamic AGC attack and decay speed setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgcSpeed {
    Slow,
    Medium,
    Fast,
}

/// Standard AGC tables from GoguFW and Fagci-K1.
pub const AGC_DEFAULT_TABLE: [u16; 5] = [0x0000, 0x1020, 0x2040, 0x3060, 0x4080];

/// Declarative interface for RF front-end AGC thresholds, manual attenuation, and speed.
pub trait RfAgc {
    fn set_agc_table(&mut self, table: &[u16; 5]);
    fn set_agc_mode(&mut self, mode: AgcListeningMode, disable: bool);
    fn set_agc_speed(&mut self, speed: AgcSpeed);
}
