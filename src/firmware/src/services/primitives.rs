//! Shared Microkernel Service Primitives & Radio/Bus Helpers.
//!
//! Stateless hardware/radio/storage helpers shared across KRX syscalls, RigWire,
//! and built-in apps, plus minimal VFO convenience methods on `SysContext`.

use crate::services::SysContext;
use crate::services::radio::RadioCoordinator;
use drivers::bk4829::{AfType, Bk4829, Disabled};
use kernel::service::PowerService;
use kernel::service::radio::{
    Bandwidth, ChannelConfig, ClientId, ClientToken, LockTier, ModulationMode, RfChannel,
    RfModulation, RfPowerLevel, RfSquelch, RfSubaudible, RfToneGenerator, StepSetting,
    SubaudibleTone, TxPermission, VfoSlot,
};
#[cfg(feature = "rf_fsk")]
use kernel::service::radio::{FskBaudRate, FskPhyConfig};

#[inline(always)]
pub fn color_from_u8(color: u8) -> gfx::Color {
    gfx::Color::from_u8(color)
}

#[inline(always)]
pub fn feed_watchdog() {
    py32_hal::wdt::Watchdog::feed();
}

#[inline(always)]
pub fn mcu_rdp_short_str() -> &'static str {
    bsp::uid::get_mcu_rdp_level().short_str()
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
    static ROW_WORDS: [u16; 4] = [7196, 7950, 8796, 9715];
    static COL_WORDS: [u16; 4] = [12482, 13793, 15249, 16860];
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
fn app_config_path_into<'a>(app_name: &str, buf: &'a mut [u8; 32]) -> &'a str {
    const PFX: &[u8] = b"/pref/app/";
    const SFX: &[u8] = b".cfg";
    let n = app_name.len().min(buf.len() - PFX.len() - SFX.len());
    buf[..PFX.len()].copy_from_slice(PFX);
    buf[PFX.len()..PFX.len() + n].copy_from_slice(&app_name.as_bytes()[..n]);
    buf[PFX.len() + n..PFX.len() + n + SFX.len()].copy_from_slice(SFX);
    unsafe { core::str::from_utf8_unchecked(&buf[..PFX.len() + n + SFX.len()]) }
}

#[inline(never)]
pub fn load_app_config(
    storage: &mut crate::services::SystemStorage,
    app_name: &str,
    buf: &mut [u8],
) -> Result<usize, ()> {
    let mut pbuf = [0u8; 32];
    storage
        .read_file(app_config_path_into(app_name, &mut pbuf), buf)
        .map_err(|_| ())
}

#[inline(never)]
pub fn save_app_config(
    storage: &mut crate::services::SystemStorage,
    app_name: &str,
    data: &[u8],
) -> Result<(), ()> {
    let mut pbuf = [0u8; 32];
    crate::security::write_classified_file(
        storage,
        app_config_path_into(app_name, &mut pbuf),
        data,
    )
    .map_err(|_| ())
}

#[inline(never)]
pub fn list_krx_dir_at(
    storage: &mut crate::services::SystemStorage,
    dir_path: &str,
    skip: usize,
    out_entries: &mut [kernel::proc::abi::KrxDirEntry],
) -> Result<(usize, usize), ()> {
    let mut total = 0usize;
    let mut written = 0usize;
    storage
        .for_each_entry(dir_path, |path, size, _perm, is_dir| {
            if total >= skip {
                if let Some(dst) = out_entries.get_mut(written) {
                    unsafe { core::ptr::write_bytes(dst, 0, 1) };
                    let b = path.as_bytes();
                    let n = b.len().min(24);
                    dst.name[..n].copy_from_slice(&b[..n]);
                    dst.size = size as u32;
                    dst.is_dir = u8::from(is_dir);
                    written += 1;
                }
            }
            total += 1;
        })
        .map_err(|_| ())?;
    Ok((total, written))
}

#[inline(always)]
pub fn list_krx_dir(
    storage: &mut crate::services::SystemStorage,
    dir_path: &str,
    out_entries: &mut [kernel::proc::abi::KrxDirEntry],
) -> Result<usize, ()> {
    list_krx_dir_at(storage, dir_path, 0, out_entries).map(|(_, w)| w)
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

#[inline(never)]
pub fn hw_spi_transfer(cs_pin: u8, tx: &[u8], rx: &mut [u8]) -> Result<(), ()> {
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

#[inline(never)]
pub fn hw_gpio_read(pin: u8) -> bool {
    if let Some(&p) = HW_GPIO_PINS.get(pin as usize) {
        p.is_high() ^ (pin == 1)
    } else if pin == 4 {
        unsafe { (*core::ptr::addr_of!(crate::POWER_SERVICE)).is_charging() }
    } else {
        false
    }
}

#[inline(never)]
pub fn hw_gpio_write(pin: u8, high: bool) {
    if matches!(pin, 0 | 2 | 3) {
        let p = HW_GPIO_PINS[pin as usize];
        if high {
            p.set_high();
        } else {
            p.set_low();
        }
    }
}

// ── Shared Radio Operations ─────────────────────────────────────────────────

#[inline(never)]
pub fn rf_apply_subaudible_split(
    coord: &mut RadioCoordinator,
    radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>,
    set_rx: bool,
    set_tx: bool,
    tone: SubaudibleTone,
) {
    {
        let ch = coord.active_channel_mut();
        if set_rx {
            ch.rx_subaudible = tone;
        }
        if set_tx {
            ch.tx_subaudible = tone;
        }
    }
    if set_rx && !coord.is_transmitting {
        radio.set_rx_subaudible(tone);
    }
    if set_tx {
        radio.set_tx_subaudible(tone);
    }
}

#[inline(never)]
pub fn rf_tune(
    coord: &mut RadioCoordinator,
    radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>,
    freq_hz: u32,
    bandwidth: Bandwidth,
    step: StepSetting,
) {
    let slot = coord.active_vfo;
    {
        let ch = coord.vfo_channel_mut(slot);
        ch.rx_freq = utils::units::Frequency::new(freq_hz);
        ch.bandwidth = bandwidth;
        ch.step = step;
    }
    if !coord.is_transmitting {
        coord.select_vfo(radio, slot);
    }
}

#[inline(never)]
pub fn rf_set_param(
    coord: &mut RadioCoordinator,
    radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>,
    op: u8,
    val: u8,
) {
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
    match op {
        0 => {
            let mode = ModulationMode::from_u8(val);
            coord.active_channel_mut().modulation = mode;
            if !coord.is_transmitting {
                RfModulation::set_modulation(radio, mode);
            }
        }
        1 => {
            let sql = val.min(9);
            coord.active_channel_mut().squelch_level = sql;
            if !coord.is_transmitting {
                radio.set_squelch_level(sql);
            }
        }
        2 => {
            let pwr = RfPowerLevel::from_u8(val);
            coord.active_channel_mut().power = pwr;
            if !coord.is_transmitting {
                radio.set_power_level(pwr);
            }
        }
        3 => radio.set_af(AF_LUT.get(val as usize).copied().unwrap_or(AfType::Mute)),
        4 => radio.mute_mic(val != 0),
        _ => radio.set_agc(val != 0),
    }
}

#[inline(never)]
pub fn rf_sweep(
    radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>,
    start_hz: u32,
    step_hz: u32,
    out_rssi: &mut [i16],
) -> usize {
    let count = out_rssi.len().min(128);
    let mut freq = start_hz;
    for slot in &mut out_rssi[..count] {
        radio.set_rx_frequency(utils::units::Frequency::new(freq));
        *slot = radio.get_rssi_dbm();
        freq = freq.saturating_add(step_hz);
    }
    count
}

#[inline(never)]
pub fn rf_emit_symbols(
    radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>,
    symbols: &[kernel::proc::abi::KrxSymbol],
) -> Result<(), ()> {
    for sym in symbols {
        radio.write_reg(drivers::bk4829::registers::REG_TONE1_FREQ, sym.freq_word);
        py32_hal::delay::delay_us(sym.duration_us as u32);
    }
    Ok(())
}

#[inline(never)]
pub fn rf_set_dual_tones_hz(
    radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>,
    tone1_hz: u16,
    tone1_gain: u8,
    tone2_hz: u16,
    tone2_gain: u8,
) {
    use kernel::service::radio::tone_gen::calc_tone_word;
    radio.set_dual_tones_raw(
        calc_tone_word(tone1_hz),
        tone1_gain,
        calc_tone_word(tone2_hz),
        tone2_gain,
    );
}

#[inline(never)]
pub fn rf_set_dtmf(radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>, digit: u8, gain: u8) {
    let (w1, w2) = dtmf_tone_words(digit);
    radio.set_dual_tones_raw(w1, gain, w2, gain);
}

#[cfg(feature = "rf_fsk")]
pub fn rf_fsk_init(radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>, baud: u16) {
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
    let _ = crate::services::radio::fsk_stream::FskStreamService::arm_rx(radio, &config);
}

#[cfg(feature = "rf_fsk")]
pub fn rf_fsk_send(radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>, data: &[u8]) -> Result<(), ()> {
    let mut words = [0u16; 64];
    let limit = data.len().div_ceil(2).min(words.len());
    for i in 0..limit {
        let b0 = data[i * 2] as u16;
        let b1 = data.get(i * 2 + 1).copied().unwrap_or(0) as u16;
        words[i] = (b0 << 8) | b1;
    }
    crate::services::radio::fsk_stream::FskStreamService::transmit_words(
        radio,
        &words[..limit],
        3,
        500,
    )
}

#[cfg(feature = "rf_fsk")]
pub fn rf_fsk_receive(radio: &mut Bk4829<'_, bsp::RadioBus, Disabled>, buf: &mut [u8]) -> usize {
    let mut words = [0u16; 32];
    let count =
        crate::services::radio::fsk_stream::FskStreamService::poll_and_read(radio, &mut words);
    let mut out_len = 0;
    for &w in &words[..count] {
        if out_len < buf.len() {
            buf[out_len] = (w >> 8) as u8;
            out_len += 1;
        }
        if out_len < buf.len() {
            buf[out_len] = (w & 0xFF) as u8;
            out_len += 1;
        }
    }
    out_len
}

// ── VFO Convenience Methods on SysContext ───────────────────────────────────

impl<'a, 'hw> SysContext<'a, 'hw> {
    pub fn load_config(&mut self, app_name: &str, buf: &mut [u8]) -> Result<usize, ()> {
        load_app_config(self.storage, app_name, buf)
    }

    pub fn save_config(&mut self, app_name: &str, data: &[u8]) -> Result<(), ()> {
        save_app_config(self.storage, app_name, data)
    }

    pub fn active_vfo(&self) -> VfoSlot {
        self.state.coord.active_vfo
    }

    pub fn select_vfo(&mut self, slot: VfoSlot) {
        self.state.coord.select_vfo(self.radio, slot);
    }

    pub fn toggle_vfo(&mut self) {
        self.state.coord.toggle_vfo(self.radio);
    }

    pub fn step_vfo_frequency(&mut self, slot: VfoSlot, delta_hz: i32) {
        self.state.coord.step_vfo_freq(self.radio, slot, delta_hz);
    }

    pub fn toggle_reverse(&mut self, slot: VfoSlot) {
        self.state.coord.toggle_reverse(self.radio, slot);
    }

    pub fn is_transmitting(&self) -> bool {
        self.state.coord.is_transmitting
    }

    pub fn is_carrier_active(&self) -> bool {
        self.state.coord.carrier_active
    }

    pub fn active_channel(&self) -> &ChannelConfig {
        self.state.coord.active_channel()
    }

    pub fn request_ptt_start(&mut self) -> TxPermission {
        self.state.tone_seq.abort(self.radio);
        self.state.coord.request_ptt_start(
            self.radio,
            ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND),
        )
    }

    pub fn request_ptt_stop(&mut self) {
        if self.state.coord.is_transmitting {
            self.state.coord.request_ptt_stop(self.radio);
        }
    }

    pub fn toggle_monitor(&mut self) -> bool {
        self.state.coord.toggle_monitor(self.radio)
    }

    /// System-wide PTT key event handler (`Momentary`, `Toggle`, `DoublePressLatch`),
    /// including acoustic BCLO/TOT/battery refusal warnings and post-TX roger beep playback.
    #[inline(never)]
    pub fn handle_system_ptt(&mut self, state: kernel::event::KeyState) {
        use crate::services::radio::{PttEventOutcome, PttMode};
        use kernel::event::KeyState;

        let pressed = match state {
            KeyState::Pressed => true,
            KeyState::Released => false,
            KeyState::Held => return,
        };
        let mode = PttMode::from_u8(self.config.keys.ptt_mode);
        let now_ms = self.millis();
        if pressed {
            self.state.tone_seq.abort(self.radio);
        }
        let client = ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND);
        match self
            .state
            .coord
            .handle_ptt_key(self.radio, mode, pressed, now_ms, client)
        {
            PttEventOutcome::Started(perm) if !perm.is_granted() => {
                if matches!(
                    perm,
                    TxPermission::BlockedBusyChannel
                        | TxPermission::BlockedTotPenalty
                        | TxPermission::BlockedBatteryCritical
                        | TxPermission::BlockedTxInhibited
                ) && self.config.audio.key_beep != 0
                {
                    self.play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
                }
            }
            PttEventOutcome::Stopped => {
                if self.config.audio.roger_beep != 0 {
                    self.play_active_roger();
                }
            }
            _ => {}
        }
    }
}
