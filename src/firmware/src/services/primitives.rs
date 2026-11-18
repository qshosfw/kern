//! High-Level Coordinated Microkernel Service Methods on `SysContext`.
//!
//! Direct inherent methods on `SysContext` for display, radio, audio, storage,
//! power, serial, FM, and raw hardware buses without single-implementor trait hops.

use crate::services::SysContext;
#[cfg(feature = "rf_fsk")]
use kernel::service::radio::{FskBaudRate, FskPhyConfig};
use kernel::service::radio::{RfChannel, RfModulation, RfSquelch, RfSubaudible};
use kernel::service::{DisplayService, PowerService};

#[inline(always)]
pub fn color_from_u8(color: u8) -> gfx::Color {
    gfx::Color::from_u8(color)
}

#[inline(always)]
pub fn font_from_u8(font_id: u8) -> &'static gfx::GfxFont {
    if font_id == 1 {
        &gfx::fonts::mumatrix_regular::FONT
    } else {
        &gfx::fonts::tomthumb::FONT
    }
}

/// Convert DTMF character to BK4819/BK4829 dual DDS control words.
#[inline(never)]
pub fn dtmf_tone_words(digit: u8) -> (u16, u16) {
    const ROW_WORDS: [u16; 4] = [7196, 7950, 8796, 9715];
    const COL_WORDS: [u16; 4] = [12482, 13793, 15249, 16860];
    let norm = if digit >= b'a' && digit <= b'd' {
        digit - (b'a' - b'A')
    } else {
        digit
    };
    for (idx, &key) in b"123A456B789C*0#D".iter().enumerate() {
        if key == norm {
            return (ROW_WORDS[idx >> 2], COL_WORDS[idx & 3]);
        }
    }
    (0, 0)
}

#[inline(never)]
fn app_config_path(app_name: &str) -> heapless::String<32> {
    let mut path = heapless::String::<32>::new();
    let _ = path.push_str("/pref/app/");
    let _ = path.push_str(app_name);
    let _ = path.push_str(".cfg");
    path
}

#[inline(never)]
pub fn list_krx_dir(
    storage: &mut crate::services::SystemStorage,
    dir_path: &str,
    out_entries: &mut [kernel::proc::abi::KrxDirEntry],
) -> Result<usize, ()> {
    let mut count = 0usize;
    storage
        .fs
        .list_dir(dir_path, |path, size, _perm, is_dir| {
            if let Some(dst) = out_entries.get_mut(count) {
                unsafe { core::ptr::write_bytes(dst, 0, 1) };
                let b = path.as_bytes();
                let n = b.len().min(24);
                dst.name[..n].copy_from_slice(&b[..n]);
                dst.size = size as u32;
                dst.is_dir = u8::from(is_dir);
                count += 1;
            }
        })
        .map_err(|_| ())?;
    Ok(count)
}

#[inline(never)]
fn spi_xfer_bytes(is_flash: bool, tx: &[u8], rx: &mut [u8]) -> Result<(), ()> {
    let count = tx.len().max(rx.len());
    for i in 0..count {
        let out_b = tx.get(i).copied().unwrap_or(0);
        let in_b = if is_flash {
            py32_hal::spi::spi2_transfer(out_b)
        } else {
            py32_hal::spi::transfer_byte(out_b)
        }
        .map_err(|_| ())?;
        if let Some(slot) = rx.get_mut(i) {
            *slot = in_b;
        }
    }
    Ok(())
}

const HW_GPIO_PINS: [py32_hal::gpio::Pin; 4] = [
    bsp::pins::FLASHLIGHT,
    bsp::pins::PTT,
    bsp::pins::AUDIO_PA,
    bsp::pins::BACKLIGHT,
];

impl<'a, 'hw> SysContext<'a, 'hw> {
    // ── Display ─────────────────────────────────────────────────────────────
    pub fn set_inverted(&mut self, inverted: bool) {
        if let Some(ref mut d) = self.display {
            d.set_inverted(inverted);
        }
    }

    pub fn set_contrast(&mut self, contrast: u8) {
        if let Some(ref mut d) = self.display {
            d.set_contrast(contrast);
        }
    }

    pub fn commit_rows(&mut self, row_mask: u8) {
        if let Some(ref mut d) = self.display {
            d.canvas.dirty |= row_mask;
            d.lcd.blit(d.canvas);
        }
    }

    // ── Radio ───────────────────────────────────────────────────────────────
    #[inline(never)]
    pub(crate) fn apply_subaudible_split(
        &mut self,
        set_rx: bool,
        set_tx: bool,
        tone: kernel::service::radio::SubaudibleTone,
    ) {
        {
            let ch = self.state.coord.active_channel_mut();
            if set_rx {
                ch.rx_subaudible = tone;
            }
            if set_tx {
                ch.tx_subaudible = tone;
            }
        }
        if set_rx && !self.state.coord.is_transmitting {
            self.radio.set_rx_subaudible(tone);
        }
        if set_tx {
            self.radio.set_tx_subaudible(tone);
        }
    }

    pub fn tune(&mut self, freq_hz: u32, bw_hz: u32, step_hz: u32) {
        use kernel::service::radio::{Bandwidth, StepSetting};
        use utils::units::Frequency;
        let bw = match bw_hz {
            0..=12_500 => Bandwidth::Narrow12k5,
            _ => Bandwidth::Wide25k,
        };
        let (step_10hz, _) = utils::math::div10_u32(step_hz);
        let step = StepSetting::from_hz_x10(step_10hz as u16);
        let slot = self.state.coord.active_vfo;
        {
            let ch = self.state.coord.vfo_channel_mut(slot);
            ch.rx_freq = Frequency::new(freq_hz);
            ch.bandwidth = bw;
            ch.step = step;
        }
        if !self.state.coord.is_transmitting {
            self.state.coord.select_vfo(self.radio, slot);
        }
    }

    pub fn set_af_mode(&mut self, mode: u8) {
        use drivers::bk4829::AfType;
        const AF_LUT: [AfType; 8] = [
            AfType::Mute,
            AfType::Fm,
            AfType::Mute,
            AfType::Mute,
            AfType::Baseband1,
            AfType::Baseband2,
            AfType::Mute,
            AfType::Am,
        ];
        self.radio
            .set_af(AF_LUT.get(mode as usize).copied().unwrap_or(AfType::Mute));
    }

    pub fn set_agc(&mut self, table_idx: u8) {
        self.radio.set_agc(table_idx != 0);
    }

    pub fn read_register(&mut self, reg: u8) -> u16 {
        self.radio.read_reg(reg)
    }

    pub fn write_register(&mut self, reg: u8, val: u16) {
        self.radio.write_reg(reg, val);
    }

    pub fn set_modulation(&mut self, mod_type: u8) {
        use kernel::service::radio::ModulationMode;
        let mode = ModulationMode::from_u8(mod_type);
        self.state.coord.active_channel_mut().modulation = mode;
        if !self.state.coord.is_transmitting {
            RfModulation::set_modulation(self.radio, mode);
        }
    }

    pub fn set_squelch(&mut self, level: u8) {
        let sql = level.clamp(0, 9);
        self.state.coord.active_channel_mut().squelch_level = sql;
        if !self.state.coord.is_transmitting {
            self.radio.set_squelch_level(sql);
        }
    }

    pub fn get_rssi_dbm(&mut self) -> i16 {
        self.radio.get_rssi_dbm()
    }

    pub fn is_carrier_detected(&mut self) -> bool {
        self.state.coord.carrier_active || self.radio.is_carrier_open()
    }

    pub fn set_tx_power(&mut self, level: u8) {
        let pwr = kernel::service::radio::RfPowerLevel::from_u8(level);
        self.state.coord.active_channel_mut().power = pwr;
        if !self.state.coord.is_transmitting {
            self.radio.set_power_level(pwr);
        }
    }

    pub fn send_tail(&mut self) {
        use kernel::service::radio::CssTailMode;
        self.radio.send_css_tail(CssTailMode::Tone55Hz);
    }

    pub fn mute_mic(&mut self, muted: bool) {
        self.radio.mute_mic(muted);
    }

    #[cfg(feature = "rf_fsk")]
    pub fn fsk_init(&mut self, baud: u16) {
        let baud_rate = match baud {
            1200 => FskBaudRate::Baud1200,
            2400 => FskBaudRate::Baud2400,
            4800 => FskBaudRate::Baud4800,
            9600 => FskBaudRate::Baud9600,
            _ => FskBaudRate::Baud1200,
        };
        let mut config = FskPhyConfig::default();
        config.baud_rate = baud_rate;
        config.raw_length_bytes = 64;
        let _ = crate::services::radio::fsk_stream::FskStreamService::arm_rx(self.radio, &config);
    }

    #[cfg(feature = "rf_fsk")]
    pub fn fsk_send(&mut self, data: &[u8]) -> Result<(), ()> {
        let mut words = [0u16; 64];
        let word_count = (data.len() + 1) / 2;
        let limit = word_count.min(words.len());
        for i in 0..limit {
            let b0 = data[i * 2] as u16;
            let b1 = if i * 2 + 1 < data.len() {
                data[i * 2 + 1] as u16
            } else {
                0
            };
            words[i] = (b0 << 8) | b1;
        }
        crate::services::radio::fsk_stream::FskStreamService::transmit_words(
            self.radio,
            &words[..limit],
            3,
            500,
        )
    }

    #[cfg(feature = "rf_fsk")]
    pub fn fsk_receive(&mut self, buf: &mut [u8]) -> usize {
        let mut words = [0u16; 32];
        let count = crate::services::radio::fsk_stream::FskStreamService::poll_and_read(
            self.radio, &mut words,
        );
        let mut out_len = 0;
        for i in 0..count {
            if out_len + 1 < buf.len() {
                buf[out_len] = (words[i] >> 8) as u8;
                buf[out_len + 1] = (words[i] & 0xFF) as u8;
                out_len += 2;
            } else if out_len < buf.len() {
                buf[out_len] = (words[i] >> 8) as u8;
                out_len += 1;
            }
        }
        out_len
    }

    pub fn sweep(&mut self, start_hz: u32, step_hz: u32, out_rssi: &mut [i16]) -> usize {
        let count = out_rssi.len().min(128);
        let mut freq = start_hz;
        for slot in &mut out_rssi[..count] {
            self.radio
                .set_rx_frequency(utils::units::Frequency::new(freq));
            *slot = self.radio.get_rssi_dbm();
            freq = freq.saturating_add(step_hz);
        }
        count
    }

    pub fn emit_symbols(&mut self, symbols: &[kernel::proc::abi::KrxSymbol]) -> Result<(), ()> {
        for sym in symbols {
            self.radio
                .write_reg(drivers::bk4829::registers::REG_TONE1_FREQ, sym.freq_word);
            py32_hal::delay::delay_us(sym.duration_us as u32);
        }
        Ok(())
    }

    pub fn set_dual_tones_raw(
        &mut self,
        tone1_word: u16,
        tone1_gain: u8,
        tone2_word: u16,
        tone2_gain: u8,
    ) {
        use kernel::service::radio::RfToneGenerator;
        self.radio
            .set_dual_tones_raw(tone1_word, tone1_gain, tone2_word, tone2_gain);
    }

    pub fn set_dual_tones_hz(
        &mut self,
        tone1_hz: u16,
        tone1_gain: u8,
        tone2_hz: u16,
        tone2_gain: u8,
    ) {
        use kernel::service::radio::tone_gen::calc_tone_word;
        self.set_dual_tones_raw(
            calc_tone_word(tone1_hz),
            tone1_gain,
            calc_tone_word(tone2_hz),
            tone2_gain,
        );
    }

    pub fn set_dtmf(&mut self, digit: u8, gain: u8) {
        let (w1, w2) = dtmf_tone_words(digit);
        self.set_dual_tones_raw(w1, gain, w2, gain);
    }

    // ── Audio ───────────────────────────────────────────────────────────────
    pub fn set_audio_path(&mut self, on: bool) {
        if let Some(ref mut audio) = self.audio {
            if on {
                audio.enable();
            } else {
                audio.disable();
            }
        }
    }

    pub fn is_audio_playing(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| a.is_active())
    }

    pub fn set_mic_gain(&mut self, gain: u8) {
        self.radio.set_mic_gain(gain);
    }

    pub fn read_mic_sample(&mut self) -> u16 {
        self.radio
            .read_reg(drivers::bk4829::registers::REG_AUDIO_AMP)
    }

    // ── Storage ─────────────────────────────────────────────────────────────
    pub fn load_config(&mut self, app_name: &str, buf: &mut [u8]) -> Result<usize, ()> {
        let path = app_config_path(app_name);
        self.storage.read_file(path.as_str(), buf).map_err(|_| ())
    }

    pub fn save_config(&mut self, app_name: &str, data: &[u8]) -> Result<(), ()> {
        let path = app_config_path(app_name);
        crate::security::write_classified_file(self.storage, path.as_str(), data).map_err(|_| ())
    }

    pub fn read_asset(&mut self, _asset_id: u16, offset: u32, buf: &mut [u8]) -> Result<usize, ()> {
        #[cfg(feature = "krx_native")]
        {
            crate::runtime::native_runner::read_active_pkg_asset(self.storage, offset, buf)
        }
        #[cfg(not(feature = "krx_native"))]
        {
            let _ = (offset, buf);
            Err(())
        }
    }

    #[inline(always)]
    pub fn list_dir(
        &mut self,
        dir_path: &str,
        out_entries: &mut [kernel::proc::abi::KrxDirEntry],
    ) -> Result<usize, ()> {
        list_krx_dir(self.storage, dir_path, out_entries)
    }

    // ── Power & Backlight ───────────────────────────────────────────────────
    pub fn set_backlight(&mut self, mode: u8, timeout_s: u8) {
        if let Some(ref mut d) = self.display {
            let bmode = match mode {
                0 => kernel::service::BacklightMode::Off,
                1 => kernel::service::BacklightMode::Auto(((timeout_s as u16) * 1000).min(60000)),
                _ => kernel::service::BacklightMode::On,
            };
            d.set_backlight_mode(bmode);
        }
    }

    pub fn set_backlight_brightness(&mut self, max: u8, min: u8) {
        if let Some(ref mut d) = self.display {
            d.set_backlight_brightness(max, min);
        }
    }

    pub fn wake_backlight(&mut self) {
        if let Some(ref mut d) = self.display {
            d.wake();
        }
    }

    pub fn set_flashlight(&mut self, on: bool) {
        if on {
            bsp::pins::FLASHLIGHT.set_high();
        } else {
            bsp::pins::FLASHLIGHT.set_low();
        }
    }

    // ── Serial ──────────────────────────────────────────────────────────────
    pub fn write(&mut self, data: &[u8]) {
        #[cfg(feature = "usb")]
        if let Some(ref mut usb) = self.usb {
            if usb.is_connected() {
                let _ = usb.cdc_write(data);
                return;
            }
        }
        #[cfg(feature = "uart")]
        {
            self.uart.write(data);
        }
        #[cfg(not(any(feature = "usb", feature = "uart")))]
        let _ = data;
    }

    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        #[cfg(feature = "usb")]
        if let Some(ref mut usb) = self.usb {
            if usb.is_connected() {
                return usb.cdc_read(buf);
            }
        }
        #[cfg(feature = "uart")]
        {
            self.uart.read(buf)
        }
        #[cfg(not(any(feature = "usb", feature = "uart")))]
        {
            let _ = buf;
            0
        }
    }

    pub fn is_usb_active(&self) -> bool {
        #[cfg(feature = "usb")]
        {
            self.usb.as_ref().is_some_and(|u| u.is_connected())
        }
        #[cfg(not(feature = "usb"))]
        {
            false
        }
    }

    // ── Math & Random ───────────────────────────────────────────────────────
    pub fn rand32(&mut self) -> u32 {
        let seed = py32_hal::delay::millis()
            ^ ((self
                .radio
                .read_reg(drivers::bk4829::registers::REG_NOISE_VAL) as u32)
                << 16);
        crypto::crc::CrcEngine::crc32(&seed.to_le_bytes())
    }

    // ── FM Broadcast Tuner ──────────────────────────────────────────────────
    pub fn fm_tune(&mut self, freq_100khz: u16) {
        #[cfg(feature = "fm_radio")]
        {
            let i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let mut bk = drivers::bk1080::Bk1080::new(i2c);
            let _ = bk.set_frequency_10khz(freq_100khz.saturating_mul(10));
        }
        #[cfg(not(feature = "fm_radio"))]
        let _ = freq_100khz;
    }

    pub fn fm_set_volume(&mut self, volume: u8) {
        #[cfg(feature = "fm_radio")]
        {
            let i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let mut bk = drivers::bk1080::Bk1080::new(i2c);
            let _ = bk.set_volume(volume);
        }
        #[cfg(not(feature = "fm_radio"))]
        let _ = volume;
    }

    pub fn fm_mute(&mut self, mute: bool) {
        #[cfg(feature = "fm_radio")]
        {
            let i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let mut bk = drivers::bk1080::Bk1080::new(i2c);
            let _ = bk.set_mute(mute);
        }
        #[cfg(not(feature = "fm_radio"))]
        let _ = mute;
    }

    pub fn fm_get_rssi(&mut self) -> u8 {
        #[cfg(feature = "fm_radio")]
        {
            let i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let mut bk = drivers::bk1080::Bk1080::new(i2c);
            bk.get_rssi()
        }
        #[cfg(not(feature = "fm_radio"))]
        {
            0
        }
    }

    // ── Low-Level Hardware & Buses ──────────────────────────────────────────
    pub fn spi_transfer(&mut self, cs_pin: u8, tx: &[u8], rx: &mut [u8]) -> Result<(), ()> {
        let is_flash = cs_pin == 0;
        let cs = if is_flash {
            bsp::pins::FLASH_CS
        } else {
            bsp::pins::LCD_CS
        };
        cs.set_low();
        let res = spi_xfer_bytes(is_flash, tx, rx);
        let _ = if is_flash {
            py32_hal::spi::spi2_wait_idle()
        } else {
            py32_hal::spi::wait_idle()
        };
        cs.set_high();
        res
    }

    pub fn gpio_read(&self, pin: u8) -> bool {
        if let Some(&p) = HW_GPIO_PINS.get(pin as usize) {
            p.is_high() ^ (pin == 1)
        } else if pin == 4 {
            self.power.is_charging()
        } else {
            false
        }
    }

    pub fn gpio_write(&mut self, pin: u8, high: bool) {
        if pin == 0 {
            self.set_flashlight(high);
        } else if pin == 2 || pin == 3 {
            let p = HW_GPIO_PINS[pin as usize];
            if high {
                p.set_high();
            } else {
                p.set_low();
            }
        }
    }

    pub fn raw_flash_read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), ()> {
        self.storage.fs.dev.read_data(addr, buf).map_err(|_| ())
    }

    pub fn active_vfo(&self) -> kernel::service::radio::VfoSlot {
        self.state.coord.active_vfo
    }

    pub fn select_vfo(&mut self, slot: kernel::service::radio::VfoSlot) {
        self.state.coord.select_vfo(self.radio, slot);
    }

    pub fn toggle_vfo(&mut self) {
        self.state.coord.toggle_vfo(self.radio);
    }

    pub fn step_vfo_frequency(&mut self, slot: kernel::service::radio::VfoSlot, delta_hz: i32) {
        self.state.coord.step_vfo_freq(self.radio, slot, delta_hz);
    }

    pub fn toggle_reverse(&mut self, slot: kernel::service::radio::VfoSlot) {
        self.state.coord.toggle_reverse(self.radio, slot);
    }

    pub fn is_transmitting(&self) -> bool {
        self.state.coord.is_transmitting
    }

    pub fn is_carrier_active(&self) -> bool {
        self.state.coord.carrier_active
    }

    pub fn active_channel(&self) -> &kernel::service::radio::ChannelConfig {
        self.state.coord.active_channel()
    }

    pub fn request_ptt_start(&mut self) -> kernel::service::radio::TxPermission {
        self.state.coord.request_ptt_start(
            self.radio,
            kernel::service::radio::ClientToken::new(
                kernel::service::radio::ClientId::VFO_UI,
                kernel::service::radio::LockTier::FOREGROUND,
            ),
        )
    }

    pub fn request_ptt_stop(&mut self) {
        if self.state.coord.is_transmitting {
            self.state.coord.request_ptt_stop(self.radio);
        }
    }
}
