//! RF Carrier Squelch, Signal Quality Telemetry and Hysteresis Thresholds.

/// Type of squelch evaluation gating the audio path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SquelchType {
    /// Pure RF carrier energy and noise threshold.
    Carrier,
    /// Continuous Tone-Coded Squelch (CTCSS).
    Ctcss,
    /// Digitally Coded Squelch (DCS / CDCSS).
    Cdcss,
    /// Optional selective calling tone (e.g. 1750 Hz repeater burst or DTMF).
    SelectiveTone,
}

/// Detailed hysteresis threshold profile for opening and closing RF squelch.
/// Directly mirrors the 6-parameter DSP model from DeltaFW / F4HWN.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SquelchThresholds {
    pub open_rssi: u16,
    pub close_rssi: u16,
    pub open_noise: u16,
    pub close_noise: u16,
    pub open_glitch: u16,
    pub close_glitch: u16,
}

impl Default for SquelchThresholds {
    fn default() -> Self {
        Self {
            open_rssi: 0x0040,
            close_rssi: 0x0030,
            open_noise: 0x0080,
            close_noise: 0x0090,
            open_glitch: 0x0020,
            close_glitch: 0x0028,
        }
    }
}

/// Instantaneous RF signal telemetry bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignalMetrics {
    pub rssi_dbm: i16,
    pub snr: u8,
    pub noise: u8,
    pub glitch: u8,
    pub carrier_open: bool,
}

/// Declarative interface for RF squelch supervision, hysteresis, and telemetry metrics.
pub trait RfSquelch {
    /// Set coarse squelch level (0 = open/monitor, 1..9 = progressive attenuation).
    fn set_squelch_level(&mut self, level: u8);
    /// Set precise 6-parameter DSP noise, glitch, and RSSI threshold hysteresis.
    fn set_squelch_thresholds(&mut self, thresholds: &SquelchThresholds);
    /// Read full telemetry packet in a single atomic snapshot.
    fn get_signal_metrics(&mut self) -> SignalMetrics;
    /// Check whether the RF receiver carrier gate is currently open.
    fn is_carrier_open(&mut self) -> bool;
    /// Read current RSSI calibrated in dBm.
    fn get_rssi_dbm(&mut self) -> i16;
    /// Read DSP Signal-to-Noise Ratio (SNR) metric.
    fn get_snr(&mut self) -> u8;
    /// Read DSP Glitch detection metric.
    fn get_glitch(&mut self) -> u8;
    /// Read DSP Noise level metric.
    fn get_noise(&mut self) -> u8;
}
