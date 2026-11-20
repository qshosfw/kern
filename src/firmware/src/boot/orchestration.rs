//! Hardware Initialization, Reset Cause Introspection & Boot Orchestration.

use crate::config::MasterConfig;
use crate::hardware::SystemHardware;
use crate::services::power::PowerServiceConfigExt;
use crate::{CALIBRATION, PASSCODE_STATE, POWER_SERVICE};
use py32_hal::adc::Adc;

#[cfg(not(all(target_arch = "arm", not(test))))]
static TEST_BACKUP_TAG: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[inline(never)]
pub fn read_backup_tag() -> kernel::boot::BackupCrashTag {
    #[cfg(all(target_arch = "arm", not(test)))]
    let raw = py32_hal::rtc::RealTimeClock::read_alarm_raw();
    #[cfg(not(all(target_arch = "arm", not(test))))]
    let raw = TEST_BACKUP_TAG.load(core::sync::atomic::Ordering::Relaxed);
    kernel::boot::BackupCrashTag::decode(raw).unwrap_or(kernel::boot::BackupCrashTag::empty())
}

#[inline(never)]
pub fn write_backup_tag(tag: kernel::boot::BackupCrashTag) {
    #[cfg(all(target_arch = "arm", not(test)))]
    py32_hal::rtc::RealTimeClock::write_alarm_raw(tag.encode());
    #[cfg(not(all(target_arch = "arm", not(test))))]
    TEST_BACKUP_TAG.store(tag.encode(), core::sync::atomic::Ordering::Relaxed);
}

pub fn check_reset_cause() {
    let raw_csr = py32_hal::wdt::Watchdog::raw_reset_flags();
    let reset_cause = py32_hal::wdt::Watchdog::reset_cause();
    py32_hal::wdt::Watchdog::clear_reset_flags();

    let mut tag = read_backup_tag();
    let clean_reboot = (tag.flags & kernel::boot::BackupCrashTag::FLAG_CLEAN_REBOOT) != 0;

    if clean_reboot {
        // Intentional reboot: preserve strike counter unless cleared after >=10s healthy uptime,
        // clear CLEAN_REBOOT bit so unexpected resets next boot are caught.
        tag.flags &= !kernel::boot::BackupCrashTag::FLAG_CLEAN_REBOOT;
        write_backup_tag(tag);
        return;
    }

    // Never overwrite a valid HardFault / Appf BOOT_RECORD slot with a PC=0 placeholder.
    let existing = crate::panic::BOOT_RECORD.get().latest_valid_slot();
    let is_watchdog_reset = matches!(
        reset_cause,
        py32_hal::wdt::ResetCause::IndependentWatchdog | py32_hal::wdt::ResetCause::WindowWatchdog
    );
    let is_brownout = matches!(reset_cause, py32_hal::wdt::ResetCause::PowerOn)
        && ((raw_csr & py32_pac::rcc::CSR_PWRRSTF) != 0 || tag.reason_code != 0);

    if is_watchdog_reset && existing.is_none_or(|s| s.crash_count == 0) {
        let slot = crate::panic::BOOT_RECORD.get_mut().begin_write();
        slot.reset_reason = kernel::boot::ResetReason::Wdog as u32;
        slot.set_actor(b"WATCHDOG");
        slot.panic_file_hash = raw_csr;
        slot.crash_count = slot.crash_count.max(tag.strikes.saturating_add(1));
        tag.strikes = slot.crash_count;
        tag.reason_code = 3;
        tag.flags |= kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH;
        crate::panic::BOOT_RECORD.get_mut().commit_write();
        write_backup_tag(tag);
    } else if is_brownout && existing.is_none_or(|s| s.crash_count == 0) && tag.strikes > 0 {
        let slot = crate::panic::BOOT_RECORD.get_mut().begin_write();
        slot.reset_reason = kernel::boot::ResetReason::Brow as u32;
        slot.set_actor(b"BROWNOUT");
        slot.panic_file_hash = raw_csr;
        slot.crash_count = tag.strikes;
        crate::panic::BOOT_RECORD.get_mut().commit_write();
    } else if let Some(s) = existing {
        if s.crash_count > tag.strikes {
            tag.strikes = s.crash_count;
            tag.flags |= kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH;
            write_backup_tag(tag);
        }
    }
}

/// Mark the backup-domain reboot tag before an intentional software reset.
#[inline(never)]
pub fn prepare_clean_reboot() {
    let mut tag = read_backup_tag();
    tag.flags |= kernel::boot::BackupCrashTag::FLAG_CLEAN_REBOOT;
    write_backup_tag(tag);
    crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
}

/// Reset both `.uninit` crash-loop strike counter and backup-domain strikes after healthy uptime.
#[inline(never)]
pub fn clear_crash_strikes() {
    crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
    let mut tag = read_backup_tag();
    tag.strikes = 0;
    tag.flags &= !(kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH
        | kernel::boot::BackupCrashTag::FLAG_SAFE_MODE_REQ);
    tag.reason_code = 0;
    write_backup_tag(tag);
}

fn init_system_services(
    hw: &mut SystemHardware,
    master_cfg: &MasterConfig,
    passcode_state: &crate::security::PasscodeState,
    adc: &Adc,
) {
    super::ensure_default_files(hw.filesystem);
    #[cfg(feature = "filesystem")]
    super::flush_crash_log_if_pending(hw.filesystem);
    #[cfg(feature = "passcode")]
    let already_authenticated =
        !passcode_state.is_locked && passcode_state.master_key.as_slice().iter().any(|&b| b != 0);
    #[cfg(not(feature = "passcode"))]
    let already_authenticated = false;

    if !passcode_state.is_locked && !already_authenticated {
        let ps = unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) };
        crate::security::passcode::init_passcode_state(hw.filesystem, ps);
    }
    let mcu_uid = bsp::uid::read_mcu_uid();
    let calibration = unsafe { &mut *core::ptr::addr_of_mut!(CALIBRATION) };
    calibration.load_into(hw.filesystem, &mcu_uid);
    let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
    power_service.load_battery_config(&master_cfg.battery);
    power_service
        .battery_driver
        .set_calib_const(calibration.battery_calib_const());
    hw.radio.set_xtal_freq_trim(calibration.xtal_freq_low());

    hw.key_scanner.configure(
        master_cfg.keys.hold_delay_ms(),
        master_cfg.keys.repeat_rate_ms(),
        master_cfg.keys.fast_scroll != 0,
    );
    crate::services::telemetry::sample_battery_and_system(adc, false);
    bsp::pins::FLASHLIGHT.set_low();
    crate::apps::init_all_apps();

    #[cfg(feature = "usb")]
    crate::hardware::SystemHardware::sync_usb_telemetry(&mut hw.usb, master_cfg);
    #[cfg(feature = "filesystem")]
    crate::apps::launcher::catalog::get_catalog().scan(hw.filesystem);
}

#[inline(never)]
pub fn run_boot_splash(
    hw: &mut SystemHardware,
    master_cfg: &MasterConfig,
    passcode_state: &crate::security::PasscodeState,
    adc: &Adc,
) {
    let disp_cfg = &master_cfg.display;
    let audio_cfg = &master_cfg.audio;
    const SPLASH_DUR_MS: [u16; 5] = [0, 500, 1000, 2000, 3000];
    let target_splash_ms: u32 = SPLASH_DUR_MS
        .get(disp_cfg.boot_splash_duration as usize)
        .copied()
        .unwrap_or(1000) as u32;
    let show_splash = target_splash_ms > 0 && disp_cfg.boot_splash_mode != 0;
    let jingle_buf: &mut [crate::services::melody::ToneNote;
             crate::services::melody::MAX_MELODY_NOTES] = unsafe {
        &mut *(crate::data::arena::scratch_tail_bytes_mut()[1760..].as_mut_ptr()
            as *mut [crate::services::melody::ToneNote; crate::services::melody::MAX_MELODY_NOTES])
    };
    let jingle_len = crate::services::melody::load_jingle_notes(
        hw.filesystem,
        audio_cfg.boot_jingle,
        audio_cfg.jingle_path_str(),
        jingle_buf,
    );
    let jingle_notes = &jingle_buf[..jingle_len];
    let play_jingle = !jingle_notes.is_empty();

    init_system_services(hw, master_cfg, passcode_state, adc);

    if !show_splash && !play_jingle {
        return;
    }

    const FRAMES: u32 = 16;
    let first_frame = if disp_cfg.boot_splash_anim != 0 {
        1
    } else {
        FRAMES
    };

    #[inline(never)]
    fn render_splash_frame(
        canvas: &mut gfx::Canvas,
        lcd: &mut drivers::st7565::St7565<bsp::LcdBus>,
        base_vram: &[u8; 1024],
        anim: u8,
        frame: u8,
    ) {
        super::splash::apply_splash_anim(canvas, base_vram, anim, frame);
        canvas.mark_all_dirty();
        lcd.blit(canvas);
    }

    let canvas = crate::system_canvas();
    let base_vram: &[u8; 1024] = unsafe {
        let slice = &mut crate::runtime::transient_arena_bytes_mut()[..1024];
        if show_splash {
            super::splash::render_base_logo(canvas, hw.filesystem, disp_cfg);
            slice.copy_from_slice(canvas.buffer_flat());
            let arr = &*(slice.as_ptr() as *const [u8; 1024]);
            render_splash_frame(
                canvas,
                &mut hw.lcd,
                arr,
                disp_cfg.boot_splash_anim,
                first_frame as u8,
            );
        }
        &*(slice.as_ptr() as *const [u8; 1024])
    };

    // Immediately illuminate display and wake backlight when splash is visible — zero black-screen lag!
    if show_splash {
        hw.lcd.turn_on();
        hw.backlight.wake();
    }

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
                render_splash_frame(
                    canvas,
                    &mut hw.lcd,
                    base_vram,
                    disp_cfg.boot_splash_anim,
                    cur_frame as u8,
                );
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

#[cfg(all(feature = "filesystem", feature = "krx_native"))]
#[inline(never)]
fn launch_crash_module(hw: &mut SystemHardware, mode: usize) -> bool {
    hw.lcd.turn_on();
    hw.backlight.wake();
    bsp::set_status_led(bsp::StatusLed::Orange);
    let launched =
        crate::runtime::try_launch_boot_module(&mut hw.lcd, hw.filesystem, "/apps/panic.mod", mode)
            .is_ok();
    bsp::set_status_led(bsp::StatusLed::Off);
    launched
}

#[inline(never)]
fn boot_stage_security_check(
    hw: &mut SystemHardware,
    master_cfg: &mut MasterConfig,
    passcode_state: &mut crate::security::PasscodeState,
    adc: &Adc,
) {
    let _ = (&hw, &master_cfg, &passcode_state, adc);
    #[cfg(feature = "filesystem")]
    super::flush_crash_log_if_pending(hw.filesystem);

    let backup_strikes = read_backup_tag().strikes;
    let crash_count = crate::panic::BOOT_RECORD
        .get()
        .latest_valid_slot()
        .map_or(0, |s| s.crash_count)
        .max(backup_strikes);

    if crash_count >= 2 {
        #[cfg(all(feature = "filesystem", feature = "krx_native"))]
        {
            hw.lcd.set_contrast(50);
            if launch_crash_module(hw, 2) {
                master_cfg.load_into(hw.filesystem);
            }
        }
        super::clear_crash_strikes();
        return;
    }

    #[cfg(feature = "passcode")]
    if crate::security::passcode::is_passcode_set(hw.filesystem) {
        passcode_state.lock();
        hw.filesystem.lock_session();
        let pcfg = crate::security::passcode::get_passcode_config(hw.filesystem);
        let power_service = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
        power_service.load_battery_config(&master_cfg.battery);
        crate::services::telemetry::sample_battery_and_system(adc, false);

        // Configure LCD parameters (contrast, inversion, shading) without waking the backlight
        crate::hardware::apply_lcd_hardware(&master_cfg.display, &mut hw.lcd);

        if pcfg.stealth {
            // Stealth / Dimmed mode: ensure backlight remains strictly OFF during boot and passcode entry
            hw.backlight
                .set_mode(drivers::backlight::BacklightMode::Off);
            hw.backlight.turn_off();
        } else {
            master_cfg.display.apply_to_backlight(&mut hw.backlight);
            hw.backlight.wake();
        }

        super::security::run_boot_unlock_prompt(hw, passcode_state, master_cfg, adc);
        let _ = crate::security::sync_filesystem_security(hw.filesystem, passcode_state);
    }

    #[cfg(not(feature = "passcode"))]
    {
        let status = crate::security::sync_filesystem_security(hw.filesystem, passcode_state);
        let msg = unsafe {
            let ptr = crate::data::arena::scratch_tail_bytes_mut().as_mut_ptr()
                as *mut heapless::String<168>;
            ptr.write(heapless::String::new());
            &mut *ptr
        };
        if let Some(title) = crate::security::format_storage_alert_message(status, msg) {
            crate::orchestrator::compositor::queue_system_alert(title, msg.as_str());
        }
    }
}

#[inline(never)]
pub fn boot_orchestration(hw: &mut SystemHardware) -> Adc {
    let master_cfg = crate::master_config();
    let passcode_state = unsafe { &mut *core::ptr::addr_of_mut!(PASSCODE_STATE) };
    *passcode_state = crate::security::PasscodeState::new();

    let adc = py32_hal::adc::Adc::init();

    master_cfg.load_into(hw.filesystem);

    // ── STAGE 1: Clock Stabilization & USB Bring-up (<2ms) ──────────
    // Clock is stabilized at 48 MHz for clean USB bring-up and PLL lock.
    py32_hal::rcc::set_sysclk_48mhz();
    #[cfg(feature = "usb")]
    {
        hw.usb.init();
    }

    // ── STAGE 2: Instant Security & Recovery Introspection (<30ms) ──
    boot_stage_security_check(hw, master_cfg, passcode_state, &adc);

    let _ = hw.radio.init();
    hw.audio.init();
    crate::hardware::apply_audio_hardware(&master_cfg.audio, &mut hw.audio, &mut hw.radio);

    if !passcode_state.is_locked {
        crate::hardware::apply_display_hardware(
            &master_cfg.display,
            &mut hw.lcd,
            &mut hw.backlight,
        );
    }

    // ── STAGE 3: Instant Boot Splash & Concurrent System Bring-up ───
    run_boot_splash(hw, master_cfg, passcode_state, &adc);

    // ── STAGE 4: Post-Boot Recovery Prompt (Offloaded to /apps/panic.mod) ───
    #[cfg(all(feature = "filesystem", feature = "krx_native"))]
    {
        let crash_slot = crate::panic::BOOT_RECORD.get().latest_valid_slot();
        if crash_slot.is_some_and(|s| s.crash_count > 0 && s.is_crash()) {
            use crate::services::radio::RadioMelodyExt;
            hw.radio
                .play_beep(crate::services::melody::BeepType::Warning);
            hw.key_scanner.clear();
            let _ = launch_crash_module(hw, 1);
            hw.key_scanner.clear();
        }
    }

    #[cfg(feature = "usb")]
    if hw.usb.is_connected() || hw.usb.consecutive_sof() > 0 {
        py32_hal::rcc::set_sysclk_48mhz();
    } else {
        py32_hal::rcc::set_sysclk_index(master_cfg.system.cpu_clock);
    }
    #[cfg(not(feature = "usb"))]
    py32_hal::rcc::set_sysclk_index(master_cfg.system.cpu_clock);

    adc
}
