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
            HANGOVER_MS = 250;
        }
    }

    /// Immediately cancel any active hangover and shut down the LN4898 amplifier.
    pub fn cancel_hangover() {
        unsafe {
            HANGOVER_MS = 0;
        }
        AUDIO_PA.set_low();
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

impl kern_platform_traits::audio::AudioPa for AudioAmplifier {
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

/// Hardware driver for Continuous Audio Input & ADC Sampling on Quansheng UV-K1 / K5.
///
/// Uses PA4 (VOICE_DAC) in analog mode with DAC1 Channel 1 providing a stable 1.65V DC midpoint bias,
/// and ADC1 Channel 4 sampling the AC-coupled voice signal.
///
/// Hardening & Anti-Glitch Invariants:
/// - Baseline DC offset pre-calibration (averages 8 startup conversions to eliminate startup transient pops)
/// - Adaptive 64-sample IIR DC-removal high-pass filter: `dc_bias += (raw - dc_bias) >> 6`
/// - Hardware ADC channel conflict protection: re-verifies `ADC_SQR3 = 4` per batch to prevent battery telemetry hijacking
/// - Non-blocking abort check: polls keypad matrix every 64 samples to keep the UI interactive
/// - Microsecond timing precision across all supported MCU clock speeds
#[inline(never)]
unsafe fn adc_convert_single(cr2: u32) -> i32 {
    use py32_pac as pac;
    unsafe {
        pac::adc::ADC_SR.write_volatile(!pac::adc::SR_EOC);
        pac::adc::ADC_CR2.write_volatile(cr2 | pac::adc::CR2_EXTTRIG | pac::adc::CR2_SWSTART);
        let mut timeout = 2000;
        while (pac::adc::ADC_SR.read_volatile() & pac::adc::SR_EOC) == 0 && timeout > 0 {
            timeout -= 1;
        }
        (pac::adc::ADC_DR.read_volatile() & 0x0FFF) as i32
    }
}

#[inline(never)]
unsafe fn adc_select_channel(ch: u32) {
    use py32_pac as pac;
    unsafe {
        pac::adc::ADC_SQR1.write_volatile(0);
        pac::adc::ADC_SQR3.write_volatile(ch);
    }
}

pub struct AudioCaptureDriver {
    active: bool,
    rate_hz: u16,
    dc_bias_est: i32,
}

impl AudioCaptureDriver {
    pub const fn new() -> Self {
        Self {
            active: false,
            rate_hz: 8000,
            dc_bias_est: 2048,
        }
    }

    pub fn start_capture(&mut self, rate_hz: u16) -> Result<(), ()> {
        if self.active {
            return Ok(());
        }

        unsafe {
            use py32_pac as pac;

            // 1. Configure VOICE_DAC pin (PA4) in analog mode to receive AC-coupled signal
            bsp::pins::VOICE_DAC.configure_analog();

            // 2. Enable DAC1 peripheral clock in RCC APB1ENR (bit 29)
            let apb1 = pac::rcc::RCC_APB1ENR.read_volatile();
            pac::rcc::RCC_APB1ENR.write_volatile(apb1 | pac::rcc::APB1ENR_DAC);

            // 3. Configure DAC1 Channel 1: enable output buffer, inject 1.65V DC midpoint bias (2048)
            pac::dac::DAC_DHR12R1.write_volatile(2048);
            pac::dac::DAC_CR.write_volatile(pac::dac::CR_EN1);
            pac::dac::DAC_SWTRIGR.write_volatile(pac::dac::SWTRIGR_SWTRIG1);

            // 4. Ensure ADC1 is powered and select Channel 4 (PA4 / ADC_IN4)
            let mut cr2 = pac::adc::ADC_CR2.read_volatile();
            if (cr2 & pac::adc::CR2_ADON) == 0 {
                cr2 |= pac::adc::CR2_ADON | pac::adc::CR2_EXTSEL_SWSTART;
                pac::adc::ADC_CR2.write_volatile(cr2);
                py32_hal::delay::delay_us(150);
            }
            adc_select_channel(4); // Channel 4

            py32_hal::delay::delay_us(50);

            // 5. Calibrate baseline DC bias: take 8 initial conversions and average them
            let mut sum: i32 = 0;
            for _ in 0..8 {
                sum += adc_convert_single(cr2);
            }

            self.rate_hz = if rate_hz == 0 {
                8000
            } else {
                rate_hz.clamp(1000, 24000)
            };
            self.dc_bias_est = sum >> 3;
            self.active = true;
        }
        Ok(())
    }

    pub fn read_samples(&mut self, out_samples: &mut [i16]) -> usize {
        if !self.active || out_samples.is_empty() {
            return 0;
        }

        unsafe {
            use py32_pac as pac;

            // Re-verify ADC is set to Channel 4 (in case battery telemetry polled Channel 8)
            adc_select_channel(4);

            let period_us = (1_000_000 / (self.rate_hz as u32)).max(10);
            let cr2 = pac::adc::ADC_CR2.read_volatile();

            for (i, sample) in out_samples.iter_mut().enumerate() {
                // Check keypad every 64 samples for non-blocking abort
                if (i & 0x3F) == 0 && i > 0 {
                    let key_mask = crate::keyboard::KeypadScanner::poll_raw_mask();
                    if key_mask != 0 {
                        return i;
                    }
                }

                let raw = adc_convert_single(cr2);

                // Adaptive 64-sample IIR DC-removal filter
                self.dc_bias_est += (raw - self.dc_bias_est) >> 6;
                let centered = raw - self.dc_bias_est;

                // Scale 12-bit centered audio (-2048..+2047) to 16-bit PCM (-32768..+32767)
                *sample = (centered << 4).clamp(-32768, 32767) as i16;

                py32_hal::delay::delay_us(period_us);
            }

            out_samples.len()
        }
    }

    pub fn stop_capture(&mut self) {
        if !self.active {
            return;
        }

        unsafe {
            use py32_pac as pac;

            // 1. Disable DAC1 output
            pac::dac::DAC_CR.write_volatile(0);

            // 2. Disable DAC clock in APB1ENR
            let apb1 = pac::rcc::RCC_APB1ENR.read_volatile();
            pac::rcc::RCC_APB1ENR.write_volatile(apb1 & !pac::rcc::APB1ENR_DAC);

            // 3. Put PA4 back into low-leakage analog mode
            bsp::pins::VOICE_DAC.configure_analog();

            // 4. Restore ADC sequence to Channel 8 (Battery voltage)
            adc_select_channel(8);

            self.active = false;
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.active
    }
}

impl kern_platform_traits::audio::AudioCapture for AudioCaptureDriver {
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

impl kernel::service::AudioCaptureService for AudioCaptureDriver {
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
