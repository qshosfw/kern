//! High-Level Coordinated Microkernel Primitives Implementation (§8.2 / KRX-ABI 2.5.0).
//!
//! Exposes the complete 12 primitive interfaces on `SysContext` with full subsystem
//! coordination (RadioCoordinator, Audio priority ladder, Canvas compositor, KernFS, Keypad).

use crate::services::SysContext;
use kernel::proc::primitives::*;
#[cfg(feature = "rf_fsk")]
use kernel::service::radio::{FskBaudRate, FskPhyConfig};
use kernel::service::radio::{RfAudioPath, RfChannel, RfModulation, RfSquelch, RfSubaudible};
use kernel::service::{DisplayService, PowerService};

// ── 1. Display Primitive ────────────────────────────────────────────────────
#[inline(never)]
pub fn color_from_u8(color: u8) -> gfx::Color {
    gfx::Color::from_u8(color)
}

#[inline(never)]
pub fn font_from_u8(font_id: u8) -> &'static gfx::GfxFont {
    if font_id == 1 {
        &gfx::fonts::mumatrix_regular::FONT
    } else {
        &gfx::fonts::tomthumb::FONT
    }
}

impl<'a, 'hw> SysContext<'a, 'hw> {
    #[inline(always)]
    fn active_canvas(&mut self) -> &mut gfx::Canvas {
        crate::runtime::syscall::active_canvas_mut()
    }

    #[inline(never)]
    fn apply_subaudible_tone(&mut self, tone: kernel::service::radio::SubaudibleTone) {
        let ch = self.state.coord.active_channel_mut();
        ch.rx_subaudible = tone;
        ch.tx_subaudible = tone;
        self.radio.set_rx_subaudible(tone);
        self.radio.set_tx_subaudible(tone);
    }
}

impl<'a, 'hw> DisplayPrimitive for SysContext<'a, 'hw> {
    fn clear(&mut self) {
        self.active_canvas().clear();
    }

    fn draw_pixel(&mut self, x: i16, y: i16, color: u8) {
        let col = color_from_u8(color);
        self.active_canvas().draw_pixel_i(x, y, col);
    }

    fn draw_line(&mut self, x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
        let col = color_from_u8(color);
        gfx::draw::draw_line(self.active_canvas(), x0, y0, x1, y1, col);
    }

    fn draw_rect(&mut self, x: i16, y: i16, w: i16, h: i16, color: u8) {
        let col = color_from_u8(color);
        self.active_canvas().draw_rect_i(x, y, w, h, col);
    }

    fn fill_rect(&mut self, x: i16, y: i16, w: i16, h: i16, color: u8) {
        let col = color_from_u8(color);
        self.active_canvas().fill_rect_i(x, y, w, h, col);
    }

    fn draw_bitmap(&mut self, x: i16, y: i16, w: u8, h: u8, data: &[u8]) {
        gfx::draw::draw_bitmap(self.active_canvas(), x, y, w, h, data, gfx::Color::Fill);
    }

    fn draw_str(&mut self, x: i16, y: i16, text: &str, font_id: u8, color: u8) {
        let col = color_from_u8(color);
        let font = font_from_u8(font_id);
        gfx::text::draw_str(self.active_canvas(), x, y, text, font, col);
    }

    fn set_inverted(&mut self, inverted: bool) {
        if let Some(ref mut d) = self.display {
            d.set_inverted(inverted);
        }
    }

    fn set_contrast(&mut self, contrast: u8) {
        if let Some(ref mut d) = self.display {
            d.set_contrast(contrast);
        }
    }

    fn draw_str_max(&mut self, x: i16, y: i16, text: &str, font_id: u8, color: u8, max_w: i16) {
        let col = color_from_u8(color);
        let font = font_from_u8(font_id);
        gfx::text::draw_str_max_x(
            self.active_canvas(),
            x,
            y,
            text,
            font,
            col,
            (x + max_w).max(0),
        );
    }

    fn draw_str_align(&mut self, x: i16, y: i16, text: &str, font_id: u8, color: u8, align: u8) {
        let col = color_from_u8(color);
        let font = font_from_u8(font_id);
        let al = match align {
            1 => gfx::text::Align::Center,
            2 => gfx::text::Align::Right,
            _ => gfx::text::Align::Left,
        };
        gfx::text::draw_str_align(self.active_canvas(), x, y, text, font, col, al);
    }

    fn commit_frame(&mut self) {
        if let Some(ref mut d) = self.display {
            d.lcd.blit(d.canvas);
        }
    }

    fn commit_rows(&mut self, row_mask: u8) {
        if let Some(ref mut d) = self.display {
            d.canvas.dirty |= row_mask;
            d.lcd.blit(d.canvas);
        }
    }

    fn get_framebuffer_mut(&mut self) -> &mut [u8; 1024] {
        let canvas = self.active_canvas();
        unsafe { &mut *(canvas.buffer.as_mut_ptr() as *mut [u8; 1024]) }
    }
}

// ── 2. Radio Primitive ──────────────────────────────────────────────────────
impl<'a, 'hw> RadioPrimitive for SysContext<'a, 'hw> {
    fn tune(&mut self, freq_hz: u32, bw_hz: u32, step_hz: u32) {
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
        self.state.coord.select_vfo(self.radio, slot);
    }

    fn get_frequency(&mut self) -> u32 {
        self.state.coord.active_channel().rx_freq.raw()
    }

    fn get_bandwidth(&mut self) -> u32 {
        self.state.coord.active_channel().bandwidth.in_hz()
    }

    fn get_modulation(&mut self) -> u8 {
        self.state.coord.active_channel().modulation as u8
    }

    fn get_tx_state(&mut self) -> u8 {
        if self.state.coord.is_transmitting {
            1
        } else {
            0
        }
    }

    fn set_tx_carrier(&mut self, on: bool) {
        self.set_tx(on);
    }

    fn set_af_mode(&mut self, mode: u8) {
        use drivers::bk4829::AfType;
        let af = match mode {
            1 => AfType::Fm,
            7 => AfType::Am,
            4 => AfType::Baseband1,
            5 => AfType::Baseband2,
            _ => AfType::Mute,
        };
        self.radio.set_af(af);
    }

    fn set_agc(&mut self, table_idx: u8) {
        self.radio.set_agc(table_idx != 0);
    }

    fn read_register(&mut self, reg: u8) -> u16 {
        self.radio.read_reg(reg)
    }

    fn write_register(&mut self, reg: u8, val: u16) {
        self.radio.write_reg(reg, val);
    }

    fn set_modulation(&mut self, mod_type: u8) {
        use kernel::service::radio::ModulationMode;
        let mode = ModulationMode::from_u8(mod_type);
        self.state.coord.active_channel_mut().modulation = mode;
        RfModulation::set_modulation(self.radio, mode);
    }

    fn set_squelch(&mut self, level: u8) {
        let sql = level.clamp(0, 9);
        self.state.coord.active_channel_mut().squelch_level = sql;
        self.radio.set_squelch_level(sql);
    }

    fn get_rssi_dbm(&mut self) -> i16 {
        self.radio.get_rssi_dbm()
    }

    fn get_glitch(&mut self) -> u8 {
        self.radio.get_glitch()
    }

    fn is_carrier_detected(&mut self) -> bool {
        self.state.coord.carrier_active || self.radio.is_carrier_open()
    }

    fn set_tx(&mut self, active: bool) {
        use crate::services::radio::coordinator::{ClientId, ClientToken, LockTier};
        let token = ClientToken::new(ClientId::SYSTEM, LockTier::FOREGROUND);
        if active {
            let _ = self.state.coord.request_ptt_start(self.radio, token);
        } else {
            self.state.coord.request_ptt_stop(self.radio);
        }
    }

    fn set_tx_power(&mut self, level: u8) {
        use kernel::service::radio::RfPowerLevel;
        let pwr = match level {
            0 => RfPowerLevel::Low,
            1 => RfPowerLevel::Medium,
            _ => RfPowerLevel::High,
        };
        self.state.coord.active_channel_mut().power = pwr;
        self.radio.set_power_level(pwr);
    }

    fn set_subaudible_ctcss(&mut self, freq_tenths_hz: u16) {
        use kernel::service::radio::SubaudibleTone;
        let tone = if freq_tenths_hz == 0 {
            SubaudibleTone::None
        } else {
            SubaudibleTone::Ctcss(freq_tenths_hz)
        };
        self.apply_subaudible_tone(tone);
    }

    fn set_subaudible_dcs(&mut self, code: u16, inverted: bool) {
        use kernel::service::radio::SubaudibleTone;
        let tone = if code == 0 {
            SubaudibleTone::None
        } else if inverted {
            SubaudibleTone::DcsInverted(code)
        } else {
            SubaudibleTone::DcsNormal(code)
        };
        self.apply_subaudible_tone(tone);
    }

    fn send_tail(&mut self) {
        use kernel::service::radio::CssTailMode;
        self.radio.send_css_tail(CssTailMode::Tone55Hz);
    }

    fn mute(&mut self, muted: bool) {
        self.radio.mute_audio(muted);
    }

    fn mute_mic(&mut self, muted: bool) {
        self.radio.mute_mic(muted);
    }

    #[cfg(feature = "rf_fsk")]
    fn fsk_init(&mut self, baud: u16) {
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
    #[cfg(not(feature = "rf_fsk"))]
    fn fsk_init(&mut self, _baud: u16) {}

    #[cfg(feature = "rf_fsk")]
    fn fsk_send(&mut self, data: &[u8]) -> Result<(), ()> {
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
    #[cfg(not(feature = "rf_fsk"))]
    fn fsk_send(&mut self, _data: &[u8]) -> Result<(), ()> {
        Err(())
    }

    #[cfg(feature = "rf_fsk")]
    fn fsk_receive(&mut self, buf: &mut [u8]) -> usize {
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
    #[cfg(not(feature = "rf_fsk"))]
    fn fsk_receive(&mut self, _buf: &mut [u8]) -> usize {
        0
    }

    fn sweep(&mut self, start_hz: u32, step_hz: u32, out_rssi: &mut [i16]) -> usize {
        let count = out_rssi.len().min(128);
        for i in 0..count {
            let freq = start_hz.saturating_add((i as u32).saturating_mul(step_hz));
            self.radio
                .set_rx_frequency(utils::units::Frequency::new(freq));
            out_rssi[i] = self.radio.get_rssi_dbm();
        }
        count
    }

    fn emit_symbols(&mut self, symbols: &[kernel::proc::abi::KrxSymbol]) -> Result<(), ()> {
        for sym in symbols {
            self.radio
                .write_reg(drivers::bk4829::registers::REG_TONE1_FREQ, sym.freq_word);
            py32_hal::delay::delay_us(sym.duration_us as u32);
        }
        Ok(())
    }

    fn set_dual_tones_raw(
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
}

// ── 3. Audio Primitive ──────────────────────────────────────────────────────
impl<'a, 'hw> AudioPrimitive for SysContext<'a, 'hw> {
    fn play_tone(&mut self, freq_hz: u16, duration_ms: u16) {
        self.play_tone(freq_hz, duration_ms);
    }

    fn play_tone_sequence(&mut self, notes: &[(u16, u16)]) {
        for &(freq, dur) in notes {
            self.play_tone(freq, dur);
        }
    }

    fn play_beep(&mut self, beep_type: u8) {
        let beep = crate::services::melody::BeepType::from_u8(beep_type);
        self.play_beep(beep);
    }

    fn set_volume(&mut self, volume: u8) {
        self.set_volume(volume);
    }

    fn get_volume(&self) -> u8 {
        self.config.audio.volume
    }

    fn set_audio_path(&mut self, on: bool) {
        if let Some(ref mut audio) = self.audio {
            if on {
                audio.enable();
            } else {
                audio.disable();
            }
        }
    }

    fn stop_audio(&mut self) {
        if let Some(ref mut audio) = self.audio {
            audio.disable();
        }
    }

    fn is_audio_playing(&self) -> bool {
        self.audio.as_ref().map_or(false, |a| a.is_active())
    }

    fn set_mic_gain(&mut self, gain: u8) {
        self.radio.set_mic_gain(gain);
    }

    fn read_mic_sample(&mut self) -> u16 {
        self.radio
            .read_reg(drivers::bk4829::registers::REG_AUDIO_AMP)
    }

    fn capture_start(&mut self, rate_hz: u16) -> Result<(), ()> {
        crate::audio_capture_mut().start_capture(rate_hz)
    }

    fn capture_read(&mut self, out_samples: &mut [i16]) -> usize {
        crate::audio_capture_mut().read_samples(out_samples)
    }

    fn capture_stop(&mut self) {
        crate::audio_capture_mut().stop_capture();
    }

    fn is_capturing(&self) -> bool {
        crate::audio_capture_mut().is_capturing()
    }
}

#[inline(never)]
fn app_config_path(app_name: &str) -> heapless::String<32> {
    let mut path = heapless::String::<32>::new();
    let _ = path.push_str("/pref/app/");
    let _ = path.push_str(app_name);
    let _ = path.push_str(".cfg");
    path
}

// ── 4. Storage Primitive ────────────────────────────────────────────────────
impl<'a, 'hw> StoragePrimitive for SysContext<'a, 'hw> {
    fn read_file(&mut self, path: &str, buf: &mut [u8]) -> Result<usize, ()> {
        self.storage.read_file(path, buf).map_err(|_| ())
    }

    fn read_file_at(&mut self, path: &str, offset: usize, buf: &mut [u8]) -> Result<usize, ()> {
        self.storage.read_file_at(path, offset, buf).map_err(|_| ())
    }

    fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), ()> {
        self.storage.write_file(path, data).map_err(|_| ())
    }

    fn append_file(&mut self, path: &str, data: &[u8]) -> Result<(), ()> {
        let buf = crate::data::arena::staging_buf();
        let existing_len = self.storage.read_file(path, buf).unwrap_or(0);
        if existing_len + data.len() > buf.len() {
            return Err(());
        }
        buf[existing_len..existing_len + data.len()].copy_from_slice(data);
        self.storage
            .write_file(path, &buf[..existing_len + data.len()])
            .map_err(|_| ())
    }

    fn remove_file(&mut self, path: &str) -> Result<(), ()> {
        self.storage.remove(path).map_err(|_| ())
    }

    fn file_size(&mut self, path: &str) -> Result<usize, ()> {
        self.storage.file_size(path).map_err(|_| ())
    }

    fn exists(&mut self, path: &str) -> bool {
        self.storage.exists(path)
    }

    fn load_config(&mut self, app_name: &str, buf: &mut [u8]) -> Result<usize, ()> {
        let path = app_config_path(app_name);
        self.storage.read_file(path.as_str(), buf).map_err(|_| ())
    }

    fn save_config(&mut self, app_name: &str, data: &[u8]) -> Result<(), ()> {
        let path = app_config_path(app_name);
        self.storage.write_file(path.as_str(), data).map_err(|_| ())
    }

    fn read_asset(&mut self, _asset_id: u16, offset: u32, buf: &mut [u8]) -> Result<usize, ()> {
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

    #[inline(never)]
    fn list_dir(
        &mut self,
        dir_path: &str,
        out_entries: &mut [kernel::proc::abi::KrxDirEntry],
    ) -> Result<usize, ()> {
        const MAX_FETCH: usize = 8;
        const {
            assert!(
                core::mem::size_of::<[kernel::service::DirEntryInfo; MAX_FETCH]>()
                    <= crate::data::arena::TRANSIENT_ARENA_BYTES
            );
        }
        let arena = crate::data::arena::transient_arena_bytes_mut();
        let temp_entries = unsafe {
            &mut *(arena.as_mut_ptr() as *mut [kernel::service::DirEntryInfo; MAX_FETCH])
        };
        let max_fetch = out_entries.len().min(MAX_FETCH);
        let count = self
            .storage
            .list_dir(dir_path, &mut temp_entries[..max_fetch])
            .map_err(|_| ())?;
        for i in 0..count {
            let src = &temp_entries[i];
            let dst = &mut out_entries[i];
            dst.name = [0u8; 24];
            let name_bytes = src.path.as_bytes();
            let copy_len = name_bytes.len().min(24);
            dst.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
            dst.size = src.size as u32;
            dst.is_dir = if src.is_dir { 1 } else { 0 };
            dst.reserved = [0u8; 3];
        }
        Ok(count)
    }

    #[inline(never)]
    fn rename_file(&mut self, old_path: &str, new_path: &str) -> Result<(), ()> {
        if old_path.starts_with("/sys/") || new_path.starts_with("/sys/") {
            return Err(());
        }
        self.storage.rename(old_path, new_path).map_err(|_| ())
    }

    #[inline(never)]
    fn create_dir(&mut self, path: &str) -> Result<(), ()> {
        if path.starts_with("/sys/") {
            return Err(());
        }
        self.storage.create_dir(path).map_err(|_| ())
    }
}

// ── 5. Keypad Primitive ─────────────────────────────────────────────────────
impl<'a, 'hw> KeypadPrimitive for SysContext<'a, 'hw> {
    fn poll_key(&mut self) -> Option<u8> {
        drivers::keyboard::KeypadScanner::poll_matrix().map(|k| k as u8)
    }

    fn clear_key_buffer(&mut self) {
        drivers::keyboard::KeypadScanner::reset_gpio_pins();
    }

    fn is_key_pressed(&self, key_code: u8) -> bool {
        let mask = drivers::keyboard::KeypadScanner::poll_raw_mask();
        if let Some(kc) = kernel::event::KeyCode::from_u8(key_code) {
            (mask & drivers::keyboard::key_to_bit(kc)) != 0
        } else {
            false
        }
    }

    fn get_key_matrix(&self) -> u32 {
        drivers::keyboard::KeypadScanner::poll_raw_mask()
    }

    fn is_keypad_locked(&self) -> bool {
        self.is_keypad_locked
    }

    fn is_f_active(&self) -> bool {
        self.is_f_active
    }

    fn is_ptt_pressed(&self) -> bool {
        self.is_key_pressed(kernel::event::KeyCode::Ptt as u8)
    }

    fn is_side1_pressed(&self) -> bool {
        self.is_key_pressed(kernel::event::KeyCode::Side1 as u8)
    }

    fn is_side2_pressed(&self) -> bool {
        self.is_key_pressed(kernel::event::KeyCode::Side2 as u8)
    }
}

// ── 6. Power Primitive ──────────────────────────────────────────────────────
impl<'a, 'hw> PowerPrimitive for SysContext<'a, 'hw> {
    fn battery_mv(&self) -> u16 {
        self.power.battery_voltage_mv()
    }

    fn battery_pct(&self) -> u8 {
        self.power.battery_percentage()
    }

    fn battery_ma(&self) -> u16 {
        self.power.battery_current_ma()
    }

    fn battery_type(&self) -> u8 {
        self.power.battery_type() as u8
    }

    fn raw_battery_adc(&self) -> u16 {
        self.power.raw_battery_adc()
    }

    fn mcu_temp_deci_c(&self) -> i16 {
        self.power.mcu_temperature_deci_c()
    }

    fn is_charging(&self) -> bool {
        self.power.is_charging()
    }

    fn is_low(&self) -> bool {
        self.power.is_low()
    }

    fn is_critical(&self) -> bool {
        self.power.is_critical()
    }

    fn set_backlight(&mut self, mode: u8, timeout_s: u8) {
        if let Some(ref mut d) = self.display {
            let bmode = match mode {
                0 => kernel::service::BacklightMode::Off,
                1 => kernel::service::BacklightMode::Auto(((timeout_s as u16) * 1000).min(60000)),
                _ => kernel::service::BacklightMode::On,
            };
            d.set_backlight_mode(bmode);
        }
    }

    fn set_backlight_brightness(&mut self, max: u8, min: u8) {
        if let Some(ref mut d) = self.display {
            d.set_backlight_brightness(max, min);
        }
    }

    fn wake_backlight(&mut self) {
        if let Some(ref mut d) = self.display {
            d.wake();
        }
    }

    fn set_flashlight(&mut self, on: bool) {
        if on {
            bsp::pins::FLASHLIGHT.set_high();
        } else {
            bsp::pins::FLASHLIGHT.set_low();
        }
    }

    fn cpu_freq_hz(&self) -> u32 {
        py32_hal::rcc::sysclk_hz()
    }
}

// ── 7. Serial Primitive ─────────────────────────────────────────────────────
impl<'a, 'hw> SerialPrimitive for SysContext<'a, 'hw> {
    fn flush(&mut self) {
        py32_hal::uart::wait_tx_complete();
    }

    fn write(&mut self, data: &[u8]) {
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

    fn read(&mut self, buf: &mut [u8]) -> usize {
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

    fn is_usb_active(&self) -> bool {
        #[cfg(feature = "usb")]
        {
            self.usb.as_ref().map(|u| u.is_connected()).unwrap_or(false)
        }
        #[cfg(not(feature = "usb"))]
        {
            false
        }
    }
}

// ── 8. Timer Primitive ──────────────────────────────────────────────────────
impl<'a, 'hw> TimerPrimitive for SysContext<'a, 'hw> {
    fn uptime_secs(&self) -> u32 {
        py32_hal::delay::millis() / 1000
    }

    fn uptime_ms(&self) -> u32 {
        py32_hal::delay::millis()
    }

    fn delay_ms(&self, ms: u32) {
        py32_hal::delay::delay_ms(ms);
    }

    fn delay_us(&self, us: u32) {
        py32_hal::delay::delay_us(us);
    }

    fn get_rtc_time(&self) -> (u16, u8, u8, u8, u8, u8) {
        (2026, 1, 1, 0, 0, 0)
    }
}

// ── 9. UI Modal Primitive ───────────────────────────────────────────────────
impl<'a, 'hw> UiPrimitive for SysContext<'a, 'hw> {
    fn show_popup(&mut self, tag: u8, title: &'static str, message: &str) {
        self.show_popup(tag, title, message);
    }

    fn show_confirm(&mut self, tag: u8, title: &'static str, message: &'static str) {
        self.show_confirm(tag, title, message, "OK", "CANCEL");
    }

    fn prompt_text(&mut self, tag: u8, title: &'static str, initial: &str, max_len: usize) {
        self.prompt_text(tag, title, initial, max_len, false);
    }

    fn close_modal(&mut self) {
        self.close_modal();
    }

    fn has_active_modal(&self) -> bool {
        self.has_active_modal()
    }
}

// ── 10. Math Primitive ──────────────────────────────────────────────────────
impl<'a, 'hw> MathPrimitive for SysContext<'a, 'hw> {
    fn crc32(&self, data: &[u8]) -> u32 {
        crypto::crc::CrcEngine::crc32(data)
    }

    fn crc16(&self, data: &[u8]) -> u16 {
        crypto::crc::CrcEngine::crc16_ccitt(data)
    }

    fn rand32(&mut self) -> u32 {
        let seed = py32_hal::delay::millis()
            ^ ((self
                .radio
                .read_reg(drivers::bk4829::registers::REG_NOISE_VAL) as u32)
                << 16);
        crypto::crc::CrcEngine::crc32(&seed.to_le_bytes())
    }

    fn idivmod(&self, n: i32, d: i32) -> (i32, i32) {
        if d == 0 { (0, n) } else { (n / d, n % d) }
    }

    fn uidivmod(&self, n: u32, d: u32) -> (u32, u32) {
        if d == 0 { (0, n) } else { (n / d, n % d) }
    }
}

// ── 11. FM Broadcast Tuner Primitive ────────────────────────────────────────
impl<'a, 'hw> FmPrimitive for SysContext<'a, 'hw> {
    fn fm_tune(&mut self, freq_100khz: u16) {
        #[cfg(feature = "fm_radio")]
        {
            let i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let mut bk = drivers::bk1080::Bk1080::new(i2c);
            let _ = bk.set_frequency_10khz(freq_100khz.saturating_mul(10));
        }
        #[cfg(not(feature = "fm_radio"))]
        let _ = freq_100khz;
    }

    fn fm_set_volume(&mut self, volume: u8) {
        #[cfg(feature = "fm_radio")]
        {
            let i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let mut bk = drivers::bk1080::Bk1080::new(i2c);
            let _ = bk.set_volume(volume);
        }
        #[cfg(not(feature = "fm_radio"))]
        let _ = volume;
    }

    fn fm_mute(&mut self, mute: bool) {
        #[cfg(feature = "fm_radio")]
        {
            let i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
            let mut bk = drivers::bk1080::Bk1080::new(i2c);
            let _ = bk.set_mute(mute);
        }
        #[cfg(not(feature = "fm_radio"))]
        let _ = mute;
    }

    fn fm_get_rssi(&mut self) -> u8 {
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
}

// ── 12. Service & IPC Primitive ─────────────────────────────────────────────
impl<'a, 'hw> ServicePrimitive for SysContext<'a, 'hw> {
    fn post_event(&mut self, event_id: u32) -> bool {
        self.post_event(kernel::ipc::SysEvent {
            target: (event_id >> 16) as u8,
            command: (event_id & 0xFF) as u8,
            arg: ((event_id >> 8) & 0xFF) as u16,
        })
    }

    fn publish_event(&mut self, topic: u16, data: &[u8]) -> bool {
        let arg = if !data.is_empty() { data[0] as u16 } else { 0 };
        self.post_event(kernel::ipc::SysEvent {
            target: (topic >> 8) as u8,
            command: (topic & 0xFF) as u8,
            arg,
        })
    }

    fn get_stack_free_now(&self) -> u16 {
        kernel::sys::SystemDiagnostics::stack_margin_bytes() as u16
    }

    fn get_stack_free_min(&self) -> u16 {
        kernel::sys::SystemDiagnostics::ram_available_bytes() as u16
    }
}

#[inline(never)]
fn i2c_bus_op(addr: u8, reg: u8, op2: embedded_hal::i2c::Operation) -> Result<(), ()> {
    use embedded_hal::i2c::{I2c, Operation};
    let mut i2c = py32_hal::i2c::BitbangI2c::new(bsp::pins::FM_SCL, bsp::pins::FM_SDA);
    let reg_buf = [reg];
    let mut ops = [Operation::Write(&reg_buf), op2];
    i2c.transaction(addr, &mut ops).map_err(|_| ())
}

// ── 13. Low-Level Hardware & Bus Primitive ──────────────────────────────────
impl<'a, 'hw> HardwarePrimitive for SysContext<'a, 'hw> {
    fn i2c_read(&mut self, addr: u8, reg: u8, buf: &mut [u8]) -> Result<usize, ()> {
        i2c_bus_op(addr, reg, embedded_hal::i2c::Operation::Read(buf))?;
        Ok(buf.len())
    }

    fn i2c_write(&mut self, addr: u8, reg: u8, data: &[u8]) -> Result<(), ()> {
        i2c_bus_op(addr, reg, embedded_hal::i2c::Operation::Write(data))
    }

    fn spi_transfer(&mut self, cs_pin: u8, tx: &[u8], rx: &mut [u8]) -> Result<(), ()> {
        let is_flash = cs_pin == 0;
        if is_flash {
            bsp::pins::FLASH_CS.set_low();
        } else {
            bsp::pins::LCD_CS.set_low();
        }
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
        let _ = if is_flash {
            py32_hal::spi::spi2_wait_idle()
        } else {
            py32_hal::spi::wait_idle()
        };
        if is_flash {
            bsp::pins::FLASH_CS.set_high();
        } else {
            bsp::pins::LCD_CS.set_high();
        }
        Ok(())
    }

    fn gpio_read(&self, pin: u8) -> bool {
        match pin {
            0 => bsp::pins::FLASHLIGHT.is_high(),
            1 => bsp::pins::PTT.is_low(),
            2 => bsp::pins::AUDIO_PA.is_high(),
            3 => bsp::pins::BACKLIGHT.is_high(),
            4 => self.power.is_charging(),
            _ => false,
        }
    }

    fn gpio_write(&mut self, pin: u8, high: bool) {
        match pin {
            0 => self.set_flashlight(high),
            2 => {
                if high {
                    bsp::pins::AUDIO_PA.set_high();
                } else {
                    bsp::pins::AUDIO_PA.set_low();
                }
            }
            3 => {
                if high {
                    bsp::pins::BACKLIGHT.set_high();
                } else {
                    bsp::pins::BACKLIGHT.set_low();
                }
            }
            _ => {}
        }
    }

    fn raw_flash_read(&mut self, addr: u32, buf: &mut [u8]) -> Result<(), ()> {
        bsp::pins::FLASH_CS.set_low();
        let cmd = [
            0x03,
            (addr >> 16) as u8,
            (addr >> 8) as u8,
            (addr & 0xFF) as u8,
        ];
        for &b in &cmd {
            py32_hal::spi::spi2_transfer(b).map_err(|_| ())?;
        }
        for slot in buf.iter_mut() {
            *slot = py32_hal::spi::spi2_transfer(0x00).map_err(|_| ())?;
        }
        let _ = py32_hal::spi::spi2_wait_idle();
        bsp::pins::FLASH_CS.set_high();
        Ok(())
    }
}

// ── 14. Radio Coordinator Service Trait ─────────────────────────────────────
impl<'a, 'hw> kernel::service::radio::RadioCoordinator for SysContext<'a, 'hw> {
    fn active_vfo(&self) -> kernel::service::radio::VfoSlot {
        self.state.coord.active_vfo
    }

    fn listening_vfo(&self) -> kernel::service::radio::VfoSlot {
        self.state.coord.listening_vfo
    }

    fn select_vfo(&mut self, slot: kernel::service::radio::VfoSlot) {
        self.state.coord.select_vfo(self.radio, slot);
    }

    fn toggle_vfo(&mut self) {
        self.state.coord.toggle_vfo(self.radio);
    }

    fn step_vfo_frequency(&mut self, slot: kernel::service::radio::VfoSlot, delta_hz: i32) {
        self.state.coord.step_vfo_freq(self.radio, slot, delta_hz);
    }

    fn toggle_reverse(&mut self, slot: kernel::service::radio::VfoSlot) {
        self.state.coord.toggle_reverse(self.radio, slot);
    }

    fn is_transmitting(&self) -> bool {
        self.state.coord.is_transmitting
    }

    fn is_carrier_active(&self) -> bool {
        self.state.coord.carrier_active
    }

    fn active_channel(&self) -> &kernel::service::radio::ChannelConfig {
        self.state.coord.active_channel()
    }

    fn listening_channel(&self) -> &kernel::service::radio::ChannelConfig {
        self.state.coord.listening_channel()
    }

    fn request_ptt_start(&mut self) -> kernel::service::radio::TxPermission {
        self.state.coord.request_ptt_start(
            self.radio,
            kernel::service::radio::ClientToken::new(
                kernel::service::radio::ClientId::VFO_UI,
                kernel::service::radio::LockTier::FOREGROUND,
            ),
        )
    }

    fn request_ptt_start_client(
        &mut self,
        client: kernel::service::radio::ClientToken,
    ) -> kernel::service::radio::TxPermission {
        self.state.coord.request_ptt_start(self.radio, client)
    }

    fn request_ptt_stop(&mut self) {
        if self.state.coord.is_transmitting {
            self.state.coord.request_ptt_stop(self.radio);
        }
    }

    fn request_vfo_lock(
        &mut self,
        client: kernel::service::radio::ClientToken,
        slot: kernel::service::radio::VfoSlot,
        timeout_ms: u16,
        mute_audio: bool,
    ) -> bool {
        self.state
            .coord
            .request_vfo_lock(client, slot, timeout_ms, mute_audio)
    }

    fn release_vfo_lock(&mut self, client: kernel::service::radio::ClientToken) -> bool {
        self.state.coord.release_vfo_lock(client)
    }

    fn is_locked(&self) -> bool {
        self.state.coord.is_vfo_locked()
    }
}
