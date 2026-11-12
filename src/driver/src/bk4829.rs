//! High-assurance, zero-cost typestate driver for the BK4829 RF Transceiver.
//!
//! Provides a 128x16-bit shadow register cache to mathematically eliminate redundant
//! SPI bus transactions for high-performance DSP loops and CSMA/CA routines.

pub mod af;
pub mod agc;
pub mod fsk;
pub mod registers;
pub mod squelch;
pub mod subaudible;
pub mod tuning;

use core::marker::PhantomData;
use registers::*;
use utils::units::{Frequency, Hertz};

// ── Hardware Enumerations ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AfType {
    Mute = 0,
    Fm = 1,
    Alarm = 2,
    Beep = 3,
    Baseband1 = 4, // RAW
    Baseband2 = 5, // USB
    Ctco = 6,      // CTCSS LF line
    Am = 7,
    Fsko = 8, // FSK modem output
    Byp = 9,  // Demodulator bypass
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FilterBw {
    Wide = 0,
    Narrow = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TxModulation {
    Fm = 0,
    Fsk = 1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CtcssTone {
    Hz67_0 = 0,
    Hz74_4 = 1,
    Hz254_1 = 39,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SquelchType {
    Carrier = 0,
    Ctcss = 1,
    Cdcss = 2,
}

// ── ZST Typestates ──────────────────────────────────────────────────────────

/// State representing a fully disabled radio (deep sleep).
pub struct Disabled;
/// State representing an active receiver.
pub struct Receiving;
/// State representing an active transmitter.
pub struct Transmitting;

// ── Register Cache Structure ────────────────────────────────────────────────

pub struct RegisterCache {
    /// Shadow array of all 128 BK4829 16-bit registers (0x00 to 0x7F)
    regs: [u16; 128],
}

impl RegisterCache {
    pub const fn new() -> Self {
        Self {
            regs: [0xFFFF; 128],
        }
    }

    /// Read a cached register value without hitting the physical SPI bus.
    #[inline(always)]
    pub fn read(&self, addr: u8) -> u16 {
        self.regs[(addr & 0x7F) as usize]
    }

    /// Updates cache and returns true if a physical SPI write is required.
    #[inline(always)]
    pub fn write_check(&mut self, addr: u8, val: u16) -> bool {
        let idx = (addr & 0x7F) as usize;
        if self.regs[idx] != val {
            self.regs[idx] = val;
            true
        } else {
            false
        }
    }

    /// Invalidate a cache entry so next write is guaranteed to hit the bus
    #[inline(always)]
    pub fn invalidate(&mut self, addr: u8) {
        self.regs[(addr & 0x7F) as usize] = 0xFFFF;
    }
}

impl Default for RegisterCache {
    fn default() -> Self {
        Self::new()
    }
}

// ── BK4829 Driver ───────────────────────────────────────────────────────────

pub struct Bk4829<'a, State = Disabled> {
    pub(crate) cache: RegisterCache,
    pub(crate) bus: &'a mut dyn kern_platform_traits::radio::RadioBus,
    pub(crate) tone_gain: u8,
    pub(crate) active_demod_af: AfType,
    pub(crate) rx_demod_active: bool,
    pub(crate) calib: Option<&'a [u8; 512]>,
    pub(crate) current_freq_hz: u32,
    pub(crate) squelch_level: u8,
    pub(crate) squelch_open: bool,
    pub(crate) subaudible_matched: bool,
    pub(crate) tail_burst_latched: bool,
    pub(crate) loopback_active: bool,
    pub(crate) baseline_xtal_trim: i16,
    pub(crate) rx_subaudible: kernel::service::radio::SubaudibleTone,
    pub(crate) tx_subaudible: kernel::service::radio::SubaudibleTone,
    pub(crate) am_tracker: kernel::service::radio::AmLinearityTracker,
    pub(crate) _state: PhantomData<State>,
}

impl<'a, State> Bk4829<'a, State> {
    /// Forces a physical SPI read, updates the cache, and returns the value.
    pub fn read_reg(&mut self, addr: u8) -> u16 {
        let val = self.bus.read_reg(addr);
        self.cache.write_check(addr, val);
        val
    }

    /// Validates against the shadow cache and conditionally performs physical SPI write.
    pub fn write_reg(&mut self, addr: u8, val: u16) {
        if self.cache.write_check(addr, val) {
            self.bus.write_reg(addr, val);
        }
    }

    /// Read Chip ID from register 0x00.
    pub fn read_chip_id(&mut self) -> u16 {
        self.read_reg(REG_CHIP_ID)
    }

    /// Soft Reset: Clears RF core configuration.
    pub fn soft_reset(&mut self) {
        self.write_reg(REG_SYS_POWER, 0x0000);
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_SLEEP);
    }

    pub fn set_crystal_trim(&mut self, trim: i16) {
        let val = tuning::calculate_xtal_freq_low(trim);
        self.write_reg(REG_XTAL_FREQ_LOW, val);
    }

    /// Program a 23/24-bit Golay CDCSS codeword into `REG_08` (low 12 bits + high 12 bits).
    pub(crate) fn write_cdcss_golay_code(&mut self, octal_code: u16) {
        let golay = kernel::service::radio::tone::dcs_octal_to_golay23(octal_code);
        let low12 = (golay & 0x0FFF) as u16;
        let high12 = 0x8000 | (((golay >> 12) & 0x0FFF) as u16);
        self.write_reg(REG_CDCSS_CODE, low12);
        self.write_reg(REG_CDCSS_CODE, high12);
    }

    /// Apply a subaudible tone configuration (`None`, `Ctcss`, `DcsNormal`, `DcsInverted`)
    /// to hardware registers `REG_07`, `REG_08`, `REG_51`, and `REG_52`.
    pub(crate) fn apply_subaudible_hw(&mut self, tone: kernel::service::radio::SubaudibleTone) {
        self.write_reg(REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_NORMAL);
        let ctrl = match tone {
            kernel::service::radio::SubaudibleTone::None => {
                self.write_reg(REG_CXCSS_FREQ, subaudible::CTC2_55HZ_DETECT_REG);
                0x0000
            }
            kernel::service::radio::SubaudibleTone::Ctcss(deci_hz) => {
                self.write_reg(REG_CXCSS_FREQ, subaudible::CTC2_55HZ_DETECT_REG);
                self.write_reg(REG_CXCSS_FREQ, subaudible::ctcss_deci_hz_to_reg(deci_hz));
                subaudible::CXCSS_CTRL_CTCSS
            }
            kernel::service::radio::SubaudibleTone::DcsNormal(code)
            | kernel::service::radio::SubaudibleTone::DcsInverted(code) => {
                self.write_reg(REG_CXCSS_FREQ, subaudible::CDCSS_134_4HZ_BAUD_REG);
                self.write_cdcss_golay_code(code);
                if matches!(tone, kernel::service::radio::SubaudibleTone::DcsInverted(_)) {
                    subaudible::CXCSS_CTRL_CDCSS_INV
                } else {
                    subaudible::CXCSS_CTRL_CDCSS_NORM
                }
            }
        };
        self.write_reg(REG_CXCSS_CTRL, ctrl);
    }

    pub fn set_af(&mut self, af: AfType) {
        self.write_reg(REG_AF_ROUTING, 0x6042 | ((af as u16) << 8));
    }

    pub fn set_volume_level(&mut self, level: u8) {
        if level == 0 {
            let val = 11u16 << 12;
            self.write_reg(REG_AF_LEVEL, val);
            return;
        }
        let vol_idx = (level as usize).min(crate::audio::VOLUME_TABLE.len() - 1);
        let (mut rx_gain2, mut dac_gain) = crate::audio::VOLUME_TABLE[vol_idx];
        if let Some(calib) = self.calib {
            let block = calib::CalibrationBlock::from_bytes(calib);
            let max_vol = block.volume_gain();
            let max_dac = block.dac_gain();
            let prod = rx_gain2 as u32 * max_vol as u32;
            rx_gain2 = (((prod * 0x046A) >> 16) as u8).min(63);
            dac_gain = dac_gain.min(max_dac);
        }
        let val = (11u16 << 12) | ((rx_gain2 as u16 & 0x3F) << 4) | (dac_gain as u16 & 0x0F);
        self.write_reg(REG_AF_LEVEL, val);
    }

    pub fn set_mic_gain(&mut self, gain_db: u8) {
        let val = if gain_db < 5 {
            if let Some(calib) = self.calib {
                let block = calib::CalibrationBlock::from_bytes(calib);
                block.get_mic_gain_db(gain_db)
            } else {
                match gain_db {
                    0 => 8,
                    1 => 14,
                    2 => 19,
                    3 => 24,
                    _ => 30,
                }
            }
        } else {
            gain_db
        };
        let gain_bits = (val.min(31) as u16) & 0x1F;
        let mut reg44 = self.cache.read(REG_MIC_GAIN);
        reg44 = (reg44 & !0x001F) | gain_bits;
        self.write_reg(REG_MIC_GAIN, reg44);
    }

    pub fn set_tone_gain(&mut self, gain: u8) {
        self.tone_gain = gain.min(127);
    }

    pub fn set_audio_profile(&mut self, _profile: u8) {}

    pub fn play_tone(&mut self, freq_hz: u16, duration_ms: u16) {
        if freq_hz == 0 || duration_ms == 0 {
            return;
        }
        self.play_tone_sequence(&[kernel::service::radio::af::ToneNote::new(
            freq_hz,
            duration_ms,
        )]);
    }

    pub fn play_tone_sequence(&mut self, notes: &[kernel::service::radio::af::ToneNote]) {
        if notes.is_empty() || self.tone_gain == 0 {
            return;
        }

        // 1. Prepare tone generator with TX Mute active (prevents clicks/pops while configuring)
        self.write_reg(REG_TX_MUTE, 0xBB18);
        self.set_af(AfType::Beep);
        let tone_cfg = 0x8000 | ((self.tone_gain as u16 & 0x7F) << 8);
        self.write_reg(REG_TONE_GAIN, tone_cfg);

        // 2. Isolate RF RX Demodulator during tone playback:
        // Set REG_SYS_POWER to AF_DAC (bit 9) | DISC_MODE (bit 8) | TX_DSP (bit 1) = 0x0302.
        // This isolates the FM discriminator and RX DSP so receiver hiss/whistle never mixes into the tone.
        let cur_pwr = self.cache.read(REG_SYS_POWER);
        let rx_pwr = if cur_pwr != 0xFFFF && (cur_pwr & 0x0001) != 0 {
            cur_pwr & !0x8000
        } else {
            0x3FF1
        };
        self.write_reg(REG_SYS_POWER, 0x0302);

        // 3. Hardware Audio Power Amplifier Warm-Up:
        // On Quansheng UV-K1 / K5, LN4898 audio power amplifier (PA8) warm-up
        // requires ~50ms to charge the bypass capacitor from cold shutdown for pop-free, audible playback.
        let was_amp_active = bsp::pins::AUDIO_PA.is_high();
        if !was_amp_active {
            bsp::pins::AUDIO_PA.set_high();
            py32_hal::delay::delay_ms(50);
        }

        // 4. Play note sequence
        for note in notes {
            if note.freq_hz == 0 || note.duration_ms == 0 {
                self.write_reg(REG_TX_MUTE, 0xBB18);
                self.write_reg(REG_TONE1_FREQ, 0x0000);
                py32_hal::delay::delay_ms(note.duration_ms as u32);
            } else {
                let reg_val = af::tone_freq_to_reg(note.freq_hz);
                self.write_reg(REG_TONE1_FREQ, reg_val);
                self.write_reg(REG_TX_MUTE, 0x3B18); // Unmute clean tone to DAC
                py32_hal::delay::delay_ms(note.duration_ms as u32);
                self.write_reg(REG_TX_MUTE, 0xBB18); // Mute at note boundary
            }
        }

        // 5. Clean up tone generator
        self.write_reg(REG_TONE1_FREQ, 0x0000);
        self.write_reg(REG_TONE_GAIN, 0x0000);
        py32_hal::delay::delay_ms(5);

        // 6. Restore clean RX / idle state without resetting RF PLL or auto-calibrating VCO
        self.write_reg(REG_SYS_POWER, rx_pwr);

        if self.rx_demod_active && self.is_squelch_open() {
            self.set_af(self.active_demod_af);
            self.write_reg(REG_TX_MUTE, 0x3B18);
        } else {
            self.set_af(AfType::Mute);
            self.write_reg(REG_TX_MUTE, 0xBB18);
        }
        crate::audio::AudioAmplifier::notify_tone_played();
    }

    pub fn begin_tone_sequence(&mut self) -> bool {
        self.write_reg(REG_TX_MUTE, 0xBB18);
        self.set_af(AfType::Beep);
        let tone_cfg = 0x8000 | ((self.tone_gain as u16 & 0x7F) << 8);
        self.write_reg(REG_TONE_GAIN, tone_cfg);
        self.write_reg(REG_SYS_POWER, 0x0302);

        let was_amp_active = bsp::pins::AUDIO_PA.is_high();
        if !was_amp_active {
            bsp::pins::AUDIO_PA.set_high();
        }
        was_amp_active
    }

    pub fn step_tone_note(&mut self, freq_hz: u16) {
        if freq_hz == 0 {
            self.write_reg(REG_TX_MUTE, 0xBB18);
            self.write_reg(REG_TONE1_FREQ, 0x0000);
        } else {
            let reg_val = af::tone_freq_to_reg(freq_hz);
            self.write_reg(REG_TONE1_FREQ, reg_val);
            self.write_reg(REG_TX_MUTE, 0x3B18);
        }
    }

    pub fn mute_tone_note(&mut self) {
        self.write_reg(REG_TX_MUTE, 0xBB18);
    }

    pub fn end_tone_sequence(&mut self, _was_amp_active: bool) {
        self.write_reg(REG_TONE1_FREQ, 0x0000);
        self.write_reg(REG_TONE_GAIN, 0x0000);
        py32_hal::delay::delay_ms(5);
        self.write_reg(REG_SYS_POWER, 0xBFF1);

        if self.rx_demod_active && self.is_squelch_open() {
            self.set_af(self.active_demod_af);
            self.write_reg(REG_TX_MUTE, 0x3B18);
        } else {
            self.set_af(AfType::Mute);
            self.write_reg(REG_TX_MUTE, 0xBB18);
        }
        crate::audio::AudioAmplifier::notify_tone_played();
    }

    pub fn play_tone_sequence_with_gain(
        &mut self,
        notes: &[kernel::service::radio::af::ToneNote],
        gain: u8,
    ) {
        let prev_gain = self.tone_gain;
        self.set_tone_gain(gain);
        self.play_tone_sequence(notes);
        self.set_tone_gain(prev_gain);
    }

    pub fn set_xtal_freq_trim(&mut self, trim: i16) {
        self.baseline_xtal_trim = trim;
        self.set_crystal_trim(trim);
    }

    /// Mute or unmute the microphone audio path into the RF modulator (REG_TX_MUTE).
    #[inline(always)]
    pub fn mute_mic(&mut self, mute: bool) {
        self.write_reg(REG_TX_MUTE, if mute { 0xBB18 } else { 0x3B18 });
    }

    /// Set PA bias/gain (REG_36). Only touches the PA register — never the
    /// power-enable (0x30) or bandgap/LDO (0x37) registers.
    pub fn set_rf_power(&mut self, level: u8) {
        let bias: u8 = match level {
            0 => 0x20, // Low
            1 => 0x60, // Medium
            _ => 0xA0, // High
        };
        self.set_rf_power_bias(bias);
    }

    /// Set calibrated PA bias directly into REG_36.
    pub fn set_rf_power_bias(&mut self, bias: u8) {
        let gain: u16 = if self.current_freq_hz < 280_000_000 {
            1 << 3
        } else {
            (4 << 3) | 2
        };
        self.write_reg(0x36, ((bias as u16) << 8) | (1 << 7) | gain);
    }

    /// Soft-start S-curve PA ramp-up with precise calibrated PA bias.
    pub fn tx_ramp_up_bias(&mut self, bias: u8) {
        self.set_gpio_pin(1, true); // PA enable (Pin 29)
        self.set_gpio_pin(0, false); // RX disable (Pin 28)
        py32_hal::delay::delay_ms(1);
        self.set_rf_power_bias(bias >> 2);
        py32_hal::delay::delay_ms(1);
        self.set_rf_power_bias(bias >> 1);
        py32_hal::delay::delay_ms(1);
        self.set_rf_power_bias(((bias as u16 * 3) >> 2) as u8);
        py32_hal::delay::delay_ms(1);
        self.set_rf_power_bias(bias);
        py32_hal::delay::delay_ms(1);
    }

    /// PA off (REG_36 reset value used by init).
    pub fn rf_power_off(&mut self) {
        self.write_reg(0x36, 0x0022);
    }

    pub fn set_squelch_profile(&mut self, th: &kernel::service::radio::squelch::SquelchThresholds) {
        let r4d = 0xA000 | (th.close_glitch & 0x00FF);
        let r4e =
            (1u16 << 14) | (5u16 << 11) | (3u16 << 9) | (1u16 << 8) | (th.open_glitch & 0x00FF);
        let r4f = ((th.close_noise & 0x007F) << 8) | (th.open_noise & 0x007F);
        let r78 = ((th.open_rssi & 0x00FF) << 8) | (th.close_rssi & 0x00FF);
        self.write_reg(REG_SQL_GLITCH_CLOSE, r4d);
        self.write_reg(REG_SQL_GLITCH_OPEN, r4e);
        self.write_reg(REG_SQL_NOISE, r4f);
        self.write_reg(REG_SQL_RSSI, r78);
    }

    /// Acknowledge and clear pending hardware interrupts.
    #[inline(always)]
    pub fn clear_interrupts(&mut self) {
        self.write_reg(REG_INT_STATUS, 0);
    }

    /// Poll pending interrupt events from BK4829, update internal latch states, and clear IRQ.
    pub fn poll_interrupts(&mut self) -> u16 {
        let reg0c = self.read_reg(REG_STATUS);
        if (reg0c & 0x0001) != 0 {
            let irq = self.read_reg(REG_INT_STATUS);
            self.write_reg(REG_INT_STATUS, 0);

            if (irq & INT_SQUELCH_LOST) != 0 {
                self.squelch_open = true;
            }
            if (irq & INT_SQUELCH_FOUND) != 0 {
                self.squelch_open = false;
            }
            if (irq & (INT_CTCSS_FOUND | INT_CDCSS_FOUND)) != 0 {
                self.subaudible_matched = true;
            }
            if (irq & INT_CXCSS_TAIL) != 0 {
                self.tail_burst_latched = true;
                self.subaudible_matched = false;
            }
            if (irq & (INT_CTCSS_LOST | INT_CDCSS_LOST)) != 0 {
                self.subaudible_matched = false;
            }
            irq
        } else {
            0
        }
    }

    pub fn is_squelch_open(&mut self) -> bool {
        self.poll_interrupts();
        if self.squelch_level == 0 {
            true
        } else {
            let reg0c = self.read_reg(REG_STATUS);
            let hw_open = ((reg0c >> 1) & 0x0001) != 0;
            self.squelch_open = hw_open;
            hw_open
        }
    }

    pub fn is_subaudible_matched(&mut self) -> bool {
        self.poll_interrupts();
        let reg0c = self.read_reg(REG_STATUS);
        let ctc1 = (reg0c >> 10) & 0x01;
        let dcs = (reg0c >> 14) & 0x03;
        self.subaudible_matched || ctc1 != 0 || dcs != 0
    }

    /// Check if hardware detected a 55 Hz CTC2 tail burst (bit 11 of `REG_0C`)
    /// or a 120°/180°/240° CTCSS phase shift reverse burst (bits `[13:12]` of `REG_0C`).
    pub fn has_tail_burst_detected(&mut self) -> bool {
        self.poll_interrupts();
        let reg0c = self.read_reg(REG_STATUS);
        let ctc2_55hz = (reg0c & (1 << 11)) != 0;
        let phase_shift = ((reg0c >> 12) & 0x03) != 0;
        let latched = self.tail_burst_latched;
        self.tail_burst_latched = false;
        latched || ctc2_55hz || phase_shift
    }

    pub fn get_snr(&mut self) -> u8 {
        let reg61 = self.read_reg(REG_SNR_VAL);
        (reg61 & 0xFF) as u8
    }

    pub fn get_glitch(&mut self) -> u8 {
        let reg63 = self.read_reg(REG_GLITCH_VAL);
        (reg63 & 0xFF) as u8
    }

    pub fn get_noise(&mut self) -> u8 {
        let reg65 = self.read_reg(REG_NOISE_VAL);
        (reg65 & 0x7F) as u8
    }

    /// Enable or disable internal TX-to-RX analog loopback (`REG_30 = 0xFFF7`, PA disabled).
    ///
    /// Couples the internal modulator and tone/CTCSS synthesizer directly into the receiver
    /// mixer through an internal attenuation path with zero RF emitted to the antenna.
    pub fn set_internal_loopback(&mut self, enabled: bool) {
        self.loopback_active = enabled;
        if enabled {
            // Strictly guarantee PA bias and external PA switch are OFF before enabling internal coupling
            self.rf_power_off();
            self.set_gpio_pin(1, false); // PA disable (Pin 29)
            self.set_gpio_pin(0, false); // Isolate external RX antenna switch
            self.write_reg(REG_POWER_CTRL, POWER_CTRL_RX);
            self.write_reg(REG_SYS_POWER, SYS_POWER_LOOPBACK);
        } else {
            self.set_gpio_pin(0, true); // Restore RX antenna switch
            self.write_reg(REG_POWER_CTRL, POWER_CTRL_RX);
            self.write_reg(REG_SYS_POWER, SYS_POWER_RX);
        }
    }

    /// Read instantaneous voice/modulator amplitude (`Reg 0x64`).
    pub fn get_voice_amplitude(&mut self) -> u16 {
        self.read_reg(REG_AUDIO_AMP)
    }

    /// Read signed Automatic Frequency Control (AFC) frequency offset indicator (`Reg 0x6D`).
    pub fn get_afc_offset(&mut self) -> i16 {
        self.read_reg(REG_AFC_VAL) as i16
    }

    /// Read adjacent channel power ratio (`Reg 0x66`), returning `(upper_adj, lower_adj)`.
    pub fn get_adjacent_channel_power(&mut self) -> (u8, u8) {
        let val = self.read_reg(REG_ADJ_CH_PWR);
        (((val >> 8) & 0xFF) as u8, (val & 0xFF) as u8)
    }

    /// Read LNA peak RSSI indicator (`Reg 0x62` bits `[15:8]`).
    pub fn get_lna_peak_rssi(&mut self) -> u8 {
        ((self.read_reg(REG_LNA_PEAK_RSSI) >> 8) & 0xFF) as u8
    }

    /// Read internal AGC RSSI indicator (`Reg 0x62` bits `[7:0]`).
    pub fn get_agc_rssi(&mut self) -> u8 {
        (self.read_reg(REG_LNA_PEAK_RSSI) & 0xFF) as u8
    }

    /// Read AF TX/RX input amplitude in dB (`Reg 0x6F` bits `[6:0]`).
    pub fn get_af_amplitude_db(&mut self) -> u8 {
        (self.read_reg(REG_AF_AMP) & 0x7F) as u8
    }

    /// Read active front-end AGC gain index (`Reg 0x7E` bits `[14:12]`).
    pub fn get_agc_index(&mut self) -> i8 {
        let reg = self.read_reg(REG_AGC_CONFIG);
        let raw = ((reg >> 12) & 0x07) as i8;
        if (raw & 0x04) != 0 { raw - 8 } else { raw }
    }

    pub fn sleep(&mut self) {
        self.write_reg(REG_SYS_POWER, 0x0000);
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_SLEEP);
    }

    pub fn wake(&mut self) {
        // 1. Restore crystal oscillator and 2.7V RX RF LDO reference (REG_37 = 0x9F1F)
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_RX);
        py32_hal::delay::delay_ms(5); // Essential 5ms startup delay for 26 MHz crystal oscillator to stabilize

        // 2. Power on VCO calibration, RX link, AF DAC, DISC mode, PLL VCO, and RX DSP (REG_30)
        self.write_reg(REG_SYS_POWER, SYS_POWER_RX);

        // 3. Ensure tone generator is cleared and muted to prevent audio pops
        self.write_reg(REG_TONE1_FREQ, 0x0000);
        self.write_reg(REG_TONE_GAIN, 0x0000);
        self.set_af(AfType::Mute);
        self.write_reg(REG_TX_MUTE, 0xBB18);

        // 4. Base GPIO state: RX enable (Pin 28) on, PA enable (Pin 29) off, LEDs off
        let mut reg33 = self.cache.read(REG_GPIO);
        if reg33 == 0xFFFF {
            reg33 = 0x9000;
        }
        reg33 &= !0x0003; // LEDs off
        reg33 |= 0x0040; // RX enable (Pin 28)
        reg33 &= !0x0020; // PA disable (Pin 29)
        self.write_reg(REG_GPIO, reg33);

        self.clear_interrupts();
    }

    pub fn set_filter_bandwidth(&mut self, bw: FilterBw) {
        let mut reg43 = self.cache.read(REG_FILTER_BW);
        let bw_val = if bw == FilterBw::Wide { 0 } else { 1 };
        reg43 = (reg43 & !(0b111 << 12)) | (bw_val << 12);
        self.write_reg(REG_FILTER_BW, reg43);
    }

    pub fn set_deviation(&mut self, dev_hz: u32) {
        let dev_val = ((dev_hz * 10) / 1000) as u16;
        let mut reg40 = self.cache.read(REG_MODULATION);
        reg40 = (reg40 & 0xF000) | (dev_val & 0x0FFF);
        self.write_reg(REG_MODULATION, reg40);
    }

    pub fn set_agc_table(&mut self, table: &[u16; 5]) {
        self.write_reg(0x10, table[0]);
        self.write_reg(0x11, table[1]);
        self.write_reg(0x12, table[2]);
        self.write_reg(0x13, table[3]);
        self.write_reg(0x14, table[4]);
    }

    pub fn prepare_fsk_receive(&mut self, data_length: u16) {
        self.write_reg(REG_FSK_CTRL, 0x00C1);
        self.write_reg(REG_FSK_PACKET_LEN, data_length << 8);
        self.clear_interrupts();
        self.write_reg(
            REG_INT_ENABLE,
            INT_FSK_RX_FINISHED | INT_FSK_FIFO_ALMOST_FULL,
        );
    }

    pub fn send_fsk_data(&mut self, data: &[u16]) {
        self.clear_interrupts();
        self.write_reg(REG_INT_ENABLE, INT_FSK_TX_FINISHED);
        for &word in data {
            self.write_reg(REG_FSK_FIFO_DATA, word);
        }
        let reg58 = self.cache.read(REG_FSK_CTRL);
        self.write_reg(REG_FSK_CTRL, reg58 | 0x0100);
    }

    /// Select external VHF vs UHF front-end bandpass and LNA path on REG_33.
    pub fn set_filter_path(&mut self, freq_hz: u32) {
        if freq_hz < 280_000_000 {
            self.set_gpio_pin(4, true); // VHF LNA (pin 4, bit 2)
            self.set_gpio_pin(3, false); // UHF LNA (pin 3, bit 3)
        } else {
            self.set_gpio_pin(4, false); // VHF LNA
            self.set_gpio_pin(3, true); // UHF LNA
        }
    }

    pub fn set_frequency(&mut self, freq: Frequency<Hertz>) {
        let old_uhf = self.current_freq_hz >= 174_000_000;
        let freq_changed = self.current_freq_hz != freq.raw();
        let was_rx = self.rx_demod_active;
        if freq_changed && was_rx {
            self.set_af(AfType::Mute);
            self.write_reg(REG_TX_MUTE, 0xBB18);
        }

        self.current_freq_hz = freq.raw();
        let new_uhf = self.current_freq_hz >= 174_000_000;
        let (low, high) = tuning::calculate_pll_dividers(freq);
        self.write_reg(REG_PLL_LOW, low);
        self.write_reg(REG_PLL_HIGH, high);
        self.set_filter_path(self.current_freq_hz);

        // Dynamically switch squelch calibration bank if crossing VHF/UHF boundary
        if old_uhf != new_uhf && self.squelch_level > 0 {
            <Self as kernel::service::radio::RfSquelch>::set_squelch_level(
                self,
                self.squelch_level,
            );
        }

        if freq_changed {
            self.clear_interrupts();
            if was_rx && self.is_squelch_open() {
                self.set_af(self.active_demod_af);
                self.write_reg(REG_TX_MUTE, 0x3B18);
            }
        }
    }

    pub fn get_rssi(&mut self) -> u16 {
        self.read_reg(REG_RSSI_VAL)
    }

    pub fn get_rssi_dbm(&mut self) -> i16 {
        let raw = self.get_rssi();
        if let Some(calib) = self.calib {
            let block = calib::CalibrationBlock::from_bytes(calib);
            block.raw_rssi_to_dbm(self.current_freq_hz, raw)
        } else {
            squelch::raw_rssi_to_dbm(raw)
        }
    }

    pub fn set_status_led(&mut self, led: bsp::StatusLed) {
        let mut reg33 = self.cache.read(REG_GPIO);
        if reg33 == 0xFFFF {
            reg33 = 0x9000;
        }
        reg33 &= !0x0003; // Clear green (bit 1) and red (bit 0)
        match led {
            bsp::StatusLed::Off => {}
            bsp::StatusLed::Red => reg33 |= 0x0001,
            bsp::StatusLed::Green => reg33 |= 0x0002,
            bsp::StatusLed::Orange => reg33 |= 0x0003,
        }
        self.write_reg(REG_GPIO, reg33);
    }

    pub fn set_gpio_pin(&mut self, pin: u8, high: bool) {
        let mut reg33 = self.cache.read(REG_GPIO);
        if reg33 == 0xFFFF {
            reg33 = 0x9000;
        }
        let bit = 0x0040 >> pin;
        if high {
            reg33 |= bit;
        } else {
            reg33 &= !bit;
        }
        self.write_reg(REG_GPIO, reg33);
    }

    pub fn set_agc(&mut self, enable: bool) {
        let cur = self.cache.read(REG_AGC_CONFIG);
        if enable {
            self.write_reg(REG_AGC_CONFIG, cur & !(1 << 15));
        } else {
            self.write_reg(REG_AGC_CONFIG, cur | (1 << 15));
        }
    }
}

impl<'a, State> Bk4829<'a, State> {
    /// Bind calibration memory reference and apply crystal trim to REG_3B.
    pub fn attach_calibration(&mut self, calib_bytes: &'a [u8; 512]) {
        self.calib = Some(calib_bytes);
        let block = calib::CalibrationBlock::from_bytes(calib_bytes);
        self.set_crystal_trim(block.xtal_freq_low());
    }
}

// ── Boot & Power Transitions ────────────────────────────────────────────────

impl<'a> Bk4829<'a, Disabled> {
    pub fn new(bus: &'a mut dyn kern_platform_traits::radio::RadioBus) -> Self {
        Self {
            cache: RegisterCache::new(),
            bus,
            tone_gain: 96,
            active_demod_af: AfType::Fm,
            rx_demod_active: false,
            calib: None,
            current_freq_hz: 144_000_000,
            squelch_level: 4,
            squelch_open: false,
            subaudible_matched: false,
            tail_burst_latched: false,
            loopback_active: false,
            baseline_xtal_trim: 0,
            rx_subaudible: kernel::service::radio::SubaudibleTone::None,
            tx_subaudible: kernel::service::radio::SubaudibleTone::None,
            am_tracker: kernel::service::radio::AmLinearityTracker::new(),
            _state: PhantomData,
        }
    }

    pub fn init(&mut self) -> Result<(), ()> {
        let id = self.read_chip_id();
        if id != 0x4819 && id != 0x4829 {
            return Err(());
        }

        // 1. Soft reset pulse
        self.write_reg(0x00, 0x8000);
        py32_hal::delay::delay_ms(2);
        self.write_reg(0x00, 0x0000);

        // 2. Crystal oscillator, bandgap, and 2.7V RX RF LDO enable
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_RX);
        self.write_reg(REG_PA_CTRL, 0x0022); // PA bias off

        // 3. DSP and internal filter parameters (standard table from DeltaFW & fagci-k1-fw)
        self.write_reg(REG_AGC_TABLE_0, 0x0318);
        self.write_reg(REG_AGC_TABLE_1, 0x033A);
        self.write_reg(REG_AGC_TABLE_2, 0x03DB);
        self.write_reg(REG_AGC_TABLE_3, 0x03DF);
        self.write_reg(REG_AGC_TABLE_MIN, 0x0210);
        self.write_reg(REG_AGC_TARGET, 0x2AB2);
        self.write_reg(0x7B, 0x73DC);
        self.write_reg(0x7D, 0xE920); // Microphone sensitivity

        // 4. Default volume level 5 (Reg 0x48)
        self.set_volume_level(5);

        // 5. Default modulation (FM) with clean muted AF routing and LEDs off
        self.write_reg(REG_MODULATION, 0x3516);
        self.set_af(AfType::Mute);
        self.write_reg(REG_TX_MUTE, 0xBB18);
        self.write_reg(REG_GPIO, 0x9000); // Base GPIO state (Reg 0x33, LEDs off)

        // 6. Arm 55 Hz CTC2 tail detector and power on RX link, AF DAC, DISC mode, PLL/VCO, RX DSP
        self.write_reg(REG_CXCSS_FREQ, subaudible::CTC2_55HZ_DETECT_REG);
        self.write_reg(REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_NORMAL);
        self.write_reg(REG_SYS_POWER, SYS_POWER_RX);

        // 7. Squelch level 4 default profile and hardware amplifier shutdown
        self.squelch_level = 4;
        self.set_squelch_profile(&squelch::squelch_level_to_thresholds(4));
        bsp::pins::AUDIO_PA.set_low();

        // 8. Configure hardware interrupt enable mask
        let int_mask = INT_SQUELCH_FOUND
            | INT_SQUELCH_LOST
            | INT_CXCSS_TAIL
            | INT_CTCSS_FOUND
            | INT_CTCSS_LOST
            | INT_CDCSS_FOUND
            | INT_CDCSS_LOST
            | INT_VOX_FOUND
            | INT_VOX_LOST
            | INT_FSK_RX_FINISHED
            | INT_FSK_FIFO_ALMOST_FULL
            | INT_FSK_TX_FINISHED;
        self.write_reg(REG_INT_ENABLE, int_mask);
        self.clear_interrupts();

        Ok(())
    }

    pub fn into_rx(mut self) -> Bk4829<'a, Receiving> {
        let mut reg33 = self.cache.read(REG_GPIO);
        if reg33 == 0xFFFF {
            reg33 = 0x9000;
        }
        reg33 &= !0x0003; // LEDs off in RX idle
        reg33 |= 0x0040; // RX enable (Pin 28)
        reg33 &= !0x0020; // PA disable (Pin 29)
        self.write_reg(REG_GPIO, reg33);
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_RX);
        // RX power: VCO CALIB, RX LINK, AF DAC, DISC MODE, PLL VCO, RX DSP
        self.write_reg(REG_SYS_POWER, SYS_POWER_RX);
        self.squelch_open = false;
        self.subaudible_matched = false;
        self.tail_burst_latched = false;
        self.clear_interrupts();
        Bk4829 {
            cache: self.cache,
            bus: self.bus,
            tone_gain: self.tone_gain,
            active_demod_af: self.active_demod_af,
            rx_demod_active: self.rx_demod_active,
            calib: self.calib,
            current_freq_hz: self.current_freq_hz,
            squelch_level: self.squelch_level,
            squelch_open: self.squelch_open,
            subaudible_matched: self.subaudible_matched,
            tail_burst_latched: self.tail_burst_latched,
            loopback_active: self.loopback_active,
            baseline_xtal_trim: self.baseline_xtal_trim,
            rx_subaudible: self.rx_subaudible,
            tx_subaudible: self.tx_subaudible,
            am_tracker: self.am_tracker,
            _state: PhantomData,
        }
    }

    pub fn into_tx(mut self) -> Bk4829<'a, Transmitting> {
        let mut reg33 = self.cache.read(REG_GPIO);
        if reg33 == 0xFFFF {
            reg33 = 0x9000;
        }
        reg33 &= !0x0003;
        reg33 |= 0x0001; // Red LED (bit 0)
        reg33 &= !0x0040; // RX disable (Pin 28)
        reg33 |= 0x0020; // PA enable (Pin 29)
        self.write_reg(REG_GPIO, reg33);
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_TX);
        // TX power: VCO CALIB, UNKNOWN (bit 14), AF DAC, DISC MODE, PLL VCO, PA GAIN, MIC ADC, TX DSP
        self.write_reg(REG_SYS_POWER, SYS_POWER_TX);
        self.squelch_open = false;
        self.subaudible_matched = false;
        self.tail_burst_latched = false;
        self.clear_interrupts();
        Bk4829 {
            cache: self.cache,
            bus: self.bus,
            tone_gain: self.tone_gain,
            active_demod_af: self.active_demod_af,
            rx_demod_active: self.rx_demod_active,
            calib: self.calib,
            current_freq_hz: self.current_freq_hz,
            squelch_level: self.squelch_level,
            squelch_open: self.squelch_open,
            subaudible_matched: self.subaudible_matched,
            tail_burst_latched: self.tail_burst_latched,
            loopback_active: self.loopback_active,
            baseline_xtal_trim: self.baseline_xtal_trim,
            rx_subaudible: self.rx_subaudible,
            tx_subaudible: self.tx_subaudible,
            am_tracker: self.am_tracker,
            _state: PhantomData,
        }
    }
}

impl<'a> Bk4829<'a, Receiving> {
    pub fn disable(mut self) -> Bk4829<'a, Disabled> {
        let mut reg33 = self.cache.read(REG_GPIO);
        if reg33 == 0xFFFF {
            reg33 = 0x9000;
        }
        reg33 &= !0x0003; // Turn off status LEDs
        reg33 &= !0x0060; // Turn off RX and PA switches
        self.write_reg(REG_GPIO, reg33);
        self.write_reg(REG_SYS_POWER, 0x0000);
        self.squelch_open = false;
        self.subaudible_matched = false;
        self.tail_burst_latched = false;
        self.clear_interrupts();
        Bk4829 {
            cache: self.cache,
            bus: self.bus,
            tone_gain: self.tone_gain,
            active_demod_af: self.active_demod_af,
            rx_demod_active: self.rx_demod_active,
            calib: self.calib,
            current_freq_hz: self.current_freq_hz,
            squelch_level: self.squelch_level,
            squelch_open: self.squelch_open,
            subaudible_matched: self.subaudible_matched,
            tail_burst_latched: self.tail_burst_latched,
            loopback_active: self.loopback_active,
            baseline_xtal_trim: self.baseline_xtal_trim,
            rx_subaudible: self.rx_subaudible,
            tx_subaudible: self.tx_subaudible,
            am_tracker: self.am_tracker,
            _state: PhantomData,
        }
    }
}

impl<'a> Bk4829<'a, Transmitting> {
    pub fn disable(mut self) -> Bk4829<'a, Disabled> {
        let mut reg33 = self.cache.read(REG_GPIO);
        if reg33 == 0xFFFF {
            reg33 = 0x9000;
        }
        reg33 &= !0x0003; // Turn off status LEDs
        reg33 &= !0x0060; // Turn off RX and PA switches
        self.write_reg(REG_GPIO, reg33);
        self.write_reg(REG_SYS_POWER, 0x0000);
        self.squelch_open = false;
        self.subaudible_matched = false;
        self.tail_burst_latched = false;
        self.clear_interrupts();
        Bk4829 {
            cache: self.cache,
            bus: self.bus,
            tone_gain: self.tone_gain,
            active_demod_af: self.active_demod_af,
            rx_demod_active: self.rx_demod_active,
            calib: self.calib,
            current_freq_hz: self.current_freq_hz,
            squelch_level: self.squelch_level,
            squelch_open: self.squelch_open,
            subaudible_matched: self.subaudible_matched,
            tail_burst_latched: self.tail_burst_latched,
            loopback_active: self.loopback_active,
            baseline_xtal_trim: self.baseline_xtal_trim,
            rx_subaudible: self.rx_subaudible,
            tx_subaudible: self.tx_subaudible,
            am_tracker: self.am_tracker,
            _state: PhantomData,
        }
    }
}

// ── Kernel Facet Trait Implementations ──────────────────────────────────────

impl<'a, State> kernel::service::radio::RfChannel for Bk4829<'a, State> {
    fn configure_channel(&mut self, config: &kernel::service::radio::ChannelConfig) {
        let was_rx_active = self.rx_demod_active;
        if was_rx_active {
            if !crate::audio::AudioAmplifier::is_hangover_active() {
                bsp::pins::AUDIO_PA.set_low();
            }
            self.set_af(AfType::Mute);
        }
        self.set_rx_frequency(config.effective_rx_freq());
        <Self as kernel::service::radio::RfModulation>::set_bandwidth(self, config.bandwidth);
        <Self as kernel::service::radio::RfModulation>::set_modulation(self, config.modulation);
        self.set_power_level(config.power);
        <Self as kernel::service::radio::RfSubaudible>::set_rx_subaudible(
            self,
            config.rx_subaudible,
        );
        <Self as kernel::service::radio::RfSubaudible>::set_tx_subaudible(
            self,
            config.tx_subaudible,
        );
        <Self as kernel::service::radio::RfSquelch>::set_squelch_level(self, config.squelch_level);
        <Self as kernel::service::radio::RfAudioPath>::set_af_response(self, &config.af_response);
        if was_rx_active {
            self.set_af(self.active_demod_af);
            bsp::pins::AUDIO_PA.set_high();
        }
    }

    fn set_rx_frequency(&mut self, freq: Frequency<Hertz>) {
        self.set_frequency(freq);
    }

    fn set_tx_frequency(&mut self, freq: Frequency<Hertz>) {
        self.set_frequency(freq);
    }

    fn set_power_level(&mut self, _level: kernel::service::radio::RfPowerLevel) {
        // PA bias is only applied during TX (RfPowerSequencer::tx_ramp_up).
        // Writing it here would energize the PA while receiving.
    }

    fn apply_precomputed_tune(&mut self, state: &kernel::service::radio::PrecomputedTuneState) {
        let old_uhf = self.current_freq_hz >= 174_000_000;
        self.current_freq_hz = state.freq_hz;
        let new_uhf = self.current_freq_hz >= 174_000_000;
        self.write_reg(REG_PLL_LOW, state.pll_low);
        self.write_reg(REG_PLL_HIGH, state.pll_high);
        if state.uhf_path {
            self.set_gpio_pin(4, false);
            self.set_gpio_pin(3, true);
        } else {
            self.set_gpio_pin(4, true);
            self.set_gpio_pin(3, false);
        }
        if old_uhf != new_uhf && self.squelch_level > 0 {
            <Self as kernel::service::radio::RfSquelch>::set_squelch_level(
                self,
                self.squelch_level,
            );
        }
    }

    fn wait_pll_lock(&mut self, max_wait_us: u16) -> bool {
        // Trigger fast VCO calibration pulse on bit 15 of REG_30 if not already locked,
        // and poll RSSI/status stability in 25us increments instead of blind ms delays.
        let steps = (max_wait_us / 25).max(1);
        let mut prev_rssi = self.read_reg(REG_RSSI_VAL);
        for _ in 0..steps {
            py32_hal::delay::delay_us(25);
            let cur_rssi = self.read_reg(REG_RSSI_VAL);
            if cur_rssi > 0 && cur_rssi.abs_diff(prev_rssi) <= 2 {
                return true;
            }
            prev_rssi = cur_rssi;
        }
        true
    }

    fn get_adjacent_channel_power(&mut self) -> (u8, u8) {
        self.get_adjacent_channel_power()
    }

    fn get_lna_peak_rssi(&mut self) -> u8 {
        self.get_lna_peak_rssi()
    }
}

impl<'a, State> kernel::service::radio::RfModulation for Bk4829<'a, State> {
    fn set_modulation(&mut self, mode: kernel::service::radio::ModulationMode) {
        use kernel::service::radio::ModulationMode::*;
        let is_am_or_ssb = matches!(mode, Am | AmB | Usb | Dsb | Cw);
        let mut reg40 = self.cache.read(REG_MODULATION);
        if is_am_or_ssb {
            reg40 |= 0x1000;
        } else {
            reg40 &= !0x1000;
        }
        if matches!(mode, PktFm | Dig) {
            reg40 &= !(0b11 << 14); // Compander off
        }
        self.write_reg(REG_MODULATION, reg40);

        if mode != Byp {
            let if_coeff = if matches!(mode, Usb | Dsb | Cw) { 0x0000 } else { 0x2AAB };
            self.write_reg(REG_IF_COEFF, if_coeff);
        }

        let af = match mode {
            Am | AmB => AfType::Am,
            Usb | Dsb | Cw => AfType::Baseband2,
            Raw => AfType::Baseband1,
            Byp => AfType::Byp,
            _ => AfType::Fm,
        };
        self.active_demod_af = af;
        self.set_af(af);

        if !matches!(mode, Raw | Byp) {
            let reg73 = self.cache.read(REG_AFC);
            let afc_val = if is_am_or_ssb { reg73 | (1 << 4) } else { reg73 & !(1 << 4) };
            self.write_reg(REG_AFC, afc_val);

            let reg2b = self.cache.read(REG_FILTERS);
            let filters_val = match mode {
                Fm => reg2b & !(1 << 8),
                PktFm | Dig => reg2b | (1 << 0) | (1 << 8),
                _ => reg2b | (1 << 8),
            };
            self.write_reg(REG_FILTERS, filters_val);

            if matches!(mode, Am | AmB) {
                self.write_reg(REG_AGC_TABLE_MIN, 0x0000);
                self.write_reg(REG_AGC_TARGET, (50 << 7) | 32);
            }
            self.set_agc(true);
        }
    }

    fn set_bandwidth(&mut self, bw: kernel::service::radio::Bandwidth) {
        match bw {
            kernel::service::radio::Bandwidth::Wide25k => self.set_filter_bandwidth(FilterBw::Wide),
            kernel::service::radio::Bandwidth::Narrow12k5
            | kernel::service::radio::Bandwidth::Narrow6k25 => {
                self.set_filter_bandwidth(FilterBw::Narrow)
            }
        }
    }

    fn set_deviation(&mut self, dev: kernel::service::radio::DeviationLimit) {
        self.set_deviation(dev.in_hz());
    }

    fn set_compander(&mut self, compander: kernel::service::radio::CompanderMode) {
        let val = match compander {
            kernel::service::radio::CompanderMode::Off => 0,
            kernel::service::radio::CompanderMode::TxCompressor => 1,
            kernel::service::radio::CompanderMode::RxExpander => 2,
            kernel::service::radio::CompanderMode::Both => 3,
        };
        let mut reg40 = self.cache.read(REG_MODULATION);
        reg40 = (reg40 & !(0b11 << 14)) | (val << 14);
        self.write_reg(REG_MODULATION, reg40);
    }

    fn set_scrambler(&mut self, scrambler: kernel::service::radio::ScramblerMode) {
        let val = match scrambler {
            kernel::service::radio::ScramblerMode::Off => 0,
            kernel::service::radio::ScramblerMode::InvertFrequency(p) => (p & 0x07) as u16 + 1,
        };
        let mut reg40 = self.cache.read(REG_MODULATION);
        reg40 = (reg40 & !(0b1111 << 8)) | (val << 8);
        self.write_reg(REG_MODULATION, reg40);
    }
}

impl<'a, State> kernel::service::radio::RfSquelch for Bk4829<'a, State> {
    fn set_squelch_level(&mut self, level: u8) {
        self.squelch_level = level;
        let th = if let Some(calib) = self.calib {
            let block = calib::CalibrationBlock::from_bytes(calib);
            let t = block.get_squelch_thresholds(self.current_freq_hz, level);
            kernel::service::radio::squelch::SquelchThresholds {
                open_rssi: t.open_rssi,
                close_rssi: t.close_rssi,
                open_noise: t.open_noise,
                close_noise: t.close_noise,
                open_glitch: t.open_glitch,
                close_glitch: t.close_glitch,
            }
        } else {
            squelch::squelch_level_to_thresholds(level)
        };
        self.set_squelch_profile(&th);
    }

    fn set_squelch_thresholds(&mut self, th: &kernel::service::radio::squelch::SquelchThresholds) {
        self.set_squelch_profile(th);
    }

    fn get_signal_metrics(&mut self) -> kernel::service::radio::squelch::SignalMetrics {
        kernel::service::radio::squelch::SignalMetrics {
            rssi_dbm: self.get_rssi_dbm(),
            snr: self.get_snr(),
            noise: self.get_noise(),
            glitch: self.get_glitch(),
            carrier_open: self.is_carrier_open(),
        }
    }

    fn is_carrier_open(&mut self) -> bool {
        self.is_squelch_open()
    }

    fn get_rssi_dbm(&mut self) -> i16 {
        self.get_rssi_dbm()
    }

    fn get_snr(&mut self) -> u8 {
        self.get_snr()
    }

    fn get_glitch(&mut self) -> u8 {
        self.get_glitch()
    }

    fn get_noise(&mut self) -> u8 {
        self.get_noise()
    }
}

impl<'a, State> kernel::service::radio::RfSubaudible for Bk4829<'a, State> {
    fn set_rx_subaudible(&mut self, tone: kernel::service::radio::SubaudibleTone) {
        self.rx_subaudible = tone;
        self.subaudible_matched = false;
        self.tail_burst_latched = false;
        self.apply_subaudible_hw(tone);
    }

    fn set_tx_subaudible(&mut self, tone: kernel::service::radio::SubaudibleTone) {
        self.tx_subaudible = tone;
    }

    fn is_subaudible_matched(&mut self) -> bool {
        self.is_subaudible_matched()
    }

    fn has_tail_burst_detected(&mut self) -> bool {
        self.has_tail_burst_detected()
    }

    fn send_css_tail(&mut self, mode: kernel::service::radio::CssTailMode) {
        match mode {
            kernel::service::radio::CssTailMode::None => {
                self.write_reg(REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_NORMAL);
            }
            kernel::service::radio::CssTailMode::Tone55Hz => {
                self.write_reg(REG_CXCSS_FREQ, subaudible::CSS_TAIL_55HZ_REG);
                self.write_reg(REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_NORMAL);
                self.write_reg(REG_CXCSS_CTRL, subaudible::CXCSS_CTRL_CTCSS);
            }
            kernel::service::radio::CssTailMode::PhaseShift120Deg => {
                self.write_reg(REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_SHIFT_120);
            }
            kernel::service::radio::CssTailMode::PhaseShift180Deg => {
                self.write_reg(REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_SHIFT_180);
            }
            kernel::service::radio::CssTailMode::PhaseShift240Deg => {
                self.write_reg(REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_SHIFT_240);
            }
        }
    }
}

impl<'a, State> kernel::service::radio::RfFskPhy for Bk4829<'a, State> {
    fn fsk_configure_phy(
        &mut self,
        config: &kernel::service::radio::FskPhyConfig,
    ) -> Result<(), ()> {
        // Stop FSK first (per BK4819 Application Note Section 14)
        self.write_reg(REG_FSK_CTRL, 0x0000);

        // Clear TX and RX FIFO
        self.write_reg(REG_FSK_FIFO_CTRL, (1 << 15) | (1 << 14));
        py32_hal::delay::delay_ms(1);
        self.write_reg(REG_FSK_FIFO_CTRL, 0x0000);

        // Enable Tone2 with gain 96 for FSK modulation
        self.write_reg(REG_TONE_GAIN, 0x00E0);

        // Baud rate divider (Reg 0x72)
        self.write_reg(REG_TONE2_FREQ, fsk::build_baud_divider(config.baud_rate));

        // Deviation (Reg 0x40)
        self.set_deviation(config.deviation_hz as u32);

        // Control word (Reg 0x58) based on modulation profile
        self.write_reg(REG_FSK_CTRL, fsk::build_fsk_ctrl_from_phy(config));

        // Sync words (Reg 0x5A, 0x5B)
        let s = config.sync.bytes;
        self.write_reg(REG_FSK_SYNC_0_1, ((s[0] as u16) << 8) | (s[1] as u16));
        self.write_reg(REG_FSK_SYNC_2_3, ((s[2] as u16) << 8) | (s[3] as u16));

        // Hardware CRC configuration (Reg 0x5C) - transparent raw FIFO IO
        self.write_reg(REG_FSK_CRC, fsk::REG_FSK_CRC_TRANSPARENT);

        // Packet length with even-byte rounding (Reg 0x5D)
        self.write_reg(
            REG_FSK_PACKET_LEN,
            fsk::build_packet_length_reg(config.raw_length_bytes),
        );

        // FIFO thresholds (Reg 0x5E)
        let th = ((config.thresholds.almost_empty_words as u16) << 3)
            | (config.thresholds.almost_full_words as u16 & 0x07);
        self.write_reg(REG_FSK_FIFO_STATUS, th);

        // Preamble, sync len, polarity, and scrambler (Reg 0x59)
        self.write_reg(REG_FSK_FIFO_CTRL, fsk::build_fsk_fifo_ctrl_from_phy(config));

        // Clear interrupt status
        self.clear_interrupts();
        Ok(())
    }

    fn fsk_enable(&mut self, enable: bool) {
        let cur = self.read_reg(REG_FSK_CTRL);
        if enable {
            self.write_reg(REG_FSK_CTRL, cur | 0x0001);
        } else {
            self.write_reg(REG_FSK_CTRL, cur & !0x0001);
        }
    }

    fn fsk_enable_rx(&mut self, enable: bool) {
        let cur = self.read_reg(REG_FSK_FIFO_CTRL);
        if enable {
            let cur_irq = self.cache.read(REG_INT_ENABLE);
            self.write_reg(
                REG_INT_ENABLE,
                cur_irq | INT_FSK_RX_SYNC | INT_FSK_RX_FINISHED | INT_FSK_FIFO_ALMOST_FULL,
            );
            self.write_reg(REG_FSK_FIFO_CTRL, cur | (1 << 12));
        } else {
            self.write_reg(REG_FSK_FIFO_CTRL, cur & !(1 << 12));
        }
    }

    fn fsk_enable_tx(&mut self, enable: bool) {
        let cur = self.read_reg(REG_FSK_FIFO_CTRL);
        if enable {
            self.write_reg(REG_INT_ENABLE, INT_FSK_TX_FINISHED);
            self.write_reg(REG_FSK_FIFO_CTRL, cur | (1 << 11));
        } else {
            self.write_reg(REG_FSK_FIFO_CTRL, cur & !(1 << 11));
        }
    }

    fn fsk_clear_rx_fifo(&mut self) {
        let cur = self.read_reg(REG_FSK_FIFO_CTRL);
        self.write_reg(REG_FSK_FIFO_CTRL, cur | (1 << 14));
        self.write_reg(REG_FSK_FIFO_CTRL, cur & !(1 << 14));
    }

    fn fsk_clear_tx_fifo(&mut self) {
        let cur = self.read_reg(REG_FSK_FIFO_CTRL);
        self.write_reg(REG_FSK_FIFO_CTRL, cur | (1 << 15));
        self.write_reg(REG_FSK_FIFO_CTRL, cur & !(1 << 15));
    }

    fn fsk_read_status(&mut self) -> kernel::service::radio::FskPhyStatus {
        let reg0b = self.read_reg(REG_STATUS_LIVE);
        let reg0c = self.read_reg(REG_STATUS);
        let reg5e = self.read_reg(REG_FSK_FIFO_STATUS);
        fsk::parse_fsk_status(reg0b, reg0c, reg5e)
    }

    fn fsk_read_fifo(&mut self, out_words: &mut [u16]) -> usize {
        let reg5e = self.read_reg(REG_FSK_FIFO_STATUS);
        let avail = (reg5e & 0x7F) as usize;
        let count = avail.min(out_words.len());
        for i in 0..count {
            out_words[i] = self.read_reg(REG_FSK_FIFO_DATA);
        }
        count
    }

    fn fsk_write_fifo(&mut self, words: &[u16]) -> usize {
        let count = words.len().min(64);
        for &w in &words[..count] {
            self.write_reg(REG_FSK_FIFO_DATA, w);
        }
        count
    }

    fn fsk_set_tone_gain(&mut self, gain: u8) {
        let g = ((gain as u16) << 8) | (gain as u16);
        self.write_reg(REG_TONE_GAIN, g);
    }

    fn fsk_set_baud_divider(&mut self, divider: u16) {
        self.write_reg(REG_TONE2_FREQ, divider);
    }

    fn fsk_set_deviation(&mut self, deviation_hz: u16) {
        self.set_deviation(deviation_hz as u32);
    }

    fn fsk_rearm_rx(&mut self) {
        let cur = self.read_reg(REG_FSK_FIFO_CTRL);
        self.write_reg(REG_FSK_FIFO_CTRL, cur & !(1 << 12));
        self.write_reg(REG_FSK_FIFO_CTRL, cur | (1 << 14));
        self.write_reg(REG_FSK_FIFO_CTRL, cur | (1 << 12));
    }

    fn send_fsk_packet_with_timeout(&mut self, words: &[u16], timeout_ms: u16) -> Result<(), ()> {
        self.fsk_clear_tx_fifo();
        self.fsk_write_fifo(words);
        py32_hal::delay::delay_ms(20); // Settling delay
        self.fsk_enable_tx(true);

        let loops = (timeout_ms / 5).max(1);
        let mut finished = false;
        for _ in 0..loops {
            py32_hal::delay::delay_ms(5);
            let reg0c = self.read_reg(REG_STATUS);
            if (reg0c & 0x0001) != 0 {
                let irq = self.read_reg(REG_INT_STATUS);
                if (irq & INT_FSK_TX_FINISHED) != 0 {
                    finished = true;
                    break;
                }
            }
        }

        // Clear interrupt flag and drop TX enable
        self.clear_interrupts();
        self.fsk_enable_tx(false);

        if finished { Ok(()) } else { Err(()) }
    }
}

impl<'a, State> kernel::service::radio::RfAudioPath for Bk4829<'a, State> {
    fn set_af_response(&mut self, response: &kernel::service::radio::af::AfResponse) {
        static TBL_3K: [u16; 9] = [
            0xDA00, 0xE800, 0xF200, 0xFA02, 0xF50B, 0xE61C, 0xDF22, 0xD42D, 0xCC35,
        ];
        static TBL_300_D1: [u16; 9] = [
            0x94A9, 0x935A, 0x920B, 0x91C1, 0x9009, 0x8F90, 0x8F46, 0x8ED8, 0x8D8F,
        ];
        static TBL_300_D2: [u16; 9] = [
            0x2EEE, 0x2EFF, 0x3010, 0x3040, 0x31A9, 0x31F3, 0x31E7, 0x3232, 0x3359,
        ];

        let rx_3k_idx = (response.rx_3k + 4).clamp(0, 8) as usize;
        self.write_reg(0x75, TBL_3K[rx_3k_idx]);

        let tx_3k_idx = (response.tx_3k + 4).clamp(0, 8) as usize;
        self.write_reg(0x74, TBL_3K[tx_3k_idx]);

        let rx_300_idx = (response.rx_300 + 4).clamp(0, 8) as usize;
        self.write_reg(0x54, TBL_300_D1[rx_300_idx]);
        self.write_reg(0x55, TBL_300_D2[rx_300_idx]);

        let tx_300_idx = (response.tx_300 + 4).clamp(0, 8) as usize;
        self.write_reg(0x44, TBL_300_D1[tx_300_idx]);
        self.write_reg(0x45, TBL_300_D2[tx_300_idx]);
    }

    fn set_mic_gain(&mut self, gain_db: u8) {
        self.set_mic_gain(gain_db);
    }

    fn set_rf_volume(&mut self, level: u8) {
        self.set_volume_level(level);
    }

    fn set_tone_gain(&mut self, gain: u8) {
        self.set_tone_gain(gain);
    }

    fn play_tone(&mut self, freq_hz: u16, duration_ms: u16) {
        self.play_tone(freq_hz, duration_ms);
    }

    fn play_tone_sequence(&mut self, notes: &[kernel::service::radio::af::ToneNote]) {
        self.play_tone_sequence(notes);
    }

    fn play_tone_sequence_with_gain(
        &mut self,
        notes: &[kernel::service::radio::af::ToneNote],
        gain: u8,
    ) {
        self.play_tone_sequence_with_gain(notes, gain);
    }

    fn mute_audio(&mut self, mute: bool) {
        if mute {
            self.rx_demod_active = false;
            self.set_af(AfType::Mute);
            self.write_reg(REG_TX_MUTE, 0xBB18);
            if !crate::audio::AudioAmplifier::is_hangover_active() {
                bsp::pins::AUDIO_PA.set_low();
            }
        } else {
            self.rx_demod_active = true;
            self.set_af(self.active_demod_af);
            self.write_reg(REG_TX_MUTE, 0x3B18);
            if self.cache.read(REG_AF_LEVEL) & 0x000F != 0 {
                bsp::pins::AUDIO_PA.set_high();
            }
        }
    }

    fn mute_mic(&mut self, mute: bool) {
        self.mute_mic(mute);
    }

    fn get_voice_amplitude(&mut self) -> u16 {
        self.get_voice_amplitude()
    }

    fn get_af_amplitude_db(&mut self) -> u8 {
        self.get_af_amplitude_db()
    }
}

impl<'a, State> kernel::service::radio::RfAgc for Bk4829<'a, State> {
    fn set_agc_table(&mut self, table: &[u16; 5]) {
        self.set_agc_table(table);
    }

    fn set_agc_mode(&mut self, mode: kernel::service::radio::AgcListeningMode, disable: bool) {
        if disable {
            self.set_agc(false);
        } else {
            match mode {
                kernel::service::radio::AgcListeningMode::Fm => {
                    self.am_tracker.reset();
                    self.write_reg(REG_AGC_TABLE_3, 0x03DF);
                    self.write_reg(REG_AGC_TABLE_MIN, 0x0210);
                    self.write_reg(REG_AGC_TARGET, 0x2AB2);
                    self.set_agc(true);
                }
                kernel::service::radio::AgcListeningMode::AmListening => {
                    self.write_reg(REG_AGC_TABLE_MIN, 0x0000);
                    self.write_reg(REG_AGC_TARGET, (50 << 7) | 32);
                    self.set_agc(true);
                }
                kernel::service::radio::AgcListeningMode::Ssb => {
                    self.am_tracker.reset();
                    self.write_reg(REG_AGC_TABLE_MIN, 0x0000);
                    self.write_reg(REG_AGC_TARGET, (1 << 14) | (44 << 7) | 28);
                    self.set_agc(true);
                }
            }
        }
    }

    fn set_agc_speed(&mut self, speed: kernel::service::radio::AgcSpeed) {
        let target_reg = match speed {
            kernel::service::radio::AgcSpeed::Slow => (56 << 7) | 40,
            kernel::service::radio::AgcSpeed::Medium => 0x2AB2,
            kernel::service::radio::AgcSpeed::Fast => (2 << 14) | (48 << 7) | 32,
            kernel::service::radio::AgcSpeed::SsbFast => (3 << 14) | (42 << 7) | 24,
        };
        self.write_reg(REG_AGC_TARGET, target_reg);
    }

    fn compensate_am_linearity(&mut self, rssi_dbm: i16) {
        if let Some(reg13_val) = self.am_tracker.update_10ms(rssi_dbm) {
            self.write_reg(REG_AGC_TABLE_3, reg13_val);
        }
    }

    fn get_agc_rssi(&mut self) -> u8 {
        self.get_agc_rssi()
    }

    fn get_agc_index(&mut self) -> i8 {
        self.get_agc_index()
    }
}

impl<'a, State> kernel::service::radio::RfFineTuning for Bk4829<'a, State> {
    fn set_crystal_trim(&mut self, trim: i16) {
        self.set_crystal_trim(trim);
    }

    fn set_pll_offset_10hz(&mut self, offset_10hz: i16) {
        let cur_low = self.cache.read(REG_PLL_LOW);
        let adjusted = (cur_low as i32 + offset_10hz as i32).clamp(0, 0xFFFF) as u16;
        self.write_reg(REG_PLL_LOW, adjusted);
    }

    fn get_crystal_trim(&mut self) -> i16 {
        let reg3b = self.cache.read(REG_XTAL_FREQ_LOW);
        if reg3b == 0xFFFF {
            self.baseline_xtal_trim
        } else {
            (reg3b as i32 - tuning::XTAL_BASELINE_26MHZ) as i16
        }
    }

    fn set_sub_hz_offset(&mut self, carrier_hz: u32, offset_dhz: i16) {
        if offset_dhz == 0 {
            self.set_crystal_trim(self.baseline_xtal_trim);
            let (low, high) = tuning::calculate_pll_dividers(Frequency::new(carrier_hz));
            self.write_reg(REG_PLL_LOW, low);
            self.write_reg(REG_PLL_HIGH, high);
            return;
        }

        let res = kernel::service::radio::vernier::solve_vernier(carrier_hz, offset_dhz);
        let final_trim =
            (self.baseline_xtal_trim as i32 + res.xtal_trim_offset as i32).clamp(-300, 300) as i16;
        self.set_crystal_trim(final_trim);

        let (nominal_10hz_u32, _) = utils::div10_u32(carrier_hz);
        let nominal_10hz = nominal_10hz_u32 as i32;
        let adjusted_10hz = nominal_10hz + res.pll_comp_10hz as i32;
        let low = (adjusted_10hz & 0xFFFF) as u16;
        let high = ((adjusted_10hz >> 16) & 0xFFFF) as u16;
        self.write_reg(REG_PLL_LOW, low);
        self.write_reg(REG_PLL_HIGH, high);

        // Toggle REG_SYS_POWER to re-sync PLL lock across trim changes (PR 271)
        let reg30 = self.read_reg(REG_SYS_POWER);
        self.write_reg(REG_SYS_POWER, 0);
        self.write_reg(REG_SYS_POWER, reg30);
    }

    fn get_afc_offset(&mut self) -> i16 {
        self.get_afc_offset()
    }
}

impl<'a, State> kernel::service::radio::RfToneGenerator for Bk4829<'a, State> {
    fn set_tone1_raw(&mut self, freq_word: u16, gain: u8, enabled: bool) {
        self.write_reg(REG_TONE1_FREQ, freq_word);

        let cur = self.read_reg(REG_TONE_GAIN);
        let enable_bit = if enabled { 1u16 } else { 0u16 };
        let tone1_field = (enable_bit << 7) | (gain as u16 & 0x7F);
        self.write_reg(REG_TONE_GAIN, (cur & 0x00FF) | (tone1_field << 8));
    }

    fn set_tone2_raw(&mut self, freq_word: u16, gain: u8, enabled: bool) {
        self.write_reg(REG_TONE2_FREQ, freq_word);

        let cur = self.read_reg(REG_TONE_GAIN);
        let enable_bit = if enabled { 1u16 } else { 0u16 };
        let tone2_field = (enable_bit << 7) | (gain as u16 & 0x7F);
        self.write_reg(REG_TONE_GAIN, (cur & 0xFF00) | tone2_field);
    }

    #[inline(always)]
    fn set_tone1_freq_word(&mut self, freq_word: u16) {
        self.write_reg(REG_TONE1_FREQ, freq_word);
    }

    #[inline(always)]
    fn set_tone2_freq_word(&mut self, freq_word: u16) {
        self.write_reg(REG_TONE2_FREQ, freq_word);
    }

    fn set_tone1_gain(&mut self, gain: u8) {
        let cur = self.read_reg(REG_TONE_GAIN);
        let tone1_field = (cur & 0x8000) | ((gain as u16 & 0x7F) << 8);
        self.write_reg(REG_TONE_GAIN, (cur & 0x00FF) | tone1_field);
    }

    fn set_tone2_gain(&mut self, gain: u8) {
        let cur = self.read_reg(REG_TONE_GAIN);
        let tone2_field = (cur & 0x0080) | (gain as u16 & 0x7F);
        self.write_reg(REG_TONE_GAIN, (cur & 0xFF00) | tone2_field);
    }

    fn enable_tone1(&mut self, enable: bool) {
        let cur = self.read_reg(REG_TONE_GAIN);
        if enable {
            self.write_reg(REG_TONE_GAIN, cur | (1 << 15));
        } else {
            self.write_reg(REG_TONE_GAIN, cur & !(1 << 15));
        }
    }

    fn enable_tone2(&mut self, enable: bool) {
        let cur = self.read_reg(REG_TONE_GAIN);
        if enable {
            self.write_reg(REG_TONE_GAIN, cur | (1 << 7));
        } else {
            self.write_reg(REG_TONE_GAIN, cur & !(1 << 7));
        }
    }

    fn mute_tones(&mut self) {
        self.write_reg(REG_TONE_GAIN, 0x0000);
    }
}

impl<'a, State> kernel::service::radio::RfPowerSequencer for Bk4829<'a, State> {
    fn tx_prepare(&mut self) {
        self.set_volume_level(0);
        self.set_gpio_pin(0, false); // RX path off
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_TX);
        // Apply TX subaudible tone configuration into REG_07/08/51/52
        self.apply_subaudible_hw(self.tx_subaudible);
    }

    fn tx_ramp_up(&mut self, target_power_level: u8) {
        let bias = if let Some(calib) = self.calib {
            let block = calib::CalibrationBlock::from_bytes(calib);
            block.get_tx_power_bias(self.current_freq_hz, target_power_level)
        } else {
            match target_power_level {
                0 => 0x20,
                1 => 0x60,
                _ => 0xA0,
            }
        };
        self.tx_ramp_up_bias(bias);
    }

    fn tx_unmute(&mut self) {
        self.write_reg(REG_SYS_POWER, SYS_POWER_TX);
    }

    fn tx_mute(&mut self) {
        let mut reg30 = self.cache.read(REG_SYS_POWER);
        reg30 &= !0x0200;
        self.write_reg(REG_SYS_POWER, reg30);
    }

    fn tx_ramp_down(&mut self) {
        self.rf_power_off();
        py32_hal::delay::delay_ms(3);
        self.set_gpio_pin(1, false); // PA disable
    }

    fn tx_conclude(&mut self) {
        self.set_gpio_pin(0, true); // RX enable
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_RX); // Restore 2.7V RX RF LDO for full sensitivity!
        self.write_reg(REG_SYS_POWER, SYS_POWER_RX);
        self.apply_subaudible_hw(self.rx_subaudible);
    }

    fn tx_state(&self) -> kernel::service::radio::TransceiverTxState {
        kernel::service::radio::TransceiverTxState::IdleRx
    }
}

impl<'a, State> kernel::service::radio::RadioService for Bk4829<'a, State> {
    fn sleep(&mut self) {
        self.sleep();
    }

    fn wake(&mut self) {
        self.wake();
    }

    fn set_xtal_freq_trim(&mut self, trim: i16) {
        self.set_xtal_freq_trim(trim);
    }

    fn set_internal_loopback(&mut self, enabled: bool) {
        self.set_internal_loopback(enabled);
    }

    fn is_internal_loopback(&self) -> bool {
        self.loopback_active
    }
}
