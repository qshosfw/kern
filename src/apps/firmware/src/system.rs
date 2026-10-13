//! System Event Loop & Main Execution Engine.

use crate::MASTER_CONFIG;
use crate::hardware::SystemHardware;
use crate::services::RadioMelodyExt;
use crate::services::power::PowerService;
use crate::{EVENT_QUEUE, POWER_SERVICE};
use kernel::event::{Event, KeyCode};
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

pub fn run_system_loop(hw: &mut SystemHardware, adc: Adc, boot_start_ms: u32) -> ! {
    let mut last_tick = py32_hal::delay::millis();
    let mut last_battery_tick = py32_hal::delay::millis();
    let mut last_key_tick = py32_hal::delay::millis();
    let mut last_ui_tick = py32_hal::delay::millis();
    let mut dirty_render = true;
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
                }
                enqueue_event(Event::Usb(usb_ev));
            }
        }

        let now = py32_hal::delay::millis();

        // Clear flight recorder crash loop counter after 5 seconds of stable main loop execution
        if !flight_recorder_cleared && now.wrapping_sub(boot_start_ms) >= 5000 {
            crate::panic::FLIGHT_RECORDER.get_mut().clear();
            flight_recorder_cleared = true;
        }

        // 1. Poll hardware keyboard matrix every 5ms (DeltaFW / fagci-k1-fw standard)
        if now.wrapping_sub(last_key_tick) >= 5 {
            let dt = now.wrapping_sub(last_key_tick).min(50) as u16;
            last_key_tick = now;
            hw.key_scanner.scan_with_dt(dt);
            while let Some((code, state)) = hw.key_scanner.pop_event() {
                let is_locked = hw.orchestrator.is_keypad_locked();
                let should_wake = !is_locked || matches!(code, KeyCode::Ptt);

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

        // 2. Send UI Tick every 50ms (20 FPS) for animations and cursor blinking
        if now.wrapping_sub(last_ui_tick) >= 50 {
            let delta = now.wrapping_sub(last_ui_tick);
            last_ui_tick = now;
            hw.backlight.tick(delta as u16);
            enqueue_event(Event::Tick);
        }

        // 3. Periodic ADC battery/temp sampling every 200ms (fast charging detection & screen wake)
        if now.wrapping_sub(last_battery_tick) >= 200 {
            last_battery_tick = now;
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
            unsafe {
                (*core::ptr::addr_of_mut!(POWER_SERVICE)).tick_second();
            }

            let pwr = unsafe { &*core::ptr::addr_of!(POWER_SERVICE) };
            if pwr.is_low() && !pwr.is_charging() && pwr.uptime_secs() > 5 {
                low_batt_warning_timer = low_batt_warning_timer.saturating_add(1);
                if low_batt_warning_timer >= 30 {
                    low_batt_warning_timer = 0;
                    let beeps_on =
                        unsafe { (*core::ptr::addr_of!(MASTER_CONFIG)).audio.key_beep != 0 };
                    if beeps_on {
                        hw.radio
                            .play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
                    }
                }
            } else {
                low_batt_warning_timer = 0;
            }

            #[cfg(feature = "usb")]
            {
                let cfg = unsafe { &*core::ptr::addr_of!(MASTER_CONFIG) };
                hw.usb
                    .update_device_identity(cfg.system.model_str(), cfg.system.tag_str());
                hw.usb.update_power_telemetry(
                    pwr.battery_voltage_mv(),
                    pwr.battery_percentage(),
                    pwr.is_charging(),
                );
            }

            dirty_render = true;
        }

        while let Some(ev) = dequeue_event() {
            hw.dispatch_event_and_actions(ev);
            dirty_render = true;
        }

        // Render loop
        if dirty_render {
            hw.render_frame();
            dirty_render = false;
        }

        // Sleep until next event/interrupt, unless USB is actively connected or transferring
        #[cfg(feature = "usb")]
        if hw.usb.is_connected() || hw.usb.has_pending_transfer() {
            continue;
        }

        // Sleep until next event/interrupt
        cortex_m::asm::wfi();
    }
}
