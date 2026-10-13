//! Automated CW / FM Audio Periodic Beacon Engine.
//!
//! Provides automated callsign identification, repeater announcements,
//! and emergency beacons via CW Morse or FM tone bursts on a configurable schedule.

use kernel::service::radio::{
    MORSE_BUFFER_CAPACITY, MorseEncoder, MorseTiming, RadioService, ToneNote,
};

/// Beacon transmission mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeaconMode {
    Off,
    CwMorse { wpm: u8, pitch_hz: u16 },
    FmAudioBeep { freq_hz: u16, duration_ms: u16 },
}

/// Operational state of the beacon engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeaconStatus {
    Idle,
    Transmitting,
}

/// Periodic Beacon Generator.
#[derive(Debug, Clone)]
pub struct BeaconEngine {
    pub mode: BeaconMode,
    pub interval_sec: u16,
    pub elapsed_sec: u16,
    pub message: [u8; 32],
    pub message_len: u8,
    pub status: BeaconStatus,
}

impl BeaconEngine {
    pub const fn new() -> Self {
        Self {
            mode: BeaconMode::Off,
            interval_sec: 120, // default every 2 minutes
            elapsed_sec: 0,
            message: *b"BEACON DE KERN RADIO            ",
            message_len: 20,
            status: BeaconStatus::Idle,
        }
    }

    /// Set beacon message string (up to 32 characters).
    pub fn set_message(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let len = bytes.len().min(self.message.len());
        self.message[..len].copy_from_slice(&bytes[..len]);
        self.message_len = len as u8;
    }

    /// Returns beacon message as str.
    pub fn message_str(&self) -> &str {
        core::str::from_utf8(&self.message[..self.message_len as usize]).unwrap_or("")
    }

    /// Set beacon repeat interval in seconds.
    pub fn set_interval(&mut self, interval_sec: u16) {
        self.interval_sec = interval_sec;
        self.elapsed_sec = 0;
    }

    /// Manually trigger beacon transmission on next tick.
    pub fn trigger_now(&mut self) {
        self.elapsed_sec = self.interval_sec;
    }

    /// Periodic 1-second tick.
    /// If interval expires and mode is active, transmits beacon over the radio.
    pub fn tick_1sec<R: RadioService>(&mut self, radio: &mut R) -> bool {
        if self.mode == BeaconMode::Off {
            return false;
        }

        self.elapsed_sec = self.elapsed_sec.saturating_add(1);
        if self.elapsed_sec >= self.interval_sec {
            self.elapsed_sec = 0;
            self.status = BeaconStatus::Transmitting;
            self.transmit_beacon(radio);
            self.status = BeaconStatus::Idle;
            true
        } else {
            false
        }
    }

    fn transmit_beacon<R: RadioService>(&self, radio: &mut R) {
        match self.mode {
            BeaconMode::Off => {}
            BeaconMode::CwMorse { wpm, pitch_hz } => {
                let timing = MorseTiming::from_wpm(wpm, pitch_hz);
                let encoder = MorseEncoder::new(timing);
                let msg = self.message_str();
                let mut notes = [ToneNote::new(0, 0); MORSE_BUFFER_CAPACITY];
                let count = encoder.encode_text(msg, &mut notes);

                if count > 0 {
                    radio.play_tone_sequence(&notes[..count]);
                }
            }
            BeaconMode::FmAudioBeep {
                freq_hz,
                duration_ms,
            } => {
                radio.play_tone(freq_hz, duration_ms);
            }
        }
    }

    /// Periodic 1-second tick with coordinator arbitration.
    /// Ensures beacon transmission respects active locks, BCLO, and engages RF transmitter correctly.
    pub fn tick_1sec_coord<R: RadioService>(
        &mut self,
        radio: &mut R,
        coord: &mut super::coordinator::RadioCoordinator,
    ) -> bool {
        if self.mode == BeaconMode::Off {
            return false;
        }

        self.elapsed_sec = self.elapsed_sec.saturating_add(1);
        if self.elapsed_sec >= self.interval_sec {
            let token = super::coordinator::ClientToken::new(
                super::coordinator::ClientId::BEACON,
                super::coordinator::LockTier::BACKGROUND,
            );
            if !coord.can_app_transmit(token) {
                return false;
            }

            self.elapsed_sec = 0;
            self.status = BeaconStatus::Transmitting;
            coord.start_tx(radio);
            self.transmit_beacon(radio);
            coord.stop_tx(radio);
            self.status = BeaconStatus::Idle;
            true
        } else {
            false
        }
    }
}

impl super::coordinator::RfClientParticipant for BeaconEngine {
    #[inline(always)]
    fn client_id(&self) -> super::coordinator::ClientId {
        super::coordinator::ClientId::BEACON
    }

    #[inline(always)]
    fn default_tier(&self) -> super::coordinator::LockTier {
        super::coordinator::LockTier::BACKGROUND
    }
}
