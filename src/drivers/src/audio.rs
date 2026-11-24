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

use core::sync::atomic::{AtomicU16, Ordering};
use kern_platform_traits::audio::{AudioPa, VoiceAdcBackend};
use kern_platform_traits::time::Delay;

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

static HANGOVER_MS: AtomicU16 = AtomicU16::new(0);

/// Hardware driver for the external LN4898 Audio Power Amplifier.
pub struct AudioAmplifier<P> {
    pa: P,
    enabled: bool,
    volume: u8,
}

impl<P: AudioPa> AudioAmplifier<P> {
    pub const fn new(pa: P) -> Self {
        Self {
            pa,
            enabled: false,
            volume: 5,
        }
    }

    pub fn init(&mut self) {
        self.disable();
    }

    /// Enable LN4898 power amplifier.
    pub fn enable(&mut self) {
        self.enabled = true;
        HANGOVER_MS.store(0, Ordering::Relaxed);
        self.pa.enable();
    }

    /// Disable LN4898 power amplifier (ultra-low power shutdown).
    pub fn disable(&mut self) {
        self.enabled = false;
        HANGOVER_MS.store(0, Ordering::Relaxed);
        self.pa.disable();
    }

    /// Keep amplifier energized after tone playback to eliminate key repeat pops.
    pub fn notify_tone_played() {
        HANGOVER_MS.store(2000, Ordering::Relaxed);
    }

    /// Check if post-tone hangover is currently keeping the amplifier energized.
    #[inline(always)]
    pub fn is_hangover_active() -> bool {
        HANGOVER_MS.load(Ordering::Relaxed) > 0
    }

    /// Immediately cancel any active hangover and shut down the LN4898 amplifier.
    pub fn cancel_hangover(&mut self) {
        HANGOVER_MS.store(0, Ordering::Relaxed);
        self.pa.disable();
    }

    /// Periodic system tick to step hangover shutdown timer (called every 16ms).
    pub fn tick(&mut self, elapsed_ms: u16) {
        if !self.enabled {
            let rem = HANGOVER_MS.load(Ordering::Relaxed);
            if rem > 0 {
                if elapsed_ms >= rem {
                    HANGOVER_MS.store(0, Ordering::Relaxed);
                    self.pa.disable();
                } else {
                    HANGOVER_MS.store(rem - elapsed_ms, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_active(&self) -> bool {
        self.enabled || HANGOVER_MS.load(Ordering::Relaxed) > 0 || self.pa.is_enabled()
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

impl<P: AudioPa + Default> Default for AudioAmplifier<P> {
    fn default() -> Self {
        Self::new(P::default())
    }
}

/// RAII Mute Guard that temporarily mutes speaker during FSK bursts and unmutes on drop.
pub struct MuteGuard<'a, P: AudioPa> {
    amp: &'a mut AudioAmplifier<P>,
}

impl<'a, P: AudioPa> MuteGuard<'a, P> {
    pub fn new(amp: &'a mut AudioAmplifier<P>) -> Self {
        amp.disable();
        Self { amp }
    }
}

impl<'a, P: AudioPa> Drop for MuteGuard<'a, P> {
    fn drop(&mut self) {
        self.amp.enable();
    }
}

impl<P: AudioPa> kernel::service::AudioService for AudioAmplifier<P> {
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

impl<P: AudioPa> kern_platform_traits::audio::AudioPa for AudioAmplifier<P> {
    fn enable(&mut self) {
        Self::enable(self);
    }
    fn disable(&mut self) {
        Self::disable(self);
    }
    fn is_enabled(&self) -> bool {
        Self::is_enabled(self)
    }
}

// ── Continuous Audio Input & ADC Sampling Driver ───────────────────────────

/// Portable Continuous Audio Input & ADC Sampling Driver.
///
/// Delegates MCU ADC/DAC register access to `VoiceAdcBackend + Delay` while owning:
/// - Baseline 8-sample DC offset pre-calibration
/// - Adaptive 64-sample IIR DC-removal high-pass filter (`dc_bias += (raw - dc_bias) >> 6`)
/// - 12-bit centered to 16-bit PCM scaling and non-blocking abort polling every 64 samples
pub struct AudioCaptureDriver<B> {
    backend: B,
    active: bool,
    period_us: u16,
    dc_bias_est: i32,
}

impl<B: VoiceAdcBackend + Delay> AudioCaptureDriver<B> {
    pub const fn new(backend: B) -> Self {
        Self {
            backend,
            active: false,
            period_us: 125,
            dc_bias_est: 2048,
        }
    }

    #[inline(never)]
    pub fn start_capture(&mut self, rate_hz: u16) -> Result<(), ()> {
        if self.active {
            return Ok(());
        }

        self.backend.begin_voice_bias();

        // Calibrate baseline DC bias: average 8 initial conversions
        let mut sum: i32 = 0;
        for _ in 0..8 {
            sum += self.backend.read_voice_sample();
        }

        let rate = if rate_hz == 0 {
            8000
        } else {
            rate_hz.clamp(1000, 24000)
        };
        self.period_us = (1_000_000 / (rate as u32)).max(10) as u16;
        self.dc_bias_est = sum >> 3;
        self.active = true;
        Ok(())
    }

    /// Non-blocking single-sample capture with adaptive 64-sample IIR DC-removal filter.
    #[inline(always)]
    pub fn read_sample_nonblocking(&mut self) -> Option<i16> {
        if !self.active {
            return None;
        }
        let raw = self.backend.read_voice_sample();
        self.dc_bias_est += (raw - self.dc_bias_est) >> 6;
        let centered = raw - self.dc_bias_est;
        Some((centered << 4).clamp(-32768, 32767) as i16)
    }

    #[inline(never)]
    pub fn read_samples(&mut self, out_samples: &mut [i16]) -> usize {
        if !self.active || out_samples.is_empty() {
            return 0;
        }

        self.backend.select_voice_channel();
        let period_us = self.period_us as u32;

        for (i, sample) in out_samples.iter_mut().enumerate() {
            if (i & 0x0F) == 0 && i > 0 && self.backend.should_abort() {
                return i;
            }

            if let Some(s) = self.read_sample_nonblocking() {
                *sample = s;
            }

            self.backend.delay_us(period_us);
        }

        out_samples.len()
    }

    pub fn stop_capture(&mut self) {
        if !self.active {
            return;
        }
        self.backend.end_voice_bias();
        self.active = false;
    }

    pub fn is_capturing(&self) -> bool {
        self.active
    }
}

impl<B: VoiceAdcBackend + Delay + Default> Default for AudioCaptureDriver<B> {
    fn default() -> Self {
        Self::new(B::default())
    }
}

impl<B: VoiceAdcBackend + Delay> kern_platform_traits::audio::AudioCapture
    for AudioCaptureDriver<B>
{
    fn start_capture(&mut self, rate_hz: u16) -> Result<(), ()> {
        Self::start_capture(self, rate_hz)
    }

    fn read_samples(&mut self, out: &mut [i16]) -> usize {
        Self::read_samples(self, out)
    }

    fn stop_capture(&mut self) {
        Self::stop_capture(self);
    }

    fn is_capturing(&self) -> bool {
        Self::is_capturing(self)
    }
}

impl<B: VoiceAdcBackend + Delay> kernel::service::AudioCaptureService for AudioCaptureDriver<B> {
    fn start_capture(&mut self, rate_hz: u16) -> Result<(), ()> {
        Self::start_capture(self, rate_hz)
    }

    fn read_samples(&mut self, out: &mut [i16]) -> usize {
        Self::read_samples(self, out)
    }

    fn stop_capture(&mut self) {
        Self::stop_capture(self);
    }

    fn is_capturing(&self) -> bool {
        Self::is_capturing(self)
    }
}
