//! Boot Security, Passcode Verification & Safe-Mode Lockout Routines.

use crate::config::MasterConfig;
use crate::security::PasscodeState;
use crate::services::SystemStorage;
use crate::services::radio::RadioMelodyExt;
use drivers::backlight::BacklightController;
use drivers::bk4829::Bk4829;
use drivers::st7565::St7565;

#[inline(always)]
pub fn draw_locked_screen(canvas: &mut gfx::Canvas, sub_msg: &str) {
    gfx::widgets::draw_dialog(canvas, "Locked", "Too Many Attempts", sub_msg, false);
}

#[inline(never)]
pub fn handle_lockout_penalty(
    lcd: &mut St7565<bsp::LcdBus>,
    radio: &mut Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
    storage: &mut SystemStorage,
    failed: u8,
    max_tries: u8,
) {
    if failed < max_tries {
        return;
    }
    lcd.turn_on();
    let wait_secs = ((failed - max_tries + 1) as u32) * 30;
    radio.set_status_led(bsp::StatusLed::Red);
    let canvas = crate::system_canvas();
    for remaining in (1..=wait_secs).rev() {
        let mut sec_buf = [0u8; 32];
        let s = gfx::text::format_num_msg(&mut sec_buf, "Try again in ", remaining, "s");
        draw_locked_screen(canvas, s);
        lcd.blit(canvas);
        for _ in 0..100 {
            py32_hal::wdt::Watchdog::feed();
            py32_hal::delay::delay_ms(10);
        }
    }
    crate::security::passcode::reset_lockout_penalty(storage);
    radio.set_status_led(bsp::StatusLed::Off);
}

#[inline(never)]
pub fn flash_error_and_pause(radio: &mut Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>) {
    radio.play_beep(crate::services::melody::BeepType::Warning);
    for _ in 0..3 {
        radio.set_status_led(bsp::StatusLed::Red);
        py32_hal::delay::delay_ms(100);
        radio.set_status_led(bsp::StatusLed::Off);
        py32_hal::delay::delay_ms(100);
    }
    py32_hal::delay::delay_ms(600);
}

#[inline(never)]
pub fn render_verify_modal(lcd: &mut St7565<bsp::LcdBus>, pct: u8) {
    let canvas = crate::system_canvas();
    gfx::widgets::draw_dialog_progress(canvas, "Passcode", "Verifying...", pct);
    lcd.blit(canvas);
}

#[inline(always)]
pub fn handle_unlock_success(
    backlight: &mut BacklightController<bsp::BspBacklight>,
    radio: &mut Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
    master_cfg: &MasterConfig,
    stealth: bool,
) {
    if !stealth {
        radio.set_status_led(bsp::StatusLed::Green);
    }
    if stealth {
        master_cfg.display.apply_to_backlight(backlight);
        backlight.wake();
    }
    if !stealth && master_cfg.audio.key_beep != 0 {
        radio.play_dialog_sound(crate::services::melody::DialogSoundType::Unlock);
    }
    radio.set_status_led(bsp::StatusLed::Off);
}

#[inline(never)]
pub fn handle_unlock_failure(
    lcd: &mut St7565<bsp::LcdBus>,
    radio: &mut Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
    storage: &mut SystemStorage,
    text_view: &gfx::gui::modules::text_input::TextInputView,
    text_model: &gfx::gui::modules::text_input::TextInputModel,
    stealth: bool,
) {
    let pcfg = crate::security::passcode::get_passcode_config(storage);
    let canvas = crate::system_canvas();
    if pcfg.failed_attempts >= pcfg.max_tries {
        draw_locked_screen(canvas, "Restarting...");
        lcd.blit(canvas);
        radio.set_status_led(bsp::StatusLed::Red);
        radio.play_beep(crate::services::melody::BeepType::Warning);
        py32_hal::delay::delay_ms(1500);
        kernel::power::PowerControl::reboot();
    }

    let mut msg_buf = [0u8; 32];
    let msg = if stealth {
        "Wrong Passcode"
    } else {
        crate::security::passcode::format_wrong_passcode_msg(
            pcfg.failed_attempts,
            pcfg.max_tries,
            &mut msg_buf,
        )
    };
    use gfx::gui::view::View;
    text_view.render(canvas, text_model);
    gfx::widgets::draw_modal_popup(canvas, None, msg);
    lcd.blit(canvas);

    flash_error_and_pause(radio);
}

#[inline(never)]
fn process_unlock_submission(
    hw: &mut crate::hardware::SystemHardware,
    text_model: &mut gfx::gui::modules::text_input::TextInputModel,
    passcode_state: &mut PasscodeState,
    master_cfg: &MasterConfig,
    text_view: &gfx::gui::modules::text_input::TextInputView,
    stealth: bool,
) -> bool {
    fn on_verify_progress(ctx: *mut (), pct: u8) {
        let lcd = unsafe { &mut *(ctx as *mut drivers::st7565::St7565<bsp::LcdBus>) };
        py32_hal::wdt::Watchdog::feed();
        render_verify_modal(lcd, pct);
    }
    let hook =
        crate::security::ProgressHook::new(&mut hw.lcd as *mut _ as *mut (), on_verify_progress);
    let res = crate::security::passcode::verify_passcode(
        hw.filesystem,
        text_model.buffer.as_bytes(),
        passcode_state,
        Some(hook),
    );

    let mut unlocked = false;
    match res {
        Ok(true) => {
            handle_unlock_success(&mut hw.backlight, &mut hw.radio, master_cfg, stealth);
            unlocked = true;
        }
        Ok(false) => {
            handle_unlock_failure(
                &mut hw.lcd,
                &mut hw.radio,
                hw.filesystem,
                text_view,
                text_model,
                stealth,
            );
        }
        Err(crate::security::passcode::PasscodeError::LockedOut) => {
            hw.radio
                .play_beep(crate::services::melody::BeepType::Warning);
            let pcfg = crate::security::passcode::get_passcode_config(hw.filesystem);
            handle_lockout_penalty(
                &mut hw.lcd,
                &mut hw.radio,
                hw.filesystem,
                pcfg.failed_attempts,
                pcfg.max_tries,
            );
        }
        _ => {}
    }
    text_model.buffer.clear();
    text_model.is_done = false;
    unlocked
}

#[inline(never)]
pub fn run_boot_unlock_prompt(
    hw: &mut crate::hardware::SystemHardware,
    passcode_state: &mut PasscodeState,
    master_cfg: &MasterConfig,
    adc: &py32_hal::adc::Adc,
) {
    use gfx::gui::modules::text_input::TextInputView;
    use gfx::gui::view::View;

    let cfg = crate::security::passcode::get_passcode_config(hw.filesystem);
    if !cfg.is_set {
        return;
    }
    passcode_state.lock();
    hw.filesystem.lock_session();
    // Headless nodes have no operator at the panel: stay locked (secure default) and leave
    // unlocking to the authenticated wire session instead of spinning on a dead keypad.
    if !crate::system::admits_physical_input() {
        return;
    }

    let power_service = unsafe { &mut *core::ptr::addr_of_mut!(crate::POWER_SERVICE) };
    crate::services::power::PowerServiceConfigExt::load_battery_config(
        power_service,
        &master_cfg.battery,
    );
    crate::services::telemetry::sample_battery_and_system(adc, false);
    crate::hardware::apply_lcd_hardware(&master_cfg.display, &mut hw.lcd);

    let stealth = cfg.stealth;

    // Boost to 72 MHz for unlock prompt and KDF unless USB FS requires exact 48 MHz lock.
    #[cfg(feature = "usb")]
    let usb_pinned = hw.usb.is_connected();
    #[cfg(not(feature = "usb"))]
    let usb_pinned = false;
    if !usb_pinned {
        py32_hal::rcc::set_sysclk_72mhz();
    }

    // 1. Initial Lockout check if device rebooted while locked out
    handle_lockout_penalty(
        &mut hw.lcd,
        &mut hw.radio,
        hw.filesystem,
        cfg.failed_attempts,
        cfg.max_tries,
    );

    hw.lcd.turn_on();

    if stealth {
        hw.backlight
            .set_mode(drivers::backlight::BacklightMode::Off);
        hw.backlight.turn_off();
    } else {
        master_cfg.display.apply_to_backlight(&mut hw.backlight);
        hw.backlight.wake();
    }

    let input_len = if cfg.expose_len && cfg.passcode_len > 0 {
        cfg.passcode_len as usize
    } else {
        16
    };
    let modal =
        unsafe { &mut *core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL) };
    let text_model = modal.prompt_passcode(
        0,
        "Enter Passcode",
        input_len,
        cfg.digits_only,
        cfg.expose_len,
    );
    text_model.is_k1 = master_cfg.is_k1();
    let mut text_view = TextInputView::new();

    loop {
        py32_hal::wdt::Watchdog::feed();

        #[cfg(feature = "usb")]
        let _ = hw.usb.poll();

        // 1. Scan keyboard (10ms polling)
        if let Some((code, state)) = hw.key_scanner.scan() {
            let ev = kernel::event::Event::Key { code, state };
            let _ = text_view.on_event(text_model, &ev);
            if !stealth && state == kernel::event::KeyState::Pressed {
                hw.backlight.wake();
                if master_cfg.audio.key_beep != 0 {
                    hw.radio
                        .play_beep(crate::services::melody::BeepType::from_u8(
                            master_cfg.audio.key_beep,
                        ));
                }
            }
        }
        let _ = text_view.on_event(text_model, &kernel::event::Event::Tick);
        if !stealth {
            hw.backlight.tick(10);
        }

        if (text_model.ticks & 63) == 0 {
            crate::services::telemetry::sample_battery_and_system(adc, false);
        }

        // 2. Render input screen with inverted right status bar items on header
        let canvas = crate::system_canvas();
        text_view.render(canvas, text_model);
        #[cfg(feature = "usb")]
        let is_usb = hw.usb.is_connected();
        #[cfg(not(feature = "usb"))]
        let is_usb = false;
        crate::services::draw_boot_status_right(canvas, master_cfg.battery.battery_format, is_usb);
        hw.lcd.blit(canvas);

        // 3. Process submission
        if text_model.is_done && !text_model.buffer.is_empty() {
            if process_unlock_submission(
                hw,
                text_model,
                passcode_state,
                master_cfg,
                &text_view,
                stealth,
            ) {
                break;
            }
        }

        py32_hal::delay::delay_ms(10);
    }
    modal.close();
}
