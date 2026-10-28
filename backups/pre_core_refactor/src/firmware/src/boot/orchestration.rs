//! Hardware Initialization, Reset Cause Introspection & Boot Orchestration.

use crate::config::MasterConfig;
use crate::services::SystemStorage;
use crate::services::power::PowerServiceConfigExt;
use crate::{CALIBRATION, PASSCODE_STATE, POWER_SERVICE};
use drivers::audio::AudioAmplifier;
use drivers::backlight::BacklightController;
use drivers::bk4829::Bk4829;
use drivers::keyboard::KeypadScanner;
use drivers::st7565::St7565;
use py32_hal::adc::Adc;

/// Encapsulated peripheral and subsystem hardware context for the boot pipeline.
pub struct BootHardware<'a, 'b> {
    pub lcd: &'a mut St7565<'b>,
    pub backlight: &'a mut BacklightController,
    pub key_scanner: &'a mut KeypadScanner,
    pub audio: &'a mut AudioAmplifier,
    pub radio: &'a mut Bk4829<'b, drivers::bk4829::Disabled>,
    pub vfs: &'a mut SystemStorage<'b>,
    #[cfg(feature = "usb")]
    pub usb: &'a mut drivers::usb::UsbDriver,
}

pub fn check_reset_cause() {
    let reset_cause = py32_hal::wdt::Watchdog::reset_cause();
    let is_watchdog_reset = matches!(
        reset_cause,
        py32_hal::wdt::ResetCause::IndependentWatchdog | py32_hal::wdt::ResetCause::WindowWatchdog
    );
    py32_hal::wdt::Watchdog::clear_reset_flags();

    if is_watchdog_reset {
        let slot = crate::panic::BOOT_RECORD.get_mut().begin_write();
        slot.reset_reason = kernel::boot::ResetReason::Wdog as u32;
        slot.set_actor(b"WATCHDOG");
        crate::panic::BOOT_RECORD.get_mut().commit_write();
    }
}

#[inline(never)]
pub fn run_boot_splash(disp_cfg: &crate::config::DisplayConfig, hw: &mut BootHardware) {
    let target_splash_ms: u32 = match disp_cfg.boot_splash_duration {
        0 => 0,
        1 => 500,
        2 => 1000,
        3 => 2000,
        4 => 3000,
        _ => 1000,
    };
    if target_splash_ms == 0 || disp_cfg.boot_splash_mode == 0 {
        hw.lcd.turn_on();
        hw.backlight.wake();
        return;
    }

    let canvas = crate::system_canvas();
    hw.lcd.turn_on();
    hw.backlight.wake();

    // 16 animation frames spread evenly across the user-configured duration.
    // Frame N is shown at start + N * duration / 16; frame 16 (full logo) is held last.
    const FRAMES: u32 = 16;
    let start_ms = py32_hal::delay::millis();
    let first = if disp_cfg.boot_splash_anim != 0 {
        1
    } else {
        FRAMES
    };

    for frame in first..=FRAMES {
        bsp::Board::wdt_feed();

        super::splash::render_splash_frame(canvas, hw.vfs, disp_cfg, frame as u8);
        canvas.mark_all_dirty();
        hw.lcd.blit(canvas);

        // Absolute deadline so render cost never stretches total duration
        let deadline = (target_splash_ms * frame) / FRAMES;
        let elapsed = py32_hal::delay::millis().wrapping_sub(start_ms);
        let wait = deadline.saturating_sub(elapsed);
        if wait > 0 {
            #[cfg(feature = "usb")]
            let key = super::boot_wait_key_with_usb(hw.key_scanner, hw.usb, wait);
            #[cfg(not(feature = "usb"))]
            let key = super::boot_wait_key(hw.key_scanner, wait);
            if key.is_some() {
                return;
            }
        }
    }
}

#[inline(never)]
pub fn init_system_services(hw: &mut BootHardware, master_cfg: &MasterConfig, adc: &Adc) {
    let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
    power_service.load_battery_config(&master_cfg.battery);

    hw.key_scanner.configure(
        master_cfg.keys.hold_delay_ms(),
        master_cfg.keys.repeat_rate_ms(),
        master_cfg.keys.fast_scroll != 0,
    );

    let mcu_uid = bsp::uid::read_mcu_uid();
    let calibration = unsafe { &mut *core::ptr::addr_of_mut!(CALIBRATION) };
    calibration.load_into(hw.vfs, &mcu_uid);
    power_service.battery_driver.calib_const = calibration.battery_calib_const();
    hw.radio.set_xtal_freq_trim(calibration.xtal_freq_low());

    crate::services::telemetry::sample_battery_and_system(adc, false);

    crate::apps::init_all_apps();
}

#[inline(never)]
fn boot_stage_security_check(
    hw: &mut BootHardware,
    master_cfg: &mut MasterConfig,
    passcode_state: &mut crate::security::PasscodeState,
    adc: &Adc,
) {
    let crash_slot = crate::panic::BOOT_RECORD.get().latest_valid_slot().copied();
    let crash_count = crash_slot.as_ref().map_or(0, |s| s.crash_count);

    if crash_count >= 2 {
        if let Some(ref slot) = crash_slot {
            super::recovery::run_safe_mode_recovery(hw, slot, master_cfg);
        }
    } else if cfg!(feature = "passcode") && crate::security::passcode::is_passcode_set(hw.vfs) {
        passcode_state.lock();
        let pcfg = crate::security::passcode::get_passcode_config(hw.vfs);
        let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
        power_service.load_battery_config(&master_cfg.battery);
        crate::services::telemetry::sample_battery_and_system(adc, false);

        // Configure LCD parameters (contrast, inversion, shading) without waking the backlight
        crate::hardware::apply_lcd_hardware(&master_cfg.display, hw.lcd);

        if pcfg.stealth {
            // Stealth / Dimmed mode: ensure backlight remains strictly OFF during boot and passcode entry
            hw.backlight
                .set_mode(drivers::backlight::BacklightMode::Off);
            hw.backlight.turn_off();
        } else {
            master_cfg.display.apply_to_backlight(hw.backlight);
            hw.backlight.wake();
        }

        super::security::run_boot_unlock_prompt(hw, passcode_state, master_cfg, adc);
    } else if !cfg!(feature = "passcode") && crate::security::passcode::is_passcode_set(hw.vfs) {
        super::queue_alert("Storage Locked", "Passcode enabled in storage");
    }
}

#[inline(never)]
fn boot_stage_hw_init(
    hw: &mut BootHardware,
    master_cfg: &MasterConfig,
    passcode_state: &crate::security::PasscodeState,
    adc: &Adc,
) {
    let _ = hw.radio.init();

    hw.audio.init();
    crate::hardware::apply_audio_hardware(&master_cfg.audio, hw.audio, hw.radio);
    if master_cfg.audio.boot_jingle != 0 {
        let jingle = crate::services::melody::BootJingleType::from_u8(master_cfg.audio.boot_jingle);
        let notes = crate::services::melody::MelodyEngine::boot_jingle_notes(jingle);
        if !notes.is_empty() {
            hw.radio.play_tone_sequence(notes);
        }
    }

    if master_cfg.battery.cpu_clock == 1 {
        py32_hal::rcc::set_sysclk_72mhz();
    } else {
        py32_hal::rcc::set_sysclk_48mhz();
    }

    super::ensure_default_files(hw.vfs, Some(hw.lcd));

    if !passcode_state.is_locked {
        let mut ps = unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) };
        crate::security::passcode::init_passcode_state(hw.vfs, &mut ps);
    }

    bsp::pins::FLASHLIGHT.set_low();
    init_system_services(hw, master_cfg, adc);

    #[cfg(feature = "usb")]
    crate::hardware::SystemHardware::sync_usb_telemetry(hw.usb, master_cfg);
}

#[inline(never)]
pub fn boot_orchestration(hw: &mut BootHardware) -> Adc {
    let master_cfg = crate::master_config();
    let passcode_state = unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) };
    *passcode_state = crate::security::PasscodeState::new();

    let adc = py32_hal::adc::Adc::init();

    #[cfg(feature = "usb")]
    {
        hw.usb.init();
    }

    master_cfg.load_into(hw.vfs);

    // ── STAGE 1: Instant Security & Recovery Introspection (<30ms) ──
    boot_stage_security_check(hw, master_cfg, passcode_state, &adc);

    // ── STAGE 2: Display Configuration ──────────────────────────────
    if !passcode_state.is_locked {
        crate::hardware::apply_display_hardware(&master_cfg.display, hw.lcd, hw.backlight);
    }

    // ── STAGE 3: Hardware Initialization (Concurrent with Jingle) ───
    boot_stage_hw_init(hw, master_cfg, passcode_state, &adc);

    // ── STAGE 4: Dedicated Boot Splash Animation ────────────────────
    run_boot_splash(&master_cfg.display, hw);

    // ── STAGE 5: Show Queued Alerts & Proceed ───────────────────────
    super::show_queued_alerts(hw);

    adc
}
