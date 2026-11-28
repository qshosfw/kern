//! High-assurance, zero-cost typestate driver for the BK4829 RF Transceiver.
//!
//! Provides a 128x16-bit shadow register cache to mathematically eliminate redundant
//! SPI bus transactions for high-performance DSP loops and CSMA/CA routines.

pub mod af;
pub mod agc;
pub mod fsk;
pub mod ops;
pub mod registers;
pub mod squelch;
pub mod subaudible;
pub mod tuning;

use kern_platform_traits::audio::AudioPa;
use kern_platform_traits::display::StatusLed;
use kern_platform_traits::radio::RadioBus;
use kern_platform_traits::time::Delay;
use registers::*;
use utils::units::{Frequency, Hertz};

/// Platform bus contract for the BK4829 transceiver (register bus + speaker PA gate + delay).
pub trait Bk4829Bus: RadioBus + AudioPa + Delay {}
impl<T: RadioBus + AudioPa + Delay> Bk4829Bus for T {}

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
    UltraNarrow = 2,
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

// ── BK4829 Driver ───────────────────────────────────────────────────────────

/// Table-driven register bitfield patch `(reg, clear_mask, set_mask)`.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct RegPatch {
    pub reg: u8,
    pub clear: u16,
    pub set: u16,
}

const fn build_volatile_mask() -> [u32; 4] {
    let mut m = [0u32; 4];
    const VOL: &[u8] = &[
        0x00, 0x02, 0x07, 0x08, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x32, 0x58, 0x59, 0x5E, 0x5F, 0x61,
        0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x7E,
    ];
    let mut i = 0;
    while i < VOL.len() {
        let r = VOL[i] as usize;
        m[r >> 5] |= 1u32 << (r & 31);
        i += 1;
    }
    m
}

const VOLATILE_REGS: [u32; 4] = build_volatile_mask();

#[inline(always)]
const fn is_volatile_reg(addr: u8) -> bool {
    addr >= 128 || (VOLATILE_REGS[(addr >> 5) as usize] & (1u32 << (addr & 31))) != 0
}

#[repr(C)]
pub struct Bk4829<'a, B> {
    pub(crate) tone_gain: u8,
    pub(crate) saved_tx_volume: u8,
    pub(crate) active_demod_af: AfType,
    pub(crate) rx_demod_active: bool,
    pub(crate) squelch_level: u8,
    pub(crate) squelch_open: bool,
    pub(crate) subaudible_matched: bool,
    pub(crate) tone_hold_polls: u8,
    pub(crate) tail_burst_latched: bool,
    pub(crate) loopback_active: bool,
    pub(crate) saved_mod_dev: u16,
    pub(crate) baseline_xtal_trim: i16,
    pub(crate) cur_xtal_trim: i16,
    pub(crate) current_freq_hz: u32,
    pub(crate) shadow_valid: [u32; 4],
    pub(crate) shadow: [u16; 128],
    pub(crate) bus: B,
    pub(crate) calib: Option<&'a calib::ActiveCalib>,
    pub(crate) rx_subaudible: kernel::service::radio::SubaudibleTone,
    pub(crate) tx_subaudible: kernel::service::radio::SubaudibleTone,
    pub(crate) am_tracker: kernel::service::radio::AmLinearityTracker,
}

impl<'a, B: Bk4829Bus> Bk4829<'a, B> {
    /// Clear all cached shadow register validity bits (e.g. across soft reset).
    #[inline(always)]
    pub fn invalidate_shadow(&mut self) {
        self.shadow_valid = [0; 4];
    }

    /// Reads a 16-bit BK4829 register, returning the shadow cache for non-volatile registers.
    pub fn read_reg(&mut self, addr: u8) -> u16 {
        if !is_volatile_reg(addr) {
            let idx = (addr >> 5) as usize;
            let bit = 1u32 << (addr & 31);
            if (self.shadow_valid[idx] & bit) != 0 {
                return unsafe { *self.shadow.get_unchecked(addr as usize) };
            }
            let val = self.bus.read_reg(addr);
            if val != 0xFFFF {
                unsafe { *self.shadow.get_unchecked_mut(addr as usize) = val };
                self.shadow_valid[idx] |= bit;
            }
            return val;
        }
        self.bus.read_reg(addr)
    }

    /// Writes a 16-bit BK4829 register, eliding redundant bus writes for unchanged non-volatile values.
    pub fn write_reg(&mut self, addr: u8, val: u16) {
        if !is_volatile_reg(addr) {
            let idx = (addr >> 5) as usize;
            let bit = 1u32 << (addr & 31);
            let slot = unsafe { self.shadow.get_unchecked_mut(addr as usize) };
            if (self.shadow_valid[idx] & bit) != 0 && *slot == val {
                return;
            }
            *slot = val;
            self.shadow_valid[idx] |= bit;
        }
        self.bus.write_reg(addr, val);
    }

    pub(crate) fn apply_reg_vals(&mut self, regs: &[(u8, u16)]) {
        for &(reg, val) in regs {
            self.write_reg(reg, val);
        }
    }

    pub fn apply_reg_patches(&mut self, patches: &[RegPatch]) {
        for p in patches {
            self.update_reg_bits(p.reg, p.clear, p.set);
        }
    }

    pub(crate) fn apply_demod_af(&mut self, unmuted: bool) {
        if unmuted {
            self.set_af(self.active_demod_af);
            self.write_reg(REG_TX_MUTE, 0x3B18);
        } else {
            self.set_af(AfType::Mute);
            self.write_reg(REG_TX_MUTE, 0xBB18);
        }
    }

    fn finish_freq_tune(&mut self, freq_hz: u32, uhf_path: bool, low: u16, high: u16) {
        let old_uhf = self.current_freq_hz >= 174_000_000;
        self.current_freq_hz = freq_hz;
        let new_uhf = freq_hz >= 174_000_000;
        self.write_pll_words(low, high);
        self.update_gpio_reg(0x000C, if uhf_path { 0x0008 } else { 0x0004 });
        if old_uhf != new_uhf && self.squelch_level > 0 {
            <Self as kernel::service::radio::RfSquelch>::set_squelch_level(
                self,
                self.squelch_level,
            );
        }
    }

    /// Clear `clear_mask` bits and set `set_mask` bits on a register via live RMW.
    pub(crate) fn update_reg_bits(&mut self, addr: u8, clear_mask: u16, set_mask: u16) {
        let val = (self.read_reg(addr) & !clear_mask) | set_mask;
        self.write_reg(addr, val);
    }

    fn rmw_reg_bit(&mut self, addr: u8, mask: u16, set: bool) {
        self.update_reg_bits(addr, mask, if set { mask } else { 0 });
    }

    fn pulse_fifo_bit(&mut self, mask: u16) {
        let cur = self.read_reg(REG_FSK_FIFO_CTRL);
        self.write_reg(REG_FSK_FIFO_CTRL, cur | mask);
        self.write_reg(REG_FSK_FIFO_CTRL, cur & !mask);
    }

    /// Read Chip ID from register 0x00.
    pub fn read_chip_id(&mut self) -> u16 {
        self.read_reg(REG_CHIP_ID)
    }

    /// Soft Reset: Clears RF core configuration.
    pub fn soft_reset(&mut self) {
        self.sleep();
    }

    pub fn set_crystal_trim(&mut self, trim: i16) {
        self.cur_xtal_trim = trim;
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
            let max_vol = calib.volume_gain();
            let max_dac = calib.dac_gain();
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
                calib.mic_gain_db(gain_db)
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
        self.update_reg_bits(REG_MIC_GAIN, 0x001F, gain_bits);
    }

    pub fn set_tone_gain(&mut self, gain: u8) {
        self.tone_gain = gain.min(127);
    }

    pub fn set_audio_profile(&mut self, _profile: u8) {}

    /// Current hardware tone generator gain (`0` = muted).
    #[inline(always)]
    pub fn tone_gain(&self) -> u8 {
        self.tone_gain
    }

    /// Capture the active RX power register word before isolating `REG_SYS_POWER` for tone playback.
    pub fn capture_rx_pwr(&mut self) -> u16 {
        let cur_pwr = self.read_reg(REG_SYS_POWER);
        if cur_pwr != 0xFFFF && (cur_pwr & 0x0001) != 0 {
            cur_pwr & !0x8000
        } else {
            0x3FF1
        }
    }

    /// Begin tone teardown: mute tone frequency and restore FSK/tone gain prior to the 5 ms settle window.
    pub fn prepare_tone_teardown(&mut self) {
        self.write_reg(REG_TX_MUTE, 0xBB18);
        self.write_reg(REG_TONE1_FREQ, 0x0000);
        // Preserve Tone2 / FSK subcarrier gain (bits [7:0]) if hardware FSK modem is active
        let fsk_active = {
            let fsk_ctrl = self.read_reg(REG_FSK_CTRL);
            fsk_ctrl != 0xFFFF && (fsk_ctrl & 0x0001) != 0
        };
        let restored_tone_gain = if fsk_active { 0x00E0 } else { 0x0000 };
        self.write_reg(REG_TONE_GAIN, restored_tone_gain);
        self.restore_tone_mod_dev();
    }

    /// Complete tone teardown after the 5 ms settle delay, restoring RX power and AF demod routing.
    pub fn finish_tone_teardown(&mut self, rx_pwr: u16) {
        self.write_reg(REG_SYS_POWER, rx_pwr);
        let unmuted = self.rx_demod_active && self.is_squelch_open();
        self.apply_demod_af(unmuted);
        crate::audio::AudioAmplifier::<B>::notify_tone_played();
    }

    pub fn begin_tone_sequence(&mut self) -> bool {
        self.write_reg(REG_TX_MUTE, 0xBB18);
        self.clamp_tone_mod_dev();
        self.set_af(AfType::Beep);
        let tone_cfg = 0x8000 | ((self.tone_gain as u16 & 0x7F) << 8);
        self.write_reg(REG_TONE_GAIN, tone_cfg);
        self.write_reg(REG_SYS_POWER, 0x0302);

        let was_amp_active = self.bus.is_enabled();
        if !was_amp_active {
            self.bus.enable();
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

    /// Soft-start 8-step S-curve PA ramp-up with calibrated PA bias to minimize key-up
    /// `di/dt` supply droop and USB ground-bounce transients.
    pub fn tx_ramp_up_bias(&mut self, bias: u8) {
        self.update_gpio_reg(0x0060, 0x0020); // PA enable (Pin 1), RX disable (Pin 0)
        for step in 1..=8u16 {
            self.bus.delay_ms(1);
            self.set_rf_power_bias(((bias as u16 * step) >> 3) as u8);
        }
        self.bus.delay_ms(1);
    }

    /// PA off (REG_36 reset value used by init).
    pub fn rf_power_off(&mut self) {
        self.write_reg(0x36, 0x0022);
    }

    pub fn write_pll_words(&mut self, low: u16, high: u16) {
        self.write_reg(REG_PLL_LOW, low);
        self.write_reg(REG_PLL_HIGH, high);
    }

    pub fn set_squelch_profile(&mut self, th: &kernel::service::radio::squelch::SquelchThresholds) {
        let r4d = 0xA000 | (th.close_glitch & 0x00FF);
        // BK4829 REG_4E layout (DRT01-230605-C01 p.27): [15:12] = open_delay (3), [11:8] = close_delay (8).
        let r4e = 0x3800 | (th.open_glitch & 0x00FF);
        let r4f = ((th.close_noise & 0x007F) << 8) | (th.open_noise & 0x007F);
        let r78 = ((th.open_rssi & 0x00FF) << 8) | (th.close_rssi & 0x00FF);
        self.apply_reg_vals(&[
            (REG_SQL_GLITCH_CLOSE, r4d),
            (REG_SQL_GLITCH_OPEN, r4e),
            (REG_SQL_NOISE, r4f),
            (REG_SQL_RSSI, r78),
        ]);
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
                self.subaudible_matched = false;
                self.tone_hold_polls = 0;
            }
            if (irq & (INT_CTCSS_FOUND | INT_CDCSS_FOUND)) != 0 {
                self.subaudible_matched = true;
                self.tone_hold_polls = 10;
            }
            if (irq & INT_CXCSS_TAIL) != 0 {
                self.tail_burst_latched = true;
                self.subaudible_matched = false;
                self.tone_hold_polls = 0;
            }
            if (irq & (INT_CTCSS_LOST | INT_CDCSS_LOST)) != 0 && self.tone_hold_polls == 0 {
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
        let hw_match = ((reg0c >> 10) & 0x01) != 0 || ((reg0c >> 14) & 0x03) != 0;
        if hw_match {
            self.subaudible_matched = true;
            self.tone_hold_polls = 10;
            return true;
        }
        let hw_open = self.squelch_level == 0 || ((reg0c >> 1) & 0x0001) != 0;
        if hw_open && !self.tail_burst_latched && self.tone_hold_polls > 0 {
            self.tone_hold_polls -= 1;
            self.subaudible_matched = self.tone_hold_polls > 0;
            return true;
        }
        self.tone_hold_polls = 0;
        self.subaudible_matched = false;
        false
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
        let sys_power = if enabled {
            // Strictly guarantee PA bias and external PA switch are OFF before enabling internal coupling
            self.rf_power_off();
            self.update_gpio_reg(0x0060, 0); // PA disable (Pin 1), isolate RX antenna switch (Pin 0)
            SYS_POWER_LOOPBACK
        } else {
            self.set_gpio_pin(0, true); // Restore RX antenna switch
            SYS_POWER_RX
        };
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_RX);
        self.write_reg(REG_SYS_POWER, sys_power);
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
        self.bus.delay_ms(5); // Essential 5ms startup delay for 26 MHz crystal oscillator to stabilize

        self.apply_reg_vals(&[
            (REG_SYS_POWER, SYS_POWER_RX),
            (REG_TONE1_FREQ, 0x0000),
            (REG_TONE_GAIN, 0x0000),
        ]);
        self.apply_demod_af(false);

        // 4. Base GPIO state: RX enable (Pin 28) on, PA enable (Pin 29) off, LEDs off
        self.update_gpio_reg(0x0023, 0x0040);

        self.clear_interrupts();
    }

    pub fn set_filter_bandwidth(&mut self, bw: FilterBw) {
        let bw_val = match bw {
            FilterBw::Wide => 0,
            FilterBw::Narrow => 1,
            FilterBw::UltraNarrow => 2,
        };
        self.update_reg_bits(REG_FILTER_BW, 0b111 << 12, bw_val << 12);
    }

    pub fn set_deviation(&mut self, dev_hz: u32) {
        let (dev_val, _) = utils::math::div100_u16(dev_hz.min(65535) as u16);
        self.update_reg_bits(REG_MODULATION, 0x0FFF, dev_val & 0x0FFF);
    }

    pub fn set_agc_table(&mut self, table: &[u16; 5]) {
        for (i, &val) in table.iter().enumerate() {
            self.write_reg(0x10 + i as u8, val);
        }
    }

    /// Select external VHF vs UHF front-end bandpass and LNA path on REG_33.
    pub fn set_filter_path(&mut self, freq_hz: u32) {
        self.update_gpio_reg(
            0x000C,
            if freq_hz < 280_000_000 {
                0x0004
            } else {
                0x0008
            },
        );
    }

    pub fn set_frequency(&mut self, freq: Frequency<Hertz>) {
        let freq_changed = self.current_freq_hz != freq.raw();
        let was_rx = self.rx_demod_active;
        if freq_changed && was_rx {
            self.apply_demod_af(false);
        }

        let (low, high) = tuning::calculate_pll_dividers(freq);
        let f = freq.raw();
        self.finish_freq_tune(f, f >= 280_000_000, low, high);

        if freq_changed {
            self.clear_interrupts();
            if was_rx && self.is_squelch_open() {
                self.apply_demod_af(true);
            }
        }
    }

    pub fn get_rssi(&mut self) -> u16 {
        self.read_reg(REG_RSSI_VAL)
    }

    pub fn get_rssi_dbm(&mut self) -> i16 {
        let raw = self.get_rssi();
        if let Some(calib) = self.calib {
            calib.raw_rssi_to_dbm(raw)
        } else {
            squelch::raw_rssi_to_dbm(raw)
        }
    }

    #[inline(always)]
    fn update_gpio_reg(&mut self, clear_mask: u16, set_mask: u16) {
        self.update_reg_bits(REG_GPIO, clear_mask, set_mask);
    }

    pub fn set_status_led(&mut self, led: StatusLed) {
        self.update_gpio_reg(0x0003, led as u16);
    }

    /// Returns the shadowed BK4829 GPIO output register (`REG_GPIO`).
    #[inline(always)]
    pub fn read_gpio_reg(&mut self) -> u16 {
        self.read_reg(REG_GPIO)
    }

    pub fn set_gpio_pin(&mut self, pin: u8, high: bool) {
        let bit = 0x0040 >> pin;
        let (clear_mask, set_mask) = if high { (0, bit) } else { (bit, 0) };
        self.update_gpio_reg(clear_mask, set_mask);
    }

    pub fn set_agc(&mut self, enable: bool) {
        self.rmw_reg_bit(REG_AGC_CONFIG, 1 << 15, !enable);
    }

    /// Bind 64-byte active calibration reference and apply crystal trim to REG_3B.
    pub fn attach_calibration(&mut self, active_calib: &'a calib::ActiveCalib) {
        self.calib = Some(active_calib);
        self.set_crystal_trim(active_calib.xtal_freq_low());
    }

    pub fn new(bus: B) -> Self {
        let mut shadow = [0u16; 128];
        shadow[REG_GPIO as usize] = 0x9000;
        let mut shadow_valid = [0u32; 4];
        shadow_valid[(REG_GPIO >> 5) as usize] = 1u32 << (REG_GPIO & 31);
        Self {
            bus,
            tone_gain: 96,
            saved_tx_volume: 5,
            active_demod_af: AfType::Fm,
            rx_demod_active: false,
            calib: None,
            current_freq_hz: 144_000_000,
            squelch_level: 4,
            squelch_open: false,
            subaudible_matched: false,
            tone_hold_polls: 0,
            tail_burst_latched: false,
            loopback_active: false,
            saved_mod_dev: 0,
            shadow_valid,
            shadow,
            baseline_xtal_trim: 0,
            cur_xtal_trim: 0,
            rx_subaudible: kernel::service::radio::SubaudibleTone::None,
            tx_subaudible: kernel::service::radio::SubaudibleTone::None,
            am_tracker: kernel::service::radio::AmLinearityTracker::new(),
        }
    }

    pub fn init(&mut self) -> Result<(), ()> {
        let id = self.read_chip_id();
        if id != 0x4819 && id != 0x4829 {
            return Err(());
        }

        // 1. Soft reset pulse + shadow cache invalidation
        self.write_reg(0x00, 0x8000);
        self.bus.delay_ms(2);
        self.write_reg(0x00, 0x0000);
        self.invalidate_shadow();

        // 2. Crystal oscillator, bandgap, RX LDO, AGC, filter, routing, CXCSS, RX power & IRQ mask
        static INIT_REG_PATCHES: [(u8, u16); 18] = [
            (REG_POWER_CTRL, POWER_CTRL_RX),
            (REG_PA_CTRL, 0x0022),
            (REG_AGC_TABLE_0, 0x0318),
            (REG_AGC_TABLE_1, 0x033A),
            (REG_AGC_TABLE_2, 0x03DB),
            (REG_AGC_TABLE_3, 0x03DF),
            (REG_AGC_TABLE_MIN, 0x0210),
            (REG_AGC_TARGET, 0x2AB2),
            (0x7B, 0x73DC),
            (0x7D, 0xE920), // Microphone sensitivity
            (REG_MODULATION, 0x3516),
            (REG_AF_ROUTING, 0x6042), // Muted AF routing
            (REG_TX_MUTE, 0xBB18),
            (REG_GPIO, 0x9000), // Base GPIO state (Reg 0x33, LEDs off)
            (REG_CXCSS_FREQ, subaudible::CTC2_55HZ_DETECT_REG),
            (REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_NORMAL),
            (REG_SYS_POWER, SYS_POWER_RX),
            (
                REG_INT_ENABLE,
                INT_SQUELCH_FOUND
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
                    | INT_FSK_TX_FINISHED,
            ),
        ];
        self.apply_reg_vals(&INIT_REG_PATCHES);

        // 3. Default volume level 5 (Reg 0x48)
        self.set_volume_level(5);

        // 4. Squelch level 4 default profile and hardware amplifier shutdown
        self.squelch_level = 4;
        self.set_squelch_profile(&squelch::squelch_level_to_thresholds(4));
        self.bus.disable();

        self.clear_interrupts();

        Ok(())
    }
}

// ── Kernel Facet Trait Implementations ──────────────────────────────────────

impl<'a, B: Bk4829Bus> kernel::service::radio::RfChannel for Bk4829<'a, B> {
    fn configure_channel(&mut self, config: &kernel::service::radio::ChannelConfig) {
        let was_rx_active = self.rx_demod_active;
        if was_rx_active {
            if !crate::audio::AudioAmplifier::<B>::is_hangover_active() {
                self.bus.disable();
            }
            self.set_af(AfType::Mute);
        }
        self.set_rx_frequency(config.effective_rx_freq());
        <Self as kernel::service::radio::RfModulation>::set_bandwidth(self, config.bandwidth);
        <Self as kernel::service::radio::RfModulation>::set_modulation(self, config.modulation);
        self.set_power_level(config.power);
        <Self as kernel::service::radio::RfSubaudible>::set_rx_subaudible(
            self,
            config.effective_rx_subaudible(),
        );
        <Self as kernel::service::radio::RfSubaudible>::set_tx_subaudible(
            self,
            config.effective_tx_subaudible(),
        );
        <Self as kernel::service::radio::RfSquelch>::set_squelch_level(self, config.squelch_level);
        <Self as kernel::service::radio::RfAudioPath>::set_af_response(self, &config.af_response);
        if was_rx_active {
            if self.is_squelch_open() {
                self.set_af(self.active_demod_af);
                self.bus.enable();
            } else {
                self.rx_demod_active = false;
            }
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
        self.finish_freq_tune(state.freq_hz, state.uhf_path, state.pll_low, state.pll_high);
    }

    fn wait_pll_lock(&mut self, max_wait_us: u16) -> bool {
        // Poll RSSI/synth stability in 25us increments; return `false` on timeout.
        let steps = (((max_wait_us as u32) * 41_944) >> 20).max(1);
        let mut prev_rssi = self.read_reg(REG_RSSI_VAL);
        for _ in 0..steps {
            self.bus.delay_us(25);
            let cur_rssi = self.read_reg(REG_RSSI_VAL);
            if cur_rssi > 0 && cur_rssi.abs_diff(prev_rssi) <= 2 {
                return true;
            }
            prev_rssi = cur_rssi;
        }
        false
    }

    fn get_adjacent_channel_power(&mut self) -> (u8, u8) {
        self.get_adjacent_channel_power()
    }

    fn get_lna_peak_rssi(&mut self) -> u8 {
        self.get_lna_peak_rssi()
    }
}

// Packed per-mode profile bits (2 bytes per mode, 12 bytes total vs 72 bytes):
// [2:0] AfType (0=Fm,1=Am,2=Bb2,3=Bb1,4=Byp), [3] skip_if, [4] if_zero,
// [5] skip_filt_agc, [6] am_agc, [7] afc_en, [8] mod_set_12, [9] mod_clr_bw,
// [10] filt_clr_8, [11] filt_set_8, [12] filt_set_0.
const MP_SKIP_IF: u16 = 1 << 3;
const MP_IF_ZERO: u16 = 1 << 4;
const MP_SKIP_FA: u16 = 1 << 5;
const MP_AM_AGC: u16 = 1 << 6;
const MP_AFC_EN: u16 = 1 << 7;
const MP_MOD_S12: u16 = 1 << 8;
const MP_MOD_CBW: u16 = 1 << 9;
const MP_FLT_C8: u16 = 1 << 10;
const MP_FLT_S8: u16 = 1 << 11;
const MP_FLT_S0: u16 = 1 << 12;

static MOD_PROFILES: [u16; 6] = [
    // 0: FM / WFM (AfType::Fm = 0)
    MP_FLT_C8,
    // 1: AM / AmB (AfType::Am = 1)
    1 | MP_AM_AGC | MP_AFC_EN | MP_MOD_S12 | MP_FLT_S8,
    // 2: USB / DSB / CW (AfType::Baseband2 = 2)
    2 | MP_IF_ZERO | MP_AFC_EN | MP_MOD_S12 | MP_FLT_S8,
    // 3: PktFm / Dig (AfType::Fm = 0)
    MP_MOD_CBW | MP_FLT_S8 | MP_FLT_S0,
    // 4: Raw (AfType::Baseband1 = 3)
    3 | MP_SKIP_FA,
    // 5: Byp (AfType::Byp = 4)
    4 | MP_SKIP_IF | MP_SKIP_FA,
];

static MOD_AF_MAP: [AfType; 5] = [
    AfType::Fm,
    AfType::Am,
    AfType::Baseband2,
    AfType::Baseband1,
    AfType::Byp,
];

impl<'a, B: Bk4829Bus> kernel::service::radio::RfModulation for Bk4829<'a, B> {
    fn set_modulation(&mut self, mode: kernel::service::radio::ModulationMode) {
        use kernel::service::radio::ModulationMode::*;
        let idx = match mode {
            Am | AmB => 1,
            Usb | Dsb | Cw => 2,
            PktFm | Dig => 3,
            Raw => 4,
            Byp => 5,
            _ => 0,
        };
        let p = MOD_PROFILES[idx];
        let mod_clr = if (p & MP_MOD_CBW) != 0 {
            0x1000 | (0b11 << 14)
        } else {
            0x1000
        };
        let mod_set = if (p & MP_MOD_S12) != 0 { 0x1000 } else { 0 };
        self.update_reg_bits(REG_MODULATION, mod_clr, mod_set);
        if (p & MP_SKIP_IF) == 0 {
            let if_coeff = if (p & MP_IF_ZERO) != 0 { 0 } else { 0x2AAB };
            self.write_reg(REG_IF_COEFF, if_coeff);
        }
        let af = MOD_AF_MAP[(p & 7) as usize];
        self.active_demod_af = af;
        self.set_af(af);
        if (p & MP_SKIP_FA) == 0 {
            let afc_set = if (p & MP_AFC_EN) != 0 { 1 << 4 } else { 0 };
            self.update_reg_bits(REG_AFC, 1 << 4, afc_set);
            let filt_clr = if (p & MP_FLT_C8) != 0 { 1 << 8 } else { 0 };
            let mut filt_set = if (p & MP_FLT_S8) != 0 { 1 << 8 } else { 0 };
            if (p & MP_FLT_S0) != 0 {
                filt_set |= 1 << 0;
            }
            self.update_reg_bits(REG_FILTERS, filt_clr, filt_set);
            if (p & MP_AM_AGC) != 0 {
                <Self as kernel::service::radio::RfAgc>::set_agc_mode(
                    self,
                    kernel::service::radio::AgcListeningMode::AmListening,
                    false,
                );
            } else {
                self.set_agc(true);
            }
        }
    }

    fn set_bandwidth(&mut self, bw: kernel::service::radio::Bandwidth) {
        let filter_bw = match bw {
            kernel::service::radio::Bandwidth::Wide25k => FilterBw::Wide,
            kernel::service::radio::Bandwidth::Narrow12k5 => FilterBw::Narrow,
            kernel::service::radio::Bandwidth::Narrow6k25 => FilterBw::UltraNarrow,
        };
        self.set_filter_bandwidth(filter_bw);
    }

    fn set_deviation(&mut self, dev: kernel::service::radio::DeviationLimit) {
        self.set_deviation(dev.in_hz());
    }

    fn set_compander(&mut self, compander: kernel::service::radio::CompanderMode) {
        self.update_reg_bits(REG_MODULATION, 0b11 << 14, (compander as u16) << 14);
    }

    fn set_scrambler(&mut self, scrambler: kernel::service::radio::ScramblerMode) {
        let val = match scrambler {
            kernel::service::radio::ScramblerMode::Off => 0,
            kernel::service::radio::ScramblerMode::InvertFrequency(p) => (p & 0x07) as u16 + 1,
        };
        self.update_reg_bits(REG_MODULATION, 0b1111 << 8, val << 8);
    }
}

impl<'a, B: Bk4829Bus> kernel::service::radio::RfSquelch for Bk4829<'a, B> {
    fn set_squelch_level(&mut self, level: u8) {
        self.squelch_level = level;
        let th = if let Some(calib) = self.calib {
            let t = calib.squelch_thresholds(level);
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

impl<'a, B: Bk4829Bus> kernel::service::radio::RfSubaudible for Bk4829<'a, B> {
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
        use kernel::service::radio::CssTailMode;
        if mode == CssTailMode::Tone55Hz {
            self.apply_reg_vals(&[
                (REG_CXCSS_FREQ, subaudible::CSS_TAIL_55HZ_REG),
                (REG_CXCSS_TAIL, subaudible::CXCSS_TAIL_NORMAL),
                (REG_CXCSS_CTRL, subaudible::CXCSS_CTRL_CTCSS),
            ]);
            return;
        }
        const TAIL_VALS: [u16; 5] = [
            subaudible::CXCSS_TAIL_NORMAL,
            subaudible::CXCSS_TAIL_NORMAL,
            subaudible::CXCSS_TAIL_SHIFT_120,
            subaudible::CXCSS_TAIL_SHIFT_180,
            subaudible::CXCSS_TAIL_SHIFT_240,
        ];
        self.write_reg(REG_CXCSS_TAIL, TAIL_VALS[mode as usize]);
    }
}

impl<'a, B: Bk4829Bus> kernel::service::radio::RfFskPhy for Bk4829<'a, B> {
    fn fsk_configure_phy(
        &mut self,
        config: &kernel::service::radio::FskPhyConfig,
    ) -> Result<(), ()> {
        // Stop FSK first (per BK4819 Application Note Section 14)
        self.write_reg(REG_FSK_CTRL, 0x0000);

        // Clear TX and RX FIFO
        self.write_reg(REG_FSK_FIFO_CTRL, (1 << 15) | (1 << 14));
        self.bus.delay_ms(1);
        self.write_reg(REG_FSK_FIFO_CTRL, 0x0000);

        // Deviation (Reg 0x40), then Tone2 gain + baud divider
        self.set_deviation(config.deviation_hz as u32);

        let s = config.sync.bytes;
        let th = ((config.thresholds.almost_empty_words as u16) << 3)
            | (config.thresholds.almost_full_words as u16 & 0x07);
        self.apply_reg_vals(&[
            (REG_TONE_GAIN, 0x00E0),
            (REG_TONE2_FREQ, fsk::build_baud_divider(config.baud_rate)),
            (REG_FSK_CTRL, fsk::build_fsk_ctrl_from_phy(config)),
            (REG_FSK_SYNC_0_1, ((s[0] as u16) << 8) | (s[1] as u16)),
            (REG_FSK_SYNC_2_3, ((s[2] as u16) << 8) | (s[3] as u16)),
            (REG_FSK_CRC, fsk::REG_FSK_CRC_TRANSPARENT),
            (
                REG_FSK_PACKET_LEN,
                fsk::build_packet_length_reg(config.raw_length_bytes),
            ),
            (REG_FSK_FIFO_STATUS, th),
            (REG_FSK_FIFO_CTRL, fsk::build_fsk_fifo_ctrl_from_phy(config)),
        ]);

        // Clear interrupt status
        self.clear_interrupts();
        Ok(())
    }

    fn fsk_enable(&mut self, enable: bool) {
        self.rmw_reg_bit(REG_FSK_CTRL, 0x0001, enable);
    }

    fn fsk_enable_rx(&mut self, enable: bool) {
        if enable {
            let cur_irq = self.read_reg(REG_INT_ENABLE);
            self.write_reg(
                REG_INT_ENABLE,
                cur_irq | INT_FSK_RX_SYNC | INT_FSK_RX_FINISHED | INT_FSK_FIFO_ALMOST_FULL,
            );
        }
        self.rmw_reg_bit(REG_FSK_FIFO_CTRL, 1 << 12, enable);
    }

    fn fsk_enable_tx(&mut self, enable: bool) {
        if enable {
            self.write_reg(REG_INT_ENABLE, INT_FSK_TX_FINISHED);
        }
        self.rmw_reg_bit(REG_FSK_FIFO_CTRL, 1 << 11, enable);
    }

    fn fsk_clear_rx_fifo(&mut self) {
        self.pulse_fifo_bit(1 << 14);
    }

    fn fsk_clear_tx_fifo(&mut self) {
        self.pulse_fifo_bit(1 << 15);
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
        self.bus.delay_ms(20); // Settling delay
        self.fsk_enable_tx(true);

        // Reciprocal multiply by 1/5 ≈ 0xCCCD / 2^18 avoids Cortex-M0+ software division.
        let loops = (((timeout_ms as u32) * 0xCCCD) >> 18).max(1);
        let mut finished = false;
        for _ in 0..loops {
            self.bus.delay_ms(5);
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

impl<'a, B: Bk4829Bus> kernel::service::radio::RfAudioPath for Bk4829<'a, B> {
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

        let rx_3k = (response.rx_3k + 4).clamp(0, 8) as usize;
        let tx_3k = (response.tx_3k + 4).clamp(0, 8) as usize;
        let rx_300 = (response.rx_300 + 4).clamp(0, 8) as usize;
        let tx_300 = (response.tx_300 + 4).clamp(0, 8) as usize;
        let pairs = unsafe {
            [
                (0x75, *TBL_3K.get_unchecked(rx_3k)),
                (0x74, *TBL_3K.get_unchecked(tx_3k)),
                (0x54, *TBL_300_D1.get_unchecked(rx_300)),
                (0x55, *TBL_300_D2.get_unchecked(rx_300)),
                (0x44, *TBL_300_D1.get_unchecked(tx_300)),
                (0x45, *TBL_300_D2.get_unchecked(tx_300)),
            ]
        };
        self.apply_reg_vals(&pairs);
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

    fn mute_audio(&mut self, mute: bool) {
        self.rx_demod_active = !mute;
        self.apply_demod_af(!mute);
        if mute {
            if !crate::audio::AudioAmplifier::<B>::is_hangover_active() {
                self.bus.disable();
            }
        } else if self.read_reg(REG_AF_LEVEL) & 0x000F != 0 {
            self.bus.enable();
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

impl<'a, B: Bk4829Bus> kernel::service::radio::RfAgc for Bk4829<'a, B> {
    fn set_agc_table(&mut self, table: &[u16; 5]) {
        self.set_agc_table(table);
    }

    fn set_agc_mode(&mut self, mode: kernel::service::radio::AgcListeningMode, disable: bool) {
        use kernel::service::radio::AgcListeningMode;
        if !disable {
            let (table_min, target) = match mode {
                AgcListeningMode::Fm => {
                    self.am_tracker.reset();
                    self.write_reg(REG_AGC_TABLE_3, 0x03DF);
                    (0x0210, 0x2AB2)
                }
                AgcListeningMode::AmListening => (0x0000, (50 << 7) | 32),
                AgcListeningMode::Ssb => {
                    self.am_tracker.reset();
                    (0x0000, (1 << 14) | (44 << 7) | 28)
                }
            };
            self.write_reg(REG_AGC_TABLE_MIN, table_min);
            self.write_reg(REG_AGC_TARGET, target);
        }
        self.set_agc(!disable);
    }

    fn set_agc_speed(&mut self, speed: kernel::service::radio::AgcSpeed) {
        const AGC_SPEED_REGS: [u16; 4] = [
            (56 << 7) | 40,
            0x2AB2,
            (2 << 14) | (48 << 7) | 32,
            (3 << 14) | (42 << 7) | 24,
        ];
        self.write_reg(REG_AGC_TARGET, AGC_SPEED_REGS[speed as usize]);
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

impl<'a, B: Bk4829Bus> kernel::service::radio::RfFineTuning for Bk4829<'a, B> {
    fn set_crystal_trim(&mut self, trim: i16) {
        self.set_crystal_trim(trim);
    }

    fn set_pll_offset_10hz(&mut self, offset_10hz: i16) {
        let cur_low = self.read_reg(REG_PLL_LOW);
        let adjusted = (cur_low as i32 + offset_10hz as i32).clamp(0, 0xFFFF) as u16;
        self.write_reg(REG_PLL_LOW, adjusted);
    }

    fn get_crystal_trim(&mut self) -> i16 {
        self.cur_xtal_trim
    }

    fn set_sub_hz_offset(&mut self, carrier_hz: u32, offset_dhz: i16) {
        let (nominal_10hz, _) = utils::div10_u32(carrier_hz);
        let (trim, adjusted_10hz) = if offset_dhz == 0 {
            (self.baseline_xtal_trim, nominal_10hz as i32)
        } else {
            let res = kernel::service::radio::vernier::solve_vernier(carrier_hz, offset_dhz);
            let final_trim = (self.baseline_xtal_trim as i32 + res.xtal_trim_offset as i32)
                .clamp(-300, 300) as i16;
            (final_trim, nominal_10hz as i32 + res.pll_comp_10hz as i32)
        };
        self.set_crystal_trim(trim);

        let low = (adjusted_10hz & 0xFFFF) as u16;
        let high = ((adjusted_10hz >> 16) & 0xFFFF) as u16;
        self.write_pll_words(low, high);

        if offset_dhz != 0 {
            // Toggle REG_SYS_POWER to re-sync PLL lock across trim changes (PR 271)
            let reg30 = self.read_reg(REG_SYS_POWER);
            self.write_reg(REG_SYS_POWER, 0);
            self.write_reg(REG_SYS_POWER, reg30);
        }
    }

    fn get_afc_offset(&mut self) -> i16 {
        self.get_afc_offset()
    }
}

impl<'a, B: Bk4829Bus> Bk4829<'a, B> {
    /// Clamp `REG_MODULATION` (`0x40`) deviation bits `[11:0]` to `0x043C` while the internal
    /// tone DDS is active so `1750 Hz` / DTMF / roger bursts never over-deviate narrowband repeaters.
    fn clamp_tone_mod_dev(&mut self) {
        if self.saved_mod_dev == 0 {
            let cur = self.read_reg(REG_MODULATION);
            if cur != 0xFFFF {
                self.saved_mod_dev = 0x8000 | (cur & 0x0FFF);
                self.write_reg(REG_MODULATION, (cur & 0xF000) | 0x043C);
            }
        }
    }

    fn restore_tone_mod_dev(&mut self) {
        if self.saved_mod_dev != 0 {
            let dev = self.saved_mod_dev & 0x0FFF;
            self.saved_mod_dev = 0;
            self.update_reg_bits(REG_MODULATION, 0x0FFF, dev);
        }
    }

    fn set_tone_raw(&mut self, tone2_and_enabled: u8, freq_word: u16, gain: u8) {
        let is_tone2 = (tone2_and_enabled & 2) != 0;
        let enabled = (tone2_and_enabled & 1) != 0;
        if enabled && freq_word != 0 {
            self.clamp_tone_mod_dev();
        }
        self.write_reg(
            if is_tone2 {
                REG_TONE2_FREQ
            } else {
                REG_TONE1_FREQ
            },
            freq_word,
        );
        let shift = if is_tone2 { 0 } else { 8 };
        let field = (((enabled as u16) << 7) | (gain as u16 & 0x7F)) << shift;
        self.update_reg_bits(REG_TONE_GAIN, 0x00FFu16 << shift, field);
    }

    fn set_tone_gain_field(&mut self, shift: u8, gain: u8) {
        self.update_reg_bits(
            REG_TONE_GAIN,
            0x7Fu16 << shift,
            (gain as u16 & 0x7F) << shift,
        );
    }
}

impl<'a, B: Bk4829Bus> kernel::service::radio::RfToneGenerator for Bk4829<'a, B> {
    fn set_tone1_raw(&mut self, freq_word: u16, gain: u8, enabled: bool) {
        self.set_tone_raw(enabled as u8, freq_word, gain);
    }

    fn set_tone2_raw(&mut self, freq_word: u16, gain: u8, enabled: bool) {
        self.set_tone_raw(2 | (enabled as u8), freq_word, gain);
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
        self.set_tone_gain_field(8, gain);
    }

    fn set_tone2_gain(&mut self, gain: u8) {
        self.set_tone_gain_field(0, gain);
    }

    fn enable_tone1(&mut self, enable: bool) {
        if enable {
            self.clamp_tone_mod_dev();
        }
        self.rmw_reg_bit(REG_TONE_GAIN, 1 << 15, enable);
    }

    fn enable_tone2(&mut self, enable: bool) {
        if enable {
            self.clamp_tone_mod_dev();
        }
        self.rmw_reg_bit(REG_TONE_GAIN, 1 << 7, enable);
    }

    fn mute_tones(&mut self) {
        self.write_reg(REG_TONE_GAIN, 0x0000);
        self.restore_tone_mod_dev();
    }
}

impl<'a, B: Bk4829Bus> kernel::service::radio::RfPowerSequencer for Bk4829<'a, B> {
    fn tx_prepare(&mut self) {
        let cur_vol = (self.read_reg(REG_AF_LEVEL) & 0x000F) as u8;
        if cur_vol != 0 {
            self.saved_tx_volume = cur_vol;
        }
        self.set_volume_level(0);
        self.set_gpio_pin(0, false); // RX path off
        self.write_reg(REG_POWER_CTRL, POWER_CTRL_TX);
        // Apply TX subaudible tone configuration into REG_07/08/51/52
        self.apply_subaudible_hw(self.tx_subaudible);
    }

    fn tx_ramp_up(&mut self, target_power_level: u8) {
        let bias = if let Some(calib) = self.calib {
            calib.tx_power_bias(self.current_freq_hz, target_power_level)
        } else {
            (target_power_level.min(2) << 6) | 0x20
        };
        self.tx_ramp_up_bias(bias);
    }

    fn tx_unmute(&mut self) {
        self.write_reg(REG_SYS_POWER, SYS_POWER_TX);
    }

    fn tx_mute(&mut self) {
        self.update_reg_bits(REG_SYS_POWER, 0x0200, 0);
    }

    fn tx_ramp_down(&mut self) {
        self.rf_power_off();
        self.bus.delay_ms(3);
        self.set_gpio_pin(1, false); // PA disable
    }

    fn tx_conclude(&mut self) {
        self.set_gpio_pin(0, true); // RX enable
        self.apply_reg_vals(&[
            (REG_POWER_CTRL, POWER_CTRL_RX), // Restore 2.7V RX RF LDO for full sensitivity!
            (REG_SYS_POWER, SYS_POWER_RX),
        ]);
        self.apply_subaudible_hw(self.rx_subaudible);
        if self.saved_tx_volume != 0 {
            self.set_volume_level(self.saved_tx_volume);
        }
    }

    fn tx_state(&self) -> kernel::service::radio::TransceiverTxState {
        kernel::service::radio::TransceiverTxState::IdleRx
    }
}

impl<'a, B: Bk4829Bus> kernel::service::radio::RadioService for Bk4829<'a, B> {
    fn sleep(&mut self) {
        self.sleep();
    }

    fn wake(&mut self) {
        self.wake();
    }

    fn set_xtal_freq_trim(&mut self, trim: i16) {
        self.set_xtal_freq_trim(trim);
    }

    fn set_rf_led(&mut self, led: kernel::service::radio::RfLedState) {
        let hw = match led {
            kernel::service::radio::RfLedState::Off => StatusLed::Off,
            kernel::service::radio::RfLedState::RxGreen => StatusLed::Green,
            kernel::service::radio::RfLedState::TxRed => StatusLed::Red,
        };
        self.set_status_led(hw);
    }

    fn set_internal_loopback(&mut self, enabled: bool) {
        self.set_internal_loopback(enabled);
    }

    fn is_internal_loopback(&self) -> bool {
        self.loopback_active
    }
}
