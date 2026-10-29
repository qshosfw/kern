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

impl SignalMetrics {
    /// Computes voice probability (0..=100) using integer arithmetic (Cortex-M0+ zero-division).
    /// Low demod noise + low glitch + positive SNR indicates voice audio over white noise or interference.
    pub fn voice_probability(&self) -> u8 {
        let mut score: i16 = 0;

        // Noise indicator (strongest voice predictor: low noise = clean audio)
        if self.noise < 30 {
            score += 35;
        } else if self.noise < 60 {
            score += 25;
        } else if self.noise < 100 {
            score += 10;
        } else if self.noise > 200 {
            score -= 15;
        }

        // Glitch indicator (interference rejection: low glitch = stable signal)
        if self.glitch < 15 {
            score += 20;
        } else if self.glitch < 40 {
            score += 10;
        } else if self.glitch > 150 {
            score -= 20;
        } else if self.glitch > 80 {
            score -= 5;
        }

        // SNR score
        if self.snr > 25 {
            score += 25;
        } else if self.snr > 15 {
            score += 15;
        } else if self.snr > 8 {
            score += 5;
        } else {
            score -= 10;
        }

        // Cross-correlation: voice is low noise and low-moderate glitch
        if self.noise < 60 && self.glitch < 40 && self.glitch > 5 {
            score += 10;
        }
        if self.noise > 150 && self.glitch > 100 {
            score -= 10;
        }

        score.clamp(0, 100) as u8
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_metrics_voice_probability() {
        // Clean voice signal: low noise, moderate glitch, high SNR
        let clean_voice = SignalMetrics {
            rssi_dbm: -75,
            snr: 30,
            noise: 20,
            glitch: 12,
            carrier_open: true,
        };
        assert!(clean_voice.voice_probability() >= 80);

        // White noise / static: very high noise, high glitch, low SNR
        let static_noise = SignalMetrics {
            rssi_dbm: -115,
            snr: 2,
            noise: 210,
            glitch: 180,
            carrier_open: false,
        };
        assert!(static_noise.voice_probability() <= 10);
    }
}
