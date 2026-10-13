//! Hardware Initialization, Reset Cause Introspection & Boot Orchestration.

use crate::config::{ConfigBlock, MasterConfig};
use crate::services::SystemStorage;
use crate::services::power::{PowerService, PowerServiceConfigExt};
use crate::{CALIBRATION, MASTER_CONFIG, PASSCODE_STATE, POWER_SERVICE};
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
        let rec = crate::panic::FLIGHT_RECORDER.get_mut();
        if !rec.is_valid() {
            rec.magic = kernel::flight::FLIGHT_MAGIC;
            rec.crash_counter = 1;
        } else {
            rec.crash_counter = rec.crash_counter.saturating_add(1);
        }
        rec.reset_cause = 1 << 29;
    }
}

#[inline(never)]
pub fn wait_boot_splash(
    boot_start_ms: u32,
    disp_cfg: &crate::config::DisplayConfig,
    hw: &mut BootHardware,
) {
    let target_splash_ms: u32 = match disp_cfg.boot_splash_duration {
        0 => 0,
        1 => 500,
        2 => 1000,
        3 => 2000,
        4 => 3000,
        _ => 1000,
    };
    if target_splash_ms == 0 {
        return;
    }
    let elapsed = py32_hal::delay::millis().wrapping_sub(boot_start_ms);
    if elapsed >= target_splash_ms {
        return;
    }
    let remaining = target_splash_ms - elapsed;
    if disp_cfg.boot_splash_anim != 0 {
        let frame_delay = (remaining / 16).max(10);
        let canvas = crate::system_canvas();
        for step in 1..=16 {
            super::splash::render_splash_frame(canvas, hw.vfs, disp_cfg, step);
            hw.lcd.blit(canvas);
            #[cfg(feature = "usb")]
            let key = super::boot_wait_key_with_usb(hw.key_scanner, hw.usb, frame_delay);
            #[cfg(not(feature = "usb"))]
            let key = super::boot_wait_key(hw.key_scanner, frame_delay);
            if key.is_some() {
                break;
            }
        }
    } else {
        #[cfg(feature = "usb")]
        let _ = super::boot_wait_key_with_usb(hw.key_scanner, hw.usb, remaining);
        #[cfg(not(feature = "usb"))]
        let _ = super::boot_wait_key(hw.key_scanner, remaining);
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
    let crash_count = if crate::panic::FLIGHT_RECORDER.get().is_valid() {
        crate::panic::FLIGHT_RECORDER.get().crash_counter
    } else {
        0
    };

    if crash_count >= 2 {
        let pc = crate::panic::FLIGHT_RECORDER.get().fault_pc;
        let lr = crate::panic::FLIGHT_RECORDER.get().fault_lr;
        super::recovery::run_safe_mode_recovery(hw, crash_count, pc, lr, master_cfg);
    } else if cfg!(feature = "passcode") && crate::security::passcode::is_passcode_set(hw.vfs) {
        passcode_state.lock();
        crate::config::load_or_init_raw(
            master_cfg.display.as_bytes_mut(),
            crate::config::DisplayConfig::VERSION,
            crate::config::DisplayConfig::PATH,
            hw.vfs,
        );
        crate::config::load_or_init_raw(
            master_cfg.battery.as_bytes_mut(),
            crate::config::BatteryConfig::VERSION,
            crate::config::BatteryConfig::PATH,
            hw.vfs,
        );
        let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
        power_service.load_battery_config(&master_cfg.battery);
        crate::services::telemetry::sample_battery_and_system(adc, false);
        crate::hardware::apply_display_hardware(&master_cfg.display, hw.lcd, hw.backlight);

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

    super::ensure_default_files(hw.vfs);

    if !passcode_state.is_locked {
        let mut ps = unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) };
        crate::security::passcode::init_passcode_state(hw.vfs, &mut ps);
    }

    bsp::pins::FLASHLIGHT.set_low();
    init_system_services(hw, master_cfg, adc);

    #[cfg(feature = "usb")]
    {
        hw.usb
            .update_device_identity(master_cfg.system.model_str(), master_cfg.system.tag_str());
        let pwr = unsafe { &*core::ptr::addr_of!(POWER_SERVICE) };
        hw.usb.update_power_telemetry(
            pwr.battery_voltage_mv(),
            pwr.battery_percentage(),
            pwr.is_charging(),
        );
    }
}

pub fn boot_orchestration(hw: &mut BootHardware) -> Adc {
    let master_cfg = unsafe { &mut *core::ptr::addr_of_mut!(MASTER_CONFIG) };
    let passcode_state = unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) };
    *passcode_state = crate::security::PasscodeState::new();

    let adc = py32_hal::adc::Adc::init();

    #[cfg(feature = "usb")]
    {
        hw.usb.init();
    }

    // ── STAGE 1: Instant Security & Recovery Introspection (<30ms) ──
    boot_stage_security_check(hw, master_cfg, passcode_state, &adc);

    // ── STAGE 2: Immediate Boot Splash Display ──────────────────────
    if !passcode_state.is_locked {
        master_cfg.load_into(hw.vfs);
        crate::hardware::apply_display_hardware(&master_cfg.display, hw.lcd, hw.backlight);
    }

    let show_splash =
        master_cfg.display.boot_splash_mode != 0 && master_cfg.display.boot_splash_duration != 0;
    if show_splash {
        let canvas = crate::system_canvas();
        let initial_step = if master_cfg.display.boot_splash_anim != 0 {
            0
        } else {
            16
        };
        super::splash::render_splash_frame(canvas, hw.vfs, &master_cfg.display, initial_step);
        hw.lcd.blit(canvas);
        hw.lcd.turn_on();
        hw.backlight.wake();
    }

    let splash_start_ms = py32_hal::delay::millis();

    // ── STAGE 3: Concurrent Background Initialization (DURING SPLASH) ─
    boot_stage_hw_init(hw, master_cfg, passcode_state, &adc);

    // ── STAGE 4: Pad Remainder of Splash Duration ───────────────────
    if show_splash {
        wait_boot_splash(splash_start_ms, &master_cfg.display, hw);
    } else {
        hw.lcd.turn_on();
        hw.backlight.wake();
    }

    // ── STAGE 5: Show Queued Alerts & Proceed ───────────────────────
    super::show_queued_alerts(hw);

    adc
}
