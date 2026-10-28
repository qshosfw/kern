//! Layer 2 Audio Driver: Audio Power Amplifier (LN4898), Digital Gain Tables,
//! and Hardware Tone Primitives.
//!
//! # Hardware Architecture Reality Check
//! On the Quansheng UV-K1 / K5:
//! 1. Master acoustic volume is physically controlled by the 10kΩ analog potentiometer (VOLSWA)
//!    acting as a passive voltage divider directly at the input of the LN4898 audio power amplifier.
//!    The MCU (PY32F071) has NO ADC channel connected to the potentiometer wiper.
//! 2. Firmware controls:
//!    - AUDIO_PA (PA8): Direct power gating of the LN4898 audio amplifier (<0.1µA shutdown current).
//!    - BK4829 REG_48: Digital AF Rx Gain (bits 9:4) and AF DAC Gain (bits 3:0). This sets the
//!      preamp/DAC signal amplitude BEFORE it enters the physical potentiometer knob.
//!    - BK4829 REG_70: Tone generator synthesis gain (bits 6:0, 0..=127).
//!    - BK1080 REG_05: Digital volume attenuation for the FM broadcast receiver.

use bsp::pins::AUDIO_PA;

/// 11-step digital AF Preamp / DAC Gain curve for BK4829 REG_48.
/// Maps digital level 0..=10 to (rx_gain2: 0..=63, dac_gain: 0..=15).
/// Note: This modulates the preamp signal entering the physical volume knob.
pub const VOLUME_TABLE: [(u8, u8); 11] = [
    (0, 0),   // Level 0: Muted (0dB)
    (2, 1),   // Level 1: Very Quiet (-30dB)
    (6, 2),   // Level 2: Quiet (-24dB)
    (12, 3),  // Level 3: Soft (-18dB)
    (20, 5),  // Level 4: Moderate (-14dB)
    (28, 7),  // Level 5: Medium (-10dB)
    (36, 9),  // Level 6: Medium-High (-6dB)
    (44, 11), // Level 7: High (-3dB)
    (52, 13), // Level 8: Very High (-1dB)
    (58, 14), // Level 9: Max Clean (0dB)
    (63, 15), // Level 10: Boosted (+3dB)
];

/// A single musical tone note or silence delay.
pub use kernel::service::ToneNote;

// ── Audio Amplifier Hardware Control ────────────────────────────────────────

static mut HANGOVER_MS: u16 = 0;

/// Hardware driver for the external LN4898 Audio Power Amplifier on GPIOA Pin 8.
pub struct AudioAmplifier {
    enabled: bool,
    volume: u8,
}

impl AudioAmplifier {
    pub const fn new() -> Self {
        Self {
            enabled: false,
            volume: 5,
        }
    }

    pub fn init(&mut self) {
        self.disable();
    }

    /// Enable LN4898 power amplifier by driving PA8 High.
    pub fn enable(&mut self) {
        self.enabled = true;
        unsafe {
            HANGOVER_MS = 0;
        }
        AUDIO_PA.set_high();
    }

    /// Disable LN4898 power amplifier by driving PA8 Low (ultra-low power shutdown).
    pub fn disable(&mut self) {
        self.enabled = false;
        unsafe {
            HANGOVER_MS = 0;
        }
        AUDIO_PA.set_low();
    }

    /// Keep amplifier energized after tone playback to eliminate key repeat pops.
    pub fn notify_tone_played() {
        unsafe {
            HANGOVER_MS = 150;
        }
    }

    /// Periodic system tick to step hangover shutdown timer (called every 16ms).
    pub fn tick(&mut self, elapsed_ms: u16) {
        if !self.enabled {
            unsafe {
                if HANGOVER_MS > 0 {
                    if elapsed_ms >= HANGOVER_MS {
                        HANGOVER_MS = 0;
                        AUDIO_PA.set_low();
                    } else {
                        HANGOVER_MS -= elapsed_ms;
                    }
                }
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_active(&self) -> bool {
        self.enabled || unsafe { HANGOVER_MS > 0 } || AUDIO_PA.is_high()
    }

    pub fn volume(&self) -> u8 {
        self.volume
    }

    pub fn set_volume(&mut self, level: u8) {
        self.volume = level.min(10);
    }

    pub fn hardware_gain(&self) -> (u8, u8) {
        VOLUME_TABLE[self.volume as usize]
    }
}

/// RAII Mute Guard that temporarily mutes speaker during FSK bursts and unmutes on drop.
pub struct MuteGuard<'a> {
    amp: &'a mut AudioAmplifier,
}

impl<'a> MuteGuard<'a> {
    pub fn new(amp: &'a mut AudioAmplifier) -> Self {
        amp.disable();
        Self { amp }
    }
}

impl<'a> Drop for MuteGuard<'a> {
    fn drop(&mut self) {
        self.amp.enable();
    }
}

impl kernel::service::AudioService for AudioAmplifier {
    fn set_volume(&mut self, level: u8) {
        Self::set_volume(self, level);
    }
    fn volume(&self) -> u8 {
        Self::volume(self)
    }
    fn enable(&mut self) {
        Self::enable(self);
    }
    fn disable(&mut self) {
        Self::disable(self);
    }
    fn is_enabled(&self) -> bool {
        Self::is_enabled(self)
    }
    fn is_active(&self) -> bool {
        Self::is_active(self)
    }
    fn hardware_gain(&self) -> (u8, u8) {
        Self::hardware_gain(self)
    }
}
