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

// ── BK4829 Driver ───────────────────────────────────────────────────────────

pub struct Bk4829<'a, State = Disabled> {
    pub(crate) cache: RegisterCache,
    pub(crate) bus: &'a mut dyn kern_platform_traits::radio::RadioBus,
    pub(crate) tone_gain: u8,
    pub(crate) active_demod_af: AfType,
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
        self.write_reg(REG_XTAL_TRIM, 0x1D00);
    }

    pub fn set_crystal_trim(&mut self, trim: i16) {
        let val = (22656i32 + trim as i32).clamp(0, 65535) as u16;
        self.write_reg(0x3B, val);
    }

    pub fn set_af(&mut self, af: AfType) {
        self.write_reg(REG_AF_ROUTING, 0x6042 | ((af as u16) << 8));
    }

    pub fn set_volume_level(&mut self, level: u8) {
        let vol_idx = (level as usize).min(crate::audio::VOLUME_TABLE.len() - 1);
        let (rx_gain2, dac_gain) = crate::audio::VOLUME_TABLE[vol_idx];
        let val = (11u16 << 12) | ((rx_gain2 as u16 & 0x3F) << 4) | (dac_gain as u16 & 0x0F);
        self.write_reg(REG_AF_LEVEL, val);
    }

    pub fn set_mic_gain(&mut self, gain_db: u8) {
        let val = (gain_db.min(31) as u16) & 0x1F;
        let mut reg44 = self.cache.read(REG_MIC_GAIN);
        reg44 = (reg44 & !0x001F) | val;
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
        if notes.is_empty() {
            return;
        }

        // 1. Prepare tone generator with TX Mute active (prevents clicks/pops while configuring)
        self.write_reg(REG_TX_MUTE, 0xBB18);
        self.set_af(AfType::Beep);
        let gain = if self.tone_gain == 0 {
            96
        } else {
            self.tone_gain
        };
        let tone_cfg = 0x8000 | ((gain as u16 & 0x7F) << 8);
        self.write_reg(REG_TONE_GAIN, tone_cfg);

        // Reset REG_30 first, then power on AF DAC + DISC mode + TX DSP
        self.write_reg(REG_SYS_POWER, 0x0000);
        self.write_reg(REG_SYS_POWER, 0x0302);

        // 2. Hardware Audio Power Amplifier Warm-Up:
        // On Quansheng UV-K1 / K5, the LN4898 audio power amplifier (PA8) has an internal
        // soft-start bypass capacitor that requires ~50ms to power up and stabilize.
        // During this warm-up, TX Mute (0xBB18) ensures absolute silence without pops.
        let was_amp_active = bsp::pins::AUDIO_PA.is_high();
        if !was_amp_active {
            bsp::pins::AUDIO_PA.set_high();
            py32_hal::delay::delay_ms(50);
        }

        // 3. Play note sequence
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

        // 4. Power amplifier post-tone hold & shutdown
        py32_hal::delay::delay_ms(5);
        if !was_amp_active {
            bsp::pins::AUDIO_PA.set_low();
            py32_hal::delay::delay_ms(2);
        }

        // 5. Restore clean RX state
        self.write_reg(REG_TONE1_FREQ, 0x0000);
        self.write_reg(REG_TONE_GAIN, 0x0000);
        self.set_af(AfType::Mute);
        self.write_reg(REG_TX_MUTE, 0x3B18);
        self.write_reg(REG_SYS_POWER, 0x0000);
        self.write_reg(REG_SYS_POWER, 0xBFF1);
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
        self.set_crystal_trim(trim);
    }

    /// Set PA bias/gain (REG_36). Only touches the PA register — never the
    /// power-enable (0x30) or bandgap/LDO (0x37) registers.
    pub fn set_rf_power(&mut self, level: u8) {
        // REG_36: <15:8> PA bias, <7> PA-CTL enable, <5:3> gain1, <2:0> gain2
        let bias: u16 = match level {
            0 => 0x20, // Low
            1 => 0x60, // Medium
            _ => 0xA0, // High
        };
        let gain: u16 = (4 << 3) | 2;
        self.write_reg(0x36, (bias << 8) | (1 << 7) | gain);
    }

    /// PA off (REG_36 reset value used by init).
    pub fn rf_power_off(&mut self) {
        self.write_reg(0x36, 0x0022);
    }

    pub fn set_squelch_thresholds(&mut self, noise: u16, glitch: u16, rssi: u16) {
        self.write_reg(REG_SQL_NOISE_TH, noise);
        self.write_reg(REG_SQL_GLITCH_TH, glitch);
        self.write_reg(REG_SQL_RSSI_TH, rssi);
    }

    pub fn is_squelch_open(&mut self) -> bool {
        let reg65 = self.read_reg(REG_CARRIER_STATUS);
        (reg65 & 0x0001) != 0
    }

    pub fn get_snr(&mut self) -> u8 {
        let reg66 = self.read_reg(REG_SNR_VAL);
        ((reg66 >> 8) & 0xFF) as u8
    }

    pub fn get_glitch(&mut self) -> u8 {
        let reg63 = self.read_reg(REG_GLITCH_VAL);
        (reg63 & 0xFF) as u8
    }

    pub fn get_noise(&mut self) -> u8 {
        let reg64 = self.read_reg(REG_NOISE_VAL);
        (reg64 & 0xFF) as u8
    }

    pub fn sleep(&mut self) {
        self.write_reg(REG_SYS_POWER, 0x0000);
        self.write_reg(REG_XTAL_TRIM, 0x1D00);
    }

    pub fn wake(&mut self) {
        self.write_reg(REG_SYS_POWER, 0x0200);
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
        self.write_reg(REG_INT_CLEAR, (1 << 13) | (1 << 12));
    }

    pub fn send_fsk_data(&mut self, data: &[u16]) {
        self.write_reg(REG_INT_CLEAR, 1 << 15);
        for &word in data {
            self.write_reg(REG_FSK_FIFO_DATA, word);
        }
        let reg58 = self.cache.read(REG_FSK_CTRL);
        self.write_reg(REG_FSK_CTRL, reg58 | 0x0100);
    }

    pub fn set_frequency(&mut self, freq: Frequency<Hertz>) {
        let (low, high) = tuning::calculate_pll_dividers(freq);
        self.write_reg(REG_PLL_LOW, low);
        self.write_reg(REG_PLL_HIGH, high);
    }

    pub fn get_rssi(&mut self) -> u16 {
        self.read_reg(REG_RSSI_VAL)
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
            bsp::StatusLed::Yellow => reg33 |= 0x0003,
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

// ── Boot & Power Transitions ────────────────────────────────────────────────

impl<'a> Bk4829<'a, Disabled> {
    pub fn new(bus: &'a mut dyn kern_platform_traits::radio::RadioBus) -> Self {
        Self {
            cache: RegisterCache::new(),
            bus,
            tone_gain: 128,
            active_demod_af: AfType::Fm,
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

        // 2. Crystal oscillator and bandgap enable
        self.write_reg(REG_XTAL_TRIM, 0x9D1F);
        self.write_reg(0x36, 0x0022); // PA bias off

        // 3. DSP and internal filter parameters (standard table from DeltaFW & fagci-k1-fw)
        self.write_reg(0x10, 0x0318);
        self.write_reg(0x11, 0x033A);
        self.write_reg(0x12, 0x03DB);
        self.write_reg(0x13, 0x03DF);
        self.write_reg(0x14, 0x0210);
        self.write_reg(0x49, 0x2AB2);
        self.write_reg(0x7B, 0x73DC);
        self.write_reg(0x7D, 0xE920); // Microphone sensitivity

        // 4. Default volume level 5 (Reg 0x48)
        self.set_volume_level(5);

        // 5. Default modulation and AF routing (FM)
        self.write_reg(REG_MODULATION, 0x3516);
        self.set_af(AfType::Fm);
        self.write_reg(REG_GPIO, 0x9000); // Base GPIO state (Reg 0x33)

        // 6. Power on RX link, AF DAC, DISC mode, PLL/VCO, RX DSP
        self.write_reg(REG_SYS_POWER, 0xBFF1);

        Ok(())
    }

    pub fn into_rx(mut self) -> Bk4829<'a, Receiving> {
        self.write_reg(REG_GPIO, 1 << 6); // Green LED
        self.write_reg(REG_SYS_POWER, 0x0200); // RX blocks power
        Bk4829 {
            cache: self.cache,
            bus: self.bus,
            tone_gain: self.tone_gain,
            active_demod_af: self.active_demod_af,
            _state: PhantomData,
        }
    }

    pub fn into_tx(mut self) -> Bk4829<'a, Transmitting> {
        self.write_reg(REG_GPIO, 1 << 5); // Red LED
        self.write_reg(REG_SYS_POWER, 0x0020); // TX blocks power
        Bk4829 {
            cache: self.cache,
            bus: self.bus,
            tone_gain: self.tone_gain,
            active_demod_af: self.active_demod_af,
            _state: PhantomData,
        }
    }
}

impl<'a> Bk4829<'a, Receiving> {
    pub fn disable(mut self) -> Bk4829<'a, Disabled> {
        let mut reg0d = self.cache.read(REG_GPIO);
        reg0d &= !(1 << 6);
        self.write_reg(REG_GPIO, reg0d);
        self.write_reg(REG_SYS_POWER, 0x0000);
        Bk4829 {
            cache: self.cache,
            bus: self.bus,
            tone_gain: self.tone_gain,
            active_demod_af: self.active_demod_af,
            _state: PhantomData,
        }
    }
}

impl<'a> Bk4829<'a, Transmitting> {
    pub fn disable(mut self) -> Bk4829<'a, Disabled> {
        let mut reg0d = self.cache.read(REG_GPIO);
        reg0d &= !(1 << 5);
        self.write_reg(REG_GPIO, reg0d);
        self.write_reg(REG_SYS_POWER, 0x0000);
        Bk4829 {
            cache: self.cache,
            bus: self.bus,
            tone_gain: self.tone_gain,
            active_demod_af: self.active_demod_af,
            _state: PhantomData,
        }
    }
}

// ── Kernel Facet Trait Implementations ──────────────────────────────────────

impl<'a, State> kernel::service::radio::RfChannel for Bk4829<'a, State> {
    fn configure_channel(&mut self, config: &kernel::service::radio::ChannelConfig) {
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
}

impl<'a, State> kernel::service::radio::RfTuning for Bk4829<'a, State> {
    fn set_modulation(&mut self, mode: kernel::service::radio::ModulationMode) {
        <Self as kernel::service::radio::RfModulation>::set_modulation(self, mode);
    }

    fn set_bandwidth(&mut self, bw: kernel::service::radio::Bandwidth) {
        <Self as kernel::service::radio::RfModulation>::set_bandwidth(self, bw);
    }

    fn set_xtal_trim(&mut self, trim: i16) {
        self.set_crystal_trim(trim);
    }
}

impl<'a, State> kernel::service::radio::RfModulation for Bk4829<'a, State> {
    fn set_modulation(&mut self, mode: kernel::service::radio::ModulationMode) {
        let mut reg40 = self.cache.read(REG_MODULATION);
        match mode {
            kernel::service::radio::ModulationMode::Fm => {
                reg40 &= !0x1000; // FM mode bit in Reg 0x40
                self.write_reg(REG_MODULATION, reg40);
                self.write_reg(REG_IF_COEFF, 0x2AAB); // Standard IF filter coefficient
                // Enable AFC (bit 4 of REG 0x73 = 0)
                let reg73 = self.cache.read(REG_AFC);
                self.write_reg(REG_AFC, reg73 & !(1 << 4));
                // Set demodulator to FM and restore de-emphasis
                self.active_demod_af = AfType::Fm;
                self.set_af(AfType::Fm);
                let reg2b = self.cache.read(REG_FILTERS);
                self.write_reg(REG_FILTERS, reg2b & !(1 << 8)); // Enable RX de-emphasis
                self.set_agc(true);
            }
            kernel::service::radio::ModulationMode::Am
            | kernel::service::radio::ModulationMode::AmB => {
                reg40 |= 0x1000; // AM mode bit in Reg 0x40
                self.write_reg(REG_MODULATION, reg40);
                self.write_reg(REG_IF_COEFF, 0x2AAB);
                // Disable AFC (bit 4 of REG 0x73 = 1): FM AFC pulls toward sidebands, causing severe pitch flutter in AM
                let reg73 = self.cache.read(REG_AFC);
                self.write_reg(REG_AFC, reg73 | (1 << 4));
                // Set demodulator to AM and disable de-emphasis (AM does not use pre-emphasis)
                self.active_demod_af = AfType::Am;
                self.set_af(AfType::Am);
                let reg2b = self.cache.read(REG_FILTERS);
                self.write_reg(REG_FILTERS, reg2b | (1 << 8)); // Disable RX de-emphasis
                // Configure AM AGC gain staging: drop min gain floor to prevent envelope limiter saturation
                self.write_reg(REG_AGC_TABLE_MIN, 0x0000);
                self.write_reg(REG_AGC_TARGET, (0 << 14) | (50 << 7) | (32 << 0));
                self.set_agc(true);
            }
            kernel::service::radio::ModulationMode::Usb
            | kernel::service::radio::ModulationMode::Dsb
            | kernel::service::radio::ModulationMode::Cw => {
                reg40 |= 0x1000;
                self.write_reg(REG_MODULATION, reg40);
                // Bypass IF filter coefficient (0x0000 opens wide IF baseband for sideband reception)
                self.write_reg(REG_IF_COEFF, 0x0000);
                // Disable AFC (no carrier to lock onto in suppressed carrier SSB/CW)
                let reg73 = self.cache.read(REG_AFC);
                self.write_reg(REG_AFC, reg73 | (1 << 4));
                // Route Baseband2 (undocumented SSB audio tap) and disable de-emphasis
                self.active_demod_af = AfType::Baseband2;
                self.set_af(AfType::Baseband2);
                let reg2b = self.cache.read(REG_FILTERS);
                self.write_reg(REG_FILTERS, reg2b | (1 << 8)); // Disable RX de-emphasis
                self.set_agc(true);
            }
            kernel::service::radio::ModulationMode::PktFm => {
                // Packet Flat FM: zero de-emphasis/pre-emphasis, flat audio passband for TNC/FT8/APRS
                reg40 &= !0x1000; // FM mode
                reg40 &= !(0b11 << 14); // Compander off
                self.write_reg(REG_MODULATION, reg40);
                self.write_reg(REG_IF_COEFF, 0x2AAB);
                let reg73 = self.cache.read(REG_AFC);
                self.write_reg(REG_AFC, reg73 & !(1 << 4));
                self.active_demod_af = AfType::Fm;
                self.set_af(AfType::Fm);
                // Disable both TX pre-emphasis (bit 0) and RX de-emphasis (bit 8)
                let reg2b = self.cache.read(REG_FILTERS);
                self.write_reg(REG_FILTERS, reg2b | (1 << 0) | (1 << 8));
                self.set_agc(true);
            }
            kernel::service::radio::ModulationMode::Raw => {
                reg40 &= !0x1000;
                self.write_reg(REG_MODULATION, reg40);
                self.write_reg(REG_IF_COEFF, 0x2AAB);
                self.active_demod_af = AfType::Baseband1;
                self.set_af(AfType::Baseband1);
            }
            kernel::service::radio::ModulationMode::Byp => {
                reg40 &= !0x1000;
                self.write_reg(REG_MODULATION, reg40);
                self.active_demod_af = AfType::Byp;
                self.set_af(AfType::Byp);
            }
            kernel::service::radio::ModulationMode::Wfm => {
                reg40 &= !0x1000;
                self.write_reg(REG_MODULATION, reg40);
                self.write_reg(REG_IF_COEFF, 0x2AAB);
                self.active_demod_af = AfType::Fm;
                self.set_af(AfType::Fm);
            }
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
        let th = squelch::squelch_level_to_thresholds(level);
        self.set_squelch_thresholds(th.open_noise, th.open_glitch, th.open_rssi);
    }

    fn set_squelch_thresholds(&mut self, th: &kernel::service::radio::squelch::SquelchThresholds) {
        self.set_squelch_thresholds(th.open_noise, th.open_glitch, th.open_rssi);
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
        let raw = self.get_rssi();
        squelch::raw_rssi_to_dbm(raw)
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
        match tone {
            kernel::service::radio::SubaudibleTone::None => {
                let reg4b = self.cache.read(REG_CTCSS_ENABLE);
                self.write_reg(REG_CTCSS_ENABLE, reg4b & !0x0001);
                let reg51 = self.cache.read(REG_DCS_ENABLE);
                self.write_reg(REG_DCS_ENABLE, reg51 & !0x0001);
            }
            kernel::service::radio::SubaudibleTone::Ctcss(deci_hz) => {
                let reg_val = subaudible::ctcss_deci_hz_to_reg(deci_hz);
                self.write_reg(REG_CTCSS_RX_FREQ, reg_val);
                let reg4b = self.cache.read(REG_CTCSS_ENABLE);
                self.write_reg(REG_CTCSS_ENABLE, reg4b | 0x0001);
            }
            kernel::service::radio::SubaudibleTone::DcsNormal(code)
            | kernel::service::radio::SubaudibleTone::DcsInverted(code) => {
                self.write_reg(REG_DCS_RX_CODE, code);
                let reg51 = self.cache.read(REG_DCS_ENABLE);
                self.write_reg(REG_DCS_ENABLE, reg51 | 0x0001);
            }
        }
    }

    fn set_tx_subaudible(&mut self, tone: kernel::service::radio::SubaudibleTone) {
        match tone {
            kernel::service::radio::SubaudibleTone::None => {
                let reg4b = self.cache.read(REG_CTCSS_ENABLE);
                self.write_reg(REG_CTCSS_ENABLE, reg4b & !0x0002);
            }
            kernel::service::radio::SubaudibleTone::Ctcss(deci_hz) => {
                let reg_val = subaudible::ctcss_deci_hz_to_reg(deci_hz);
                self.write_reg(REG_CTCSS_TX_FREQ, reg_val);
                let reg4b = self.cache.read(REG_CTCSS_ENABLE);
                self.write_reg(REG_CTCSS_ENABLE, reg4b | 0x0002);
            }
            kernel::service::radio::SubaudibleTone::DcsNormal(code)
            | kernel::service::radio::SubaudibleTone::DcsInverted(code) => {
                self.write_reg(REG_DCS_TX_CODE, code);
            }
        }
    }

    fn is_subaudible_matched(&mut self) -> bool {
        let reg65 = self.read_reg(REG_CARRIER_STATUS);
        (reg65 & 0x0006) != 0
    }

    fn send_css_tail(&mut self, mode: kernel::service::radio::CssTailMode) {
        match mode {
            kernel::service::radio::CssTailMode::None => {}
            kernel::service::radio::CssTailMode::Tone55Hz => {
                self.write_reg(REG_CTCSS_TX_FREQ, subaudible::CSS_TAIL_55HZ_REG);
                let reg4b = self.cache.read(REG_CTCSS_ENABLE);
                self.write_reg(REG_CTCSS_ENABLE, reg4b | 0x0002);
                py32_hal::delay::delay_ms(150);
                self.write_reg(REG_CTCSS_ENABLE, reg4b & !0x0002);
            }
            kernel::service::radio::CssTailMode::PhaseShift120Deg
            | kernel::service::radio::CssTailMode::PhaseShift180Deg => {
                // Invert DCS code or shift tone
                py32_hal::delay::delay_ms(150);
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
        self.write_reg(REG_TONE1_GAIN, 0x00E0);

        // Baud rate divider (Reg 0x72)
        self.write_reg(REG_FSK_BAUD_DIV, fsk::build_baud_divider(config.baud_rate));

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
        self.write_reg(REG_INT_CLEAR, 0xFFFF);
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
        self.write_reg(REG_TONE1_GAIN, g);
    }

    fn fsk_set_baud_divider(&mut self, divider: u16) {
        self.write_reg(REG_FSK_BAUD_DIV, divider);
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
            if (reg0c & INT_FSK_TX_FINISHED) != 0 {
                finished = true;
                break;
            }
        }

        // Clear interrupt flag and drop TX enable
        self.write_reg(REG_INT_CLEAR, INT_FSK_TX_FINISHED);
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
            bsp::pins::AUDIO_PA.set_low();
            self.set_af(AfType::Mute);
        } else {
            self.set_af(self.active_demod_af);
            if self.cache.read(REG_AF_LEVEL) & 0x000F != 0 {
                bsp::pins::AUDIO_PA.set_high();
            }
        }
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
                    self.set_agc(true);
                }
                kernel::service::radio::AgcListeningMode::AmListening => {
                    self.write_reg(REG_AGC_TABLE_MIN, 0x0000);
                    self.write_reg(REG_AGC_TARGET, (0 << 14) | (50 << 7) | (32 << 0));
                    self.set_agc(true);
                }
                kernel::service::radio::AgcListeningMode::Ssb => {
                    self.set_agc(true);
                }
            }
        }
    }

    fn set_agc_speed(&mut self, _speed: kernel::service::radio::AgcSpeed) {}
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
        (self.cache.read(REG_XTAL_TRIM) & 0x00FF) as i16
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
        let cur_ctcss = self.cache.read(REG_CTCSS_ENABLE);
        self.write_reg(REG_CTCSS_ENABLE, cur_ctcss & !0x0002);
        self.set_gpio_pin(0, false); // RX path off
    }

    fn tx_ramp_up(&mut self, target_power_level: u8) {
        self.set_gpio_pin(1, true); // PA enable
        py32_hal::delay::delay_ms(2);
        if target_power_level > 0 {
            self.rf_power_off();
            py32_hal::delay::delay_ms(3);
        }
        self.set_rf_power(target_power_level);
        py32_hal::delay::delay_ms(5);
    }

    fn tx_unmute(&mut self) {
        let mut reg30 = self.cache.read(REG_SYS_POWER);
        reg30 |= 0x0200;
        self.write_reg(REG_SYS_POWER, reg30);
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
        let cur_ctcss = self.cache.read(REG_CTCSS_ENABLE);
        self.write_reg(REG_CTCSS_ENABLE, cur_ctcss | 0x0002);
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
        self.set_crystal_trim(trim);
    }
}
