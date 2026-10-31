//! Voice-Operated Transmission (VOX) Engine.
//!
//! Provides microphone audio level sensing, threshold hysteresis,
//! and hang-timer state machine for hands-free automatic PTT keying.

/// VOX configuration settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoxConfig {
    pub enabled: bool,
    /// Sensitivity level (1..=10, where 1 = least sensitive / loud shout, 10 = most sensitive / whisper).
    pub sensitivity: u8,
    /// Delay / hang time in milliseconds after speech stops before returning to RX.
    pub delay_ms: u16,
}

impl Default for VoxConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            sensitivity: 5,
            delay_ms: 1000,
        }
    }
}

/// VOX state machine status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoxStatus {
    /// Inactive / squelched (RX).
    Idle,
    /// Active voice detected (TX asserted).
    Transmitting,
    /// Voice ceased; hang timer active before dropping TX.
    HangWait { remaining_ms: u16 },
}

/// VOX detector state machine tracking audio envelope.
#[derive(Debug, Clone, Copy)]
pub struct VoxDetector {
    pub config: VoxConfig,
    pub open_threshold: u16,
    pub close_threshold: u16,
    state: VoxStatus,
    voice_detected_ms: u16,
    silence_elapsed_ms: u16,
}

impl VoxDetector {
    pub const fn new(config: VoxConfig) -> Self {
        Self {
            config,
            open_threshold: 0,
            close_threshold: 0,
            state: VoxStatus::Idle,
            voice_detected_ms: 0,
            silence_elapsed_ms: 0,
        }
    }

    /// Set calibrated open/close thresholds (Schmitt trigger).
    pub fn set_thresholds(&mut self, open: u16, close: u16) {
        self.open_threshold = open;
        self.close_threshold = close;
    }

    /// Reset internal state.
    pub fn reset(&mut self) {
        self.state = VoxStatus::Idle;
        self.voice_detected_ms = 0;
        self.silence_elapsed_ms = 0;
    }

    /// Convert 1..10 sensitivity to audio envelope threshold.
    /// Higher sensitivity = lower threshold.
    pub const fn threshold_amplitude(&self) -> u16 {
        match self.config.sensitivity {
            1 => 850,
            2 => 750,
            3 => 650,
            4 => 550,
            5 => 450,
            6 => 350,
            7 => 260,
            8 => 180,
            9 => 120,
            10 => 70,
            _ => 500,
        }
    }

    /// Process a periodic audio level sample (e.g. called every 20ms with peak mic amplitude).
    /// Returns true if PTT should be keyed (TX active), false if RX.
    pub fn process_sample(&mut self, amplitude: u16, tick_step_ms: u16) -> bool {
        if !self.config.enabled {
            self.state = VoxStatus::Idle;
            return false;
        }

        let open_th = if self.open_threshold != 0 {
            self.open_threshold
        } else {
            self.threshold_amplitude()
        };
        let close_th = if self.close_threshold != 0 {
            self.close_threshold
        } else {
            open_th.saturating_sub(50)
        };

        match self.state {
            VoxStatus::Idle => {
                if amplitude >= open_th {
                    self.voice_detected_ms = self.voice_detected_ms.saturating_add(tick_step_ms);
                    // Require at least 40ms of continuous voice to prevent transient clicks triggering TX
                    if self.voice_detected_ms >= 40 {
                        self.state = VoxStatus::Transmitting;
                        self.silence_elapsed_ms = 0;
                        return true;
                    }
                } else {
                    self.voice_detected_ms = 0;
                }
                false
            }
            VoxStatus::Transmitting => {
                if amplitude >= close_th {
                    self.silence_elapsed_ms = 0;
                    true
                } else {
                    self.silence_elapsed_ms = self.silence_elapsed_ms.saturating_add(tick_step_ms);
                    if self.silence_elapsed_ms >= self.config.delay_ms {
                        self.state = VoxStatus::Idle;
                        self.voice_detected_ms = 0;
                        false
                    } else {
                        let remaining =
                            self.config.delay_ms.saturating_sub(self.silence_elapsed_ms);
                        self.state = VoxStatus::HangWait {
                            remaining_ms: remaining,
                        };
                        true
                    }
                }
            }
            VoxStatus::HangWait { .. } => {
                if amplitude >= close_th {
                    self.state = VoxStatus::Transmitting;
                    self.silence_elapsed_ms = 0;
                    true
                } else {
                    self.silence_elapsed_ms = self.silence_elapsed_ms.saturating_add(tick_step_ms);
                    if self.silence_elapsed_ms >= self.config.delay_ms {
                        self.state = VoxStatus::Idle;
                        self.voice_detected_ms = 0;
                        false
                    } else {
                        let remaining =
                            self.config.delay_ms.saturating_sub(self.silence_elapsed_ms);
                        self.state = VoxStatus::HangWait {
                            remaining_ms: remaining,
                        };
                        true
                    }
                }
            }
        }
    }

    /// Current status of the VOX detector.
    pub fn status(&self) -> VoxStatus {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vox_schmitt_trigger_hysteresis() {
        let mut vox = VoxDetector::new(VoxConfig {
            enabled: true,
            sensitivity: 5,
            delay_ms: 100,
        });
        vox.set_thresholds(600, 400);

        // Amplitude 500 is between close (400) and open (600) -> should not trigger open
        assert!(!vox.process_sample(500, 20));
        assert!(!vox.process_sample(500, 20));
        assert_eq!(vox.status(), VoxStatus::Idle);

        // Exceed open threshold for 40ms -> should transition to Transmitting
        assert!(!vox.process_sample(650, 20));
        assert!(vox.process_sample(650, 20));
        assert_eq!(vox.status(), VoxStatus::Transmitting);

        // Drop to 500 (above close threshold 400) -> stays Transmitting!
        assert!(vox.process_sample(500, 20));
        assert_eq!(vox.status(), VoxStatus::Transmitting);

        // Drop below close threshold (350) -> transitions to HangWait
        assert!(vox.process_sample(350, 20));
        assert!(matches!(vox.status(), VoxStatus::HangWait { .. }));

        // Hold below 400 for 100ms total -> returns to Idle
        vox.process_sample(350, 40);
        vox.process_sample(350, 50);
        assert_eq!(vox.status(), VoxStatus::Idle);
    }
}
