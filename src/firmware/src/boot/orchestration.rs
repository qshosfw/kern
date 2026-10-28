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
    pub filesystem: &'a mut SystemStorage<'b>,
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

#[inline(always)]
fn execute_bg_step(
    step: &mut u8,
    hw: &mut BootHardware,
    master_cfg: &MasterConfig,
    passcode_state: &crate::security::PasscodeState,
    adc: &Adc,
) {
    match *step {
        0 => {
            super::ensure_default_files(hw.filesystem, None);
            *step = 1;
        }
        1 => {
            if !passcode_state.is_locked {
                let mut ps = unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) };
                crate::security::passcode::init_passcode_state(hw.filesystem, &mut ps);
            }
            *step = 2;
        }
        2 => {
            let mcu_uid = bsp::uid::read_mcu_uid();
            let calibration = unsafe { &mut *core::ptr::addr_of_mut!(CALIBRATION) };
            calibration.load_into(hw.filesystem, &mcu_uid);
            let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
            power_service.load_battery_config(&master_cfg.battery);
            power_service.battery_driver.calib_const = calibration.battery_calib_const();
            hw.radio.set_xtal_freq_trim(calibration.xtal_freq_low());
            *step = 3;
        }
        3 => {
            hw.key_scanner.configure(
                master_cfg.keys.hold_delay_ms(),
                master_cfg.keys.repeat_rate_ms(),
                master_cfg.keys.fast_scroll != 0,
            );
            crate::services::telemetry::sample_battery_and_system(adc, false);
            bsp::pins::FLASHLIGHT.set_low();
            crate::apps::init_all_apps();
            *step = 4;
        }
        4 => {
            #[cfg(feature = "usb")]
            crate::hardware::SystemHardware::sync_usb_telemetry(hw.usb, master_cfg);
            #[cfg(feature = "filesystem")]
            crate::apps::launcher::catalog::get_catalog().scan(hw.filesystem);
            *step = 5;
        }
        _ => {}
    }
}

#[inline(never)]
pub fn run_boot_splash(
    hw: &mut BootHardware,
    master_cfg: &MasterConfig,
    passcode_state: &crate::security::PasscodeState,
    adc: &Adc,
) {
    let disp_cfg = &master_cfg.display;
    let audio_cfg = &master_cfg.audio;
    let target_splash_ms: u32 = match disp_cfg.boot_splash_duration {
        0 => 0,
        1 => 500,
        2 => 1000,
        3 => 2000,
        4 => 3000,
        _ => 1000,
    };
    let show_splash = target_splash_ms > 0 && disp_cfg.boot_splash_mode != 0;
    let jingle = crate::services::melody::load_jingle_melody(
        hw.filesystem,
        audio_cfg.boot_jingle,
        audio_cfg.jingle_path_str(),
    );
    let jingle_notes = jingle.as_slice();
    let play_jingle = !jingle_notes.is_empty();

    let mut bg_step = 0u8;
    while bg_step < 5 {
        execute_bg_step(&mut bg_step, hw, master_cfg, passcode_state, adc);
    }

    if !show_splash && !play_jingle {
        hw.lcd.turn_on();
        hw.backlight.wake();
        return;
    }

    const FRAMES: u32 = 16;
    let first_frame = if disp_cfg.boot_splash_anim != 0 {
        1
    } else {
        FRAMES
    };

    let canvas = crate::system_canvas();
    let base_vram: &[u8; 1024] = unsafe {
        let slice = &mut crate::runtime::transient_arena_bytes_mut()[..1024];
        if show_splash {
            super::splash::render_base_logo(canvas, hw.filesystem, disp_cfg);
            slice.copy_from_slice(canvas.buffer_flat());
            let arr = &*(slice.as_ptr() as *const [u8; 1024]);
            super::splash::apply_splash_anim(
                canvas,
                arr,
                disp_cfg.boot_splash_anim,
                first_frame as u8,
            );
            canvas.mark_all_dirty();
            hw.lcd.blit(canvas);
        }
        &*(slice.as_ptr() as *const [u8; 1024])
    };

    // Immediately illuminate display and wake backlight — zero black-screen lag!
    hw.lcd.turn_on();
    hw.backlight.wake();

    let mut note_idx = 0usize;
    let mut note_end_ms = 0u32;
    let mut was_amp_active = false;
    if play_jingle {
        was_amp_active = hw.radio.begin_tone_sequence();
        if !was_amp_active {
            py32_hal::delay::delay_ms(35);
        }
        let n = jingle_notes[0];
        hw.radio.step_tone_note(n.freq_hz);
        note_end_ms = n.duration_ms as u32;
    }

    let start_ms = py32_hal::delay::millis();
    let mut cur_frame = first_frame;

    loop {
        bsp::Board::wdt_feed();
        let elapsed = py32_hal::delay::millis().wrapping_sub(start_ms);

        if play_jingle && note_idx < jingle_notes.len() && elapsed >= note_end_ms {
            hw.radio.mute_tone_note();
            note_idx += 1;
            if note_idx < jingle_notes.len() {
                let n = jingle_notes[note_idx];
                hw.radio.step_tone_note(n.freq_hz);
                note_end_ms = note_end_ms.saturating_add(n.duration_ms as u32);
            } else {
                hw.radio.end_tone_sequence(was_amp_active);
            }
        }

        if show_splash && cur_frame < FRAMES {
            let next_frame_ms = (target_splash_ms * cur_frame) / FRAMES;
            if elapsed >= next_frame_ms {
                cur_frame += 1;
                super::splash::apply_splash_anim(
                    canvas,
                    base_vram,
                    disp_cfg.boot_splash_anim,
                    cur_frame as u8,
                );
                canvas.mark_all_dirty();
                hw.lcd.blit(canvas);
            }
        }

        let splash_done = !show_splash || elapsed >= target_splash_ms;
        let jingle_done = !play_jingle || note_idx >= jingle_notes.len();
        if splash_done && jingle_done {
            break;
        }

        #[cfg(feature = "usb")]
        let _ = hw.usb.poll();
        if let Some((_, kernel::event::KeyState::Pressed)) = hw.key_scanner.scan() {
            break;
        }
        py32_hal::delay::delay_ms(1);
    }

    if play_jingle && note_idx < jingle_notes.len() {
        hw.radio.end_tone_sequence(was_amp_active);
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
    calibration.load_into(hw.filesystem, &mcu_uid);
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
    let _ = (&hw, &master_cfg, &passcode_state, adc);
    let crash_slot = crate::panic::BOOT_RECORD.get().latest_valid_slot().copied();
    let crash_count = crash_slot.as_ref().map_or(0, |s| s.crash_count);

    if crash_count >= 2 {
        #[cfg(feature = "panic_gui")]
        if let Some(ref slot) = crash_slot {
            super::recovery::run_safe_mode_recovery(hw, slot, master_cfg);
        }
        #[cfg(not(feature = "panic_gui"))]
        crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
        return;
    }

    #[cfg(feature = "passcode")]
    if crate::security::passcode::is_passcode_set(hw.filesystem) {
        passcode_state.lock();
        let pcfg = crate::security::passcode::get_passcode_config(hw.filesystem);
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
    }

    #[cfg(not(feature = "passcode"))]
    if crate::security::passcode::is_passcode_set(hw.filesystem) {
        super::queue_alert("Storage Locked", "Passcode enabled in storage");
    }
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

    master_cfg.load_into(hw.filesystem);

    // ── STAGE 1: Instant Security & Recovery Introspection (<30ms) ──
    boot_stage_security_check(hw, master_cfg, passcode_state, &adc);

    // ── STAGE 2: Clocks, Radio & Display Core Setup (<5ms) ──────────
    if master_cfg.battery.cpu_clock == 1 {
        py32_hal::rcc::set_sysclk_72mhz();
    } else {
        py32_hal::rcc::set_sysclk_48mhz();
    }

    let _ = hw.radio.init();
    hw.audio.init();
    crate::hardware::apply_audio_hardware(&master_cfg.audio, hw.audio, hw.radio);

    if !passcode_state.is_locked {
        crate::hardware::apply_display_hardware(&master_cfg.display, hw.lcd, hw.backlight);
    }

    // ── STAGE 3: Instant Boot Splash & Concurrent System Bring-up ───
    run_boot_splash(hw, master_cfg, passcode_state, &adc);

    // ── STAGE 4: Show Queued Alerts & Proceed ───────────────────────
    super::show_queued_alerts(hw);

    adc
}
