pub mod calibration;
#[cfg(all(feature = "filesystem", feature = "i18n"))]
pub mod lang;
#[cfg(not(all(feature = "filesystem", feature = "i18n")))]
pub mod lang {
    #[inline(always)]
    pub fn zone_a_end_bytes() -> usize {
        0
    }
    #[inline(always)]
    pub fn reset_arena_cache() {}
    #[inline(always)]
    pub fn reload_os<D: kernfs::BlockDevice>(
        _storage: &mut kernfs::KernFS<'_, D>,
        _sys: &crate::config::SystemConfig,
    ) {
    }
    #[inline(always)]
    pub fn bind_app<D: kernfs::BlockDevice>(_storage: &mut kernfs::KernFS<'_, D>, _path: &str) {}
    #[inline(always)]
    pub fn unbind_app() {}
    #[cfg(feature = "text_input")]
    #[inline(always)]
    pub fn load_locale_keymap<D: kernfs::BlockDevice, T>(
        _storage: &mut kernfs::KernFS<'_, D>,
        _km: &mut T,
    ) {
    }
}
pub mod melody;
pub mod power;
pub mod radio;
pub mod telemetry;
#[cfg(feature = "wire")]
pub mod wire;

pub use crate::data::arena::staging_buf;
pub use calibration::*;
pub use melody::*;
pub use power::*;
pub use radio::*;
pub use telemetry::*;
#[cfg(feature = "wire")]
pub use wire::*;

pub use drivers::battery::SystemPower;
pub use kernfs::{
    ATTR_COMPRESSED, ATTR_ENCRYPTED, ATTR_HIDDEN, ATTR_READ_ONLY, ATTR_SYSTEM, ATTR_UNENCRYPTED,
    Fd, FsError, KernFS,
};

#[inline(always)]
pub fn config() -> &'static crate::config::MasterConfig {
    crate::master_config_ref()
}

#[inline(always)]
pub fn config_mut() -> &'static mut crate::config::MasterConfig {
    crate::master_config()
}

#[inline(always)]
pub fn storage_mut()
-> &'static mut KernFS<'static, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>> {
    crate::kern_fs()
}

#[inline(always)]
pub fn state_and_radio() -> (
    &'static mut RadioState,
    &'static mut drivers::bk4829::Bk4829<'static, bsp::RadioBus>,
) {
    (crate::radio_state(), &mut crate::kern_hw().radio)
}

#[inline(always)]
pub fn power() -> &'static SystemPower {
    crate::power_service()
}

/// Enqueue a high-level `SysEvent` onto the unified lock-free system `EventRing`.
#[inline(always)]
pub fn post_event(event: kernel::ipc::SysEvent) -> bool {
    crate::system::enqueue_event(kernel::event::Event::Sys(event))
}

/// Show an OS modal popup dialog on top of the active application.
#[inline(always)]
pub fn show_popup(tag: u8, title: &str, message: &str) {
    crate::orchestrator::compositor::active_modal_mut().show_popup(tag, title, message);
}

/// Show an OS modal confirmation dialog with Confirm/Cancel badges.
#[inline(always)]
pub fn show_confirm(
    tag: u8,
    title: &str,
    message: &str,
    confirm: &'static str,
    cancel: &'static str,
) {
    crate::orchestrator::compositor::active_modal_mut()
        .show_confirm(tag, title, message, confirm, cancel);
}

#[inline(always)]
pub fn wake_display() {
    #[cfg(not(test))]
    crate::kern_hw().backlight.wake();
}

/// Play single keypress or system feedback beep (non-blocking via `ToneSequencer`).
pub fn play_beep(beep: BeepType) {
    let (state, radio) = state_and_radio();
    state
        .tone_seq
        .enqueue(radio, melody::MelodyEngine::beep_notes(beep));
}

/// Play a system dialog / notification sound (non-blocking via `ToneSequencer`).
pub fn play_dialog_sound(sound: melody::DialogSoundType) {
    if config().audio.volume > 0 {
        let (state, radio) = state_and_radio();
        state
            .tone_seq
            .enqueue(radio, melody::MelodyEngine::dialog_notes(sound));
    }
}

/// Play roger beep melody from file path with automatic fallback (non-blocking).
pub fn play_roger_path(path: &str) {
    let (state, radio) = state_and_radio();
    state
        .tone_seq
        .enqueue_roger_file(radio, storage_mut(), path);
}

/// Play currently configured active roger beep (non-blocking).
pub fn play_active_roger() {
    let audio = &config().audio;
    if audio.roger_beep != 0 {
        let mut pbuf = [0u8; 32];
        let ps = audio.roger_path_str();
        let n = ps.len().min(32);
        pbuf[..n].copy_from_slice(&ps.as_bytes()[..n]);
        let path = unsafe { core::str::from_utf8_unchecked(&pbuf[..n]) };
        play_roger_path(path);
    }
}

/// Play customized frequency tone note (non-blocking via `ToneSequencer`).
pub fn play_tone(freq_hz: u16, duration_ms: u16) {
    let note = [melody::ToneNote::new(freq_hz, duration_ms)];
    let (state, radio) = state_and_radio();
    state.tone_seq.enqueue(radio, &note);
}

/// System-wide PTT key event handler (`Momentary`, `Toggle`, `DoublePressLatch`),
/// including acoustic BCLO/TOT/battery refusal warnings and post-TX roger beep playback.
pub fn handle_system_ptt(state: kernel::event::KeyState) {
    use crate::services::radio::{PttEventOutcome, PttMode};
    use kernel::event::KeyState;
    use kernel::service::radio::{ClientId, ClientToken, LockTier, TxPermission};

    let pressed = match state {
        KeyState::Pressed => true,
        KeyState::Released => false,
        KeyState::Held => return,
    };
    let cfg = config();
    let mode = PttMode::from_u8(cfg.keys.ptt_mode);
    let key_beep = cfg.audio.key_beep;
    let roger_beep = cfg.audio.roger_beep;
    let now_ms = py32_hal::delay::millis();
    let client = ClientToken::new(ClientId::VFO_UI, LockTier::FOREGROUND);
    let outcome = {
        let (st, radio) = state_and_radio();
        if pressed {
            st.tone_seq.abort(radio);
        }
        st.coord
            .handle_ptt_key(radio, mode, pressed, now_ms, client)
    };
    match outcome {
        PttEventOutcome::Started(perm) if !perm.is_granted() => {
            if matches!(
                perm,
                TxPermission::BlockedBusyChannel
                    | TxPermission::BlockedTotPenalty
                    | TxPermission::BlockedBatteryCritical
                    | TxPermission::BlockedTxInhibited
            ) && key_beep != 0
            {
                play_dialog_sound(melody::DialogSoundType::Warning);
            }
        }
        PttEventOutcome::Stopped => {
            if roger_beep != 0 {
                play_active_roger();
            }
        }
        _ => {}
    }
}

pub fn draw_status_bar_right_mode(canvas: &mut gfx::Canvas, inverted: bool, partial: bool) -> i16 {
    let headless = crate::system::is_headless();
    #[cfg(test)]
    let (is_usb, is_f, is_locked) = (false, false, false);
    #[cfg(not(test))]
    let (is_usb, is_f, is_locked) = {
        #[cfg(feature = "usb")]
        let usb_conn = !headless && crate::hardware::is_active_usb_connected();
        #[cfg(not(feature = "usb"))]
        let usb_conn = false;
        let orch = &crate::kern_hw().orchestrator;
        (usb_conn, orch.is_f_active(), orch.is_keypad_locked())
    };

    let serial_active = !headless && py32_hal::uart::is_active(800);
    let anim_step = ((py32_hal::delay::millis() >> 8) & 0xFF) as u8;
    let extra_flags = ((serial_active as u8) << 3)
        | ((is_f as u8) << 4)
        | ((is_locked as u8) << 5)
        | ((inverted as u8) << 6)
        | ((partial as u8) << 7);
    let cfg = config();

    draw_status_bar_right_raw(
        canvas,
        power(),
        cfg.battery.battery_format,
        cfg.display.shading_mode,
        anim_step,
        is_usb,
        extra_flags,
        crate::orchestrator::status_tray().active_glyphs(),
    )
}

/// Renders a fully declarative, modular, auto-justified status bar.
pub fn draw_status_bar_cfg(canvas: &mut gfx::Canvas, config: &kernel::app::StatusBarConfig<'_>) {
    let right_limit = draw_status_bar_right_mode(canvas, false, config.partial);
    if !config.partial {
        gfx::views::chrome::draw_status_bar_layout(canvas, config, right_limit);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn draw_status_bar_right_raw(
    canvas: &mut gfx::Canvas,
    pwr: &drivers::battery::SystemPower,
    battery_format: u8,
    shading_mode: u8,
    anim_step: u8,
    is_usb: bool,
    extra_flags: u8,
    dynamic_glyphs: &[gfx::icons::StatusGlyph],
) -> i16 {
    let pct = pwr.battery_percentage();
    let flags = extra_flags
        | (pwr.is_charging() as u8)
        | (((pwr.is_low() || pct == 0) as u8) << 1)
        | ((is_usb as u8) << 2);
    gfx::views::StatusRightConfig {
        percentage: pct,
        battery_format,
        shading_mode,
        anim_step,
        flags,
        voltage_mv: pwr.battery_voltage_mv(),
        temp_deci_c: pwr.mcu_temperature_deci_c(),
        dynamic_glyphs,
    }
    .render(canvas)
}

#[cfg(feature = "passcode")]
#[inline(always)]
pub fn draw_boot_status_right(canvas: &mut gfx::Canvas, battery_format: u8, is_usb: bool) -> i16 {
    let pwr = unsafe { &*core::ptr::addr_of!(crate::POWER_SERVICE) };
    draw_status_bar_right_raw(
        canvas,
        pwr,
        battery_format,
        crate::master_config_ref().display.shading_mode,
        0,
        is_usb && !crate::system::is_headless(),
        gfx::views::chrome::STATUS_FLAG_INVERTED,
        &[],
    )
}

/// Composite hardware capability token set exposed by this firmware build.
pub const fn system_caps() -> ::abi::CapabilitySet {
    const BASE_CAPS: ::abi::CapabilitySet = ::abi::CapabilitySet::NONE
        .with(::abi::Cap::RadioRx)
        .with(::abi::Cap::RadioTx)
        .with(::abi::Cap::StorageRead)
        .with(::abi::Cap::StorageWrite)
        .with(::abi::Cap::AudioOutput)
        .with(::abi::Cap::DisplayDirect)
        .with(::abi::Cap::Keypad)
        .with(::abi::Cap::AudioCapture)
        .with(::abi::Cap::TaskControl)
        .with(::abi::Cap::ServiceRegistry)
        .with(::abi::Cap::SerialComm)
        .with(::abi::Cap::UsbComm)
        .with(::abi::Cap::HardwareCrc)
        .with(::abi::Cap::RealTimeClock)
        .with(::abi::Cap::PowerManage)
        .with(::abi::Cap::FactoryCalib)
        .with(::abi::Cap::Preferences);

    #[allow(unused_mut)]
    let mut c = BASE_CAPS;
    #[cfg(feature = "filesystem")]
    {
        c = c.with(::abi::Cap::StorageAdmin);
    }
    #[cfg(feature = "fm_radio")]
    {
        c = c.with(::abi::Cap::BroadcastFm);
    }
    #[cfg(feature = "secure_fs")]
    {
        c = c.with(::abi::Cap::CryptoAead);
    }
    c
}
