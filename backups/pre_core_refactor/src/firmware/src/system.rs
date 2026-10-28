//! System Event Loop & Main Execution Engine.

use crate::hardware::SystemHardware;
use crate::services::RadioMelodyExt;
use crate::services::power::PowerService;
use crate::{EVENT_QUEUE, POWER_SERVICE};
use kernel::event::{Event, KeyCode, KeyState};
use kernel::service::SerialPort;
use py32_hal::adc::Adc;

pub fn enqueue_event(event: Event) {
    cortex_m::interrupt::free(|cs| {
        let _ = EVENT_QUEUE.borrow(cs).borrow_mut().push_back(event);
    });
}

pub fn dequeue_event() -> Option<Event> {
    cortex_m::interrupt::free(|cs| EVENT_QUEUE.borrow(cs).borrow_mut().pop_front())
}

#[inline(always)]
fn wake_and_notify_activity(hw: &mut SystemHardware) {
    hw.backlight.wake();
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
    }
    hw.orchestrator.notify_activity();
}

#[inline(never)]
fn handle_critical_battery_cutoff(hw: &mut SystemHardware, adc: &Adc) {
    hw.backlight.turn_off();
    hw.lcd.turn_off();
    hw.audio.disable();
    hw.radio.set_status_led(bsp::StatusLed::Red);

    let mut check_ms = py32_hal::delay::millis();
    while unsafe {
        (*core::ptr::addr_of!(POWER_SERVICE)).is_critical()
            && !(*core::ptr::addr_of!(POWER_SERVICE)).is_charging()
    } {
        py32_hal::wdt::Watchdog::feed();
        let cur = py32_hal::delay::millis();

        let cycle = (cur >> 8) & 0x07;
        if cycle == 0 {
            hw.radio.set_status_led(bsp::StatusLed::Red);
        } else {
            hw.radio.set_status_led(bsp::StatusLed::Off);
        }

        if cur.wrapping_sub(check_ms) >= 500 {
            check_ms = cur;
            crate::services::telemetry::sample_battery_and_system(adc, false);
        }

        cortex_m::asm::wfi();
    }

    hw.radio.set_status_led(bsp::StatusLed::Off);
    hw.lcd.init();
    hw.backlight.wake();
}

#[cfg(any(feature = "uart", feature = "usb_cdc"))]
#[inline(never)]
fn poll_rigwire(hw: &mut SystemHardware, now: u32) {
    if let Some((header, _raw_len)) = hw.wire.poll(now) {
        let handled = crate::services::wire::dispatch_wire_record(&header, hw, now);

        if !handled {
            wake_and_notify_activity(hw);
            enqueue_event(Event::Sys(kernel::ipc::SysEvent {
                target: header.service,
                command: header.opcode,
                arg: header.sequence as u16,
            }));
        }
    }
}

#[inline(never)]
fn handle_one_second_tick(hw: &mut SystemHardware, low_batt_warning_timer: &mut u8) {
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).tick_second();
    }

    let pwr = unsafe { &*core::ptr::addr_of!(POWER_SERVICE) };
    if pwr.is_low() && !pwr.is_charging() && pwr.uptime_secs() > 5 {
        *low_batt_warning_timer = low_batt_warning_timer.saturating_add(1);
        if *low_batt_warning_timer >= 30 {
            *low_batt_warning_timer = 0;
            let beeps_on = crate::master_config_ref().audio.key_beep != 0;
            if beeps_on {
                hw.radio
                    .play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
            }
        }
    } else {
        *low_batt_warning_timer = 0;
    }

    #[cfg(feature = "usb")]
    {
        let cfg = crate::master_config_ref();
        crate::hardware::SystemHardware::sync_usb_telemetry(&mut hw.usb, cfg);
    }
}

pub fn run_system_loop(hw: &mut SystemHardware, adc: Adc, _boot_start_ms: u32) -> ! {
    let mut last_tick = py32_hal::delay::millis();
    let mut last_battery_tick = py32_hal::delay::millis();
    let mut last_key_tick = py32_hal::delay::millis();
    let mut last_ui_tick = py32_hal::delay::millis();
    let mut last_subframe_tick = py32_hal::delay::millis();
    let mut dirty_render = true;
    #[cfg(feature = "flight_recorder")]
    let mut flight_recorder_cleared = false;
    let mut low_batt_warning_timer = 0u8;

    loop {
        // Feed Independent Watchdog
        py32_hal::wdt::Watchdog::feed();

        // 0. Poll hardware USB on EVERY iteration for instantaneous host responsiveness
        #[cfg(feature = "usb")]
        {
            if let Some(usb_ev) = hw.usb.poll() {
                dirty_render = true;
                if matches!(
                    usb_ev,
                    kernel::event::UsbEvent::Connected | kernel::event::UsbEvent::Resumed
                ) {
                    wake_and_notify_activity(hw);
                    #[cfg(feature = "usb_cdc")]
                    hw.wire.port.write(
                        b"\r\n--- KERN-OS RigWire ---\r\nReady. Type 'help' or send COBS frames.\r\n",
                    );
                }
                enqueue_event(Event::Usb(usb_ev));
            }
        }

        let now = py32_hal::delay::millis();

        // 0b. Poll RigWire serial protocol (UART or USB CDC)
        #[cfg(any(feature = "uart", feature = "usb_cdc"))]
        poll_rigwire(hw, now);

        // Clear BOOT record crash loop counter after 5 seconds of stable main loop execution (§13.4)
        #[cfg(feature = "flight_recorder")]
        if !flight_recorder_cleared && now.wrapping_sub(_boot_start_ms) >= 5000 {
            crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
            flight_recorder_cleared = true;
        }

        // 1. Poll hardware keyboard matrix every 5ms (DeltaFW / fagci-k1-fw standard)
        if now.wrapping_sub(last_key_tick) >= 5 {
            let dt = now.wrapping_sub(last_key_tick).min(50) as u16;
            last_key_tick = now;
            if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut_by_app(0xF3) {
                pcb.ledger.record_activation();
            }
            hw.key_scanner.scan_with_dt(dt);
            while let Some((code, state)) = hw.key_scanner.pop_event() {
                let is_locked = hw.orchestrator.is_keypad_locked();
                let should_wake =
                    !is_locked || state == KeyState::Pressed || matches!(code, KeyCode::Ptt);

                if should_wake {
                    hw.backlight.wake();
                    unsafe {
                        (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
                    }
                } else if hw.backlight.is_active() {
                    unsafe {
                        (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
                    }
                }

                enqueue_event(Event::Key { code, state });
            }
        }

        let is_display_awake = hw.backlight.is_active();
        let is_anim = hw.orchestrator.is_animating() && is_display_awake;

        // 2. Fully Adaptive Refresh Rate & Physical LCD Timing:
        // - Active UI animations: tick interval derived from active display refresh_rate:
        //   0 (20 Hz): 50ms tick (Battery saving)
        //   1 (60 Hz): 16ms tick (Std, matches f4hwn flashmap 15-16ms LC response time)
        //   2 (75 Hz): 13ms tick (Turbo, matches ST7565 internal panel scan ~76 Hz)
        //   3 (90 Hz): 11ms tick (Ultra)
        // - Idle: 50ms tick (20 Hz) to maximize Cortex-M0+ sleep and preserve battery life.
        #[cfg(feature = "high_refresh")]
        let ui_tick_interval = if is_anim {
            match crate::master_config_ref().display.refresh_rate {
                0 => 50u32,
                1 => 16u32,
                2 => 13u32,
                3 => 11u32,
                _ => 16u32,
            }
        } else {
            50u32
        };
        #[cfg(not(feature = "high_refresh"))]
        let ui_tick_interval = 50u32;

        if now.wrapping_sub(last_ui_tick) >= ui_tick_interval {
            let delta = now.wrapping_sub(last_ui_tick);
            last_ui_tick = now;
            if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut_by_app(0xF0) {
                pcb.ledger.record_activation();
            }
            hw.backlight.tick(delta as u16);
            hw.audio.tick(delta as u16);
            enqueue_event(Event::Tick);
        }

        // High-speed temporal FRC subframe refresh:
        // When in Grayscale mode (shading_mode == 0), display is awake, not actively animating,
        // and no canvas re-render is pending:
        // Pushes temporal subframes at rock-solid 2ms intervals (500 Hz subframe / 166.7 Hz optical cycle).
        #[cfg(all(feature = "grayscale", feature = "high_refresh"))]
        if is_display_awake
            && !is_anim
            && !dirty_render
            && crate::master_config_ref().display.shading_mode == 0
            && now.wrapping_sub(last_subframe_tick) >= 2
        {
            last_subframe_tick = now;
            hw.refresh_subframe();
        }

        // 3. Periodic ADC battery/temp sampling every 200ms (fast charging detection & screen wake)
        if now.wrapping_sub(last_battery_tick) >= 200 {
            last_battery_tick = now;
            if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut_by_app(0xF2) {
                pcb.ledger.record_activation();
            }
            crate::services::telemetry::sample_battery_and_system(&adc, hw.audio.is_active());

            // Check for battery pack charging transition
            let charging_trans =
                unsafe { (*core::ptr::addr_of_mut!(POWER_SERVICE)).poll_charging_transition() };
            if let Some(is_charging) = charging_trans {
                if is_charging {
                    wake_and_notify_activity(hw);
                }
                dirty_render = true;
                enqueue_event(Event::Charging(is_charging));
            }

            // Kernel Safeguard: Critical Battery Cutoff & Low Power Sleep Mode
            let is_crit = unsafe { (*core::ptr::addr_of!(POWER_SERVICE)).is_critical() };
            let is_chrg = unsafe { (*core::ptr::addr_of!(POWER_SERVICE)).is_charging() };
            if is_crit && !is_chrg {
                handle_critical_battery_cutoff(hw, &adc);
                dirty_render = true;
            }
        }

        // 4. 1-second system tick for uptime & telemetry update
        if now.wrapping_sub(last_tick) >= 1000 {
            last_tick = now;
            handle_one_second_tick(hw, &mut low_batt_warning_timer);
            dirty_render = true;
        }

        while let Some(ev) = dequeue_event() {
            hw.dispatch_event_and_actions(ev);
            match ev {
                Event::Tick => {
                    if is_anim {
                        dirty_render = true;
                    }
                }
                _ => {
                    dirty_render = true;
                }
            }
        }

        // Render loop
        if dirty_render {
            hw.render_frame();
            dirty_render = false;
            last_subframe_tick = py32_hal::delay::millis();
        }

        // Sleep until next event/interrupt, unless USB is actively connected, high-speed animation is active,
        // or Grayscale FRC is actively cycling subframes while the display is awake
        #[cfg(feature = "usb")]
        if hw.usb.is_connected() || hw.usb.has_pending_transfer() {
            continue;
        }

        #[cfg(feature = "high_refresh")]
        if is_anim {
            continue;
        }

        #[cfg(all(feature = "grayscale", feature = "high_refresh"))]
        if is_display_awake && crate::master_config_ref().display.shading_mode == 0 {
            continue;
        }

        // Sleep until next event/interrupt
        cortex_m::asm::wfi();
    }
}
