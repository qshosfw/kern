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

/// Dynamic Noise-Floor Estimator (Asymmetric Leaky LMS / EMA) for Adaptive Squelch Tracking.
///
/// Tracks ambient RF noise (`Reg 0x65`), glitch density (`Reg 0x63`), and baseline RSSI (`Reg 0x67`)
/// while squelch is closed (`!carrier_open`). Fast downward attack and slow upward adaptation prevent
/// transient voice packets from corrupting the floor estimate while compensating for thermal drift
/// and local RF EMI to prevent squelch chop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdaptiveSquelchTracker {
    /// Ambient noise floor in Q4 fixed-point (0..=2032, representing 0..=127).
    pub noise_floor_q4: u16,
    /// Ambient glitch floor in Q4 fixed-point (0..=4080, representing 0..=255).
    pub glitch_floor_q4: u16,
    /// Ambient RSSI floor in dBm (e.g. -122 dBm).
    pub rssi_floor_dbm: i16,
    /// Number of idle samples accumulated.
    pub sample_count: u16,
}

impl AdaptiveSquelchTracker {
    pub const fn new() -> Self {
        Self {
            noise_floor_q4: 60 << 4,
            glitch_floor_q4: 96 << 4,
            rssi_floor_dbm: -120,
            sample_count: 0,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Feed a new signal telemetry sample into the estimator.
    /// Only updates the ambient noise floor when the channel is idle (`!metrics.carrier_open`).
    pub fn update(&mut self, metrics: &SignalMetrics) {
        if metrics.carrier_open {
            return;
        }

        let n_q4 = (metrics.noise.min(127) as u16) << 4;
        let g_q4 = (metrics.glitch as u16) << 4;

        if self.sample_count == 0 {
            self.noise_floor_q4 = n_q4;
            self.glitch_floor_q4 = g_q4;
            self.rssi_floor_dbm = metrics.rssi_dbm.clamp(-140, -70);
            self.sample_count = 1;
            return;
        }

        // Asymmetric Leaky LMS step:
        // When noise/glitch drops (quieter environment), adapt in 1/8 steps;
        // when noise/glitch rises, adapt slowly in 1/16 steps.
        if n_q4 < self.noise_floor_q4 {
            self.noise_floor_q4 -= (self.noise_floor_q4 - n_q4) >> 3;
        } else {
            self.noise_floor_q4 += (n_q4 - self.noise_floor_q4) >> 4;
        }

        if g_q4 < self.glitch_floor_q4 {
            self.glitch_floor_q4 -= (self.glitch_floor_q4 - g_q4) >> 3;
        } else {
            self.glitch_floor_q4 += (g_q4 - self.glitch_floor_q4) >> 4;
        }

        let rssi_clamped = metrics.rssi_dbm.clamp(-140, -70);
        if rssi_clamped < self.rssi_floor_dbm {
            self.rssi_floor_dbm =
                ((self.rssi_floor_dbm as i32 * 3 + rssi_clamped as i32) / 4) as i16;
        } else {
            self.rssi_floor_dbm =
                ((self.rssi_floor_dbm as i32 * 15 + rssi_clamped as i32) / 16) as i16;
        }

        self.sample_count = self.sample_count.saturating_add(1);
    }

    /// Compute dynamic `SquelchThresholds` adapted to the live RF/thermal noise floor.
    pub fn adapt_thresholds(&self, base: &SquelchThresholds) -> SquelchThresholds {
        if self.sample_count < 4 || base.open_rssi == 0 {
            return *base;
        }

        let est_noise = (self.noise_floor_q4 >> 4) as i16;
        let noise_delta = ((est_noise - 60) / 4).clamp(-6, 8);

        let rssi_emi_boost = if self.rssi_floor_dbm > -115 {
            (((self.rssi_floor_dbm + 115) * 2).clamp(0, 12)) as u16
        } else {
            0
        };

        let open_noise = (base.open_noise as i16 + noise_delta).clamp(8, 120) as u16;
        let close_noise =
            (base.close_noise as i16 + noise_delta).clamp(open_noise as i16 + 2, 126) as u16;

        let open_glitch = (base.open_glitch as i16 + noise_delta * 2).clamp(6, 240) as u16;
        let close_glitch =
            (base.close_glitch as i16 + noise_delta * 2).clamp(open_glitch as i16 + 4, 254) as u16;

        let open_rssi = (base.open_rssi + rssi_emi_boost).min(240);
        let close_rssi = (base.close_rssi + rssi_emi_boost).min(open_rssi.saturating_sub(2));

        SquelchThresholds {
            open_rssi,
            close_rssi,
            open_noise,
            close_noise,
            open_glitch,
            close_glitch,
        }
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

    #[test]
    fn test_adaptive_squelch_tracker_emi_and_thermal_drift() {
        let mut tracker = AdaptiveSquelchTracker::new();
        let base = SquelchThresholds {
            open_rssi: 102,
            close_rssi: 98,
            open_noise: 44,
            close_noise: 48,
            open_glitch: 60,
            close_glitch: 64,
        };

        for _ in 0..16 {
            tracker.update(&SignalMetrics {
                rssi_dbm: -109,
                snr: 0,
                noise: 76,
                glitch: 120,
                carrier_open: false,
            });
        }

        let adapted = tracker.adapt_thresholds(&base);
        assert!(adapted.open_rssi > base.open_rssi);
        assert!(adapted.open_noise > base.open_noise);
        assert!(adapted.close_noise > adapted.open_noise);
    }
}
