//! System Event Loop & Main Execution Engine.

use crate::hardware::SystemHardware;
use crate::services::RadioMelodyExt;
use crate::services::power::PowerService;
use crate::{EVENT_QUEUE, POWER_SERVICE};
use kernel::event::{Event, KeyCode};
use py32_hal::adc::Adc;

pub fn enqueue_event(event: Event) {
    #[cfg(target_arch = "arm")]
    cortex_m::interrupt::free(|cs| {
        let _ = EVENT_QUEUE.borrow(cs).borrow_mut().push_back(event);
    });
    #[cfg(not(target_arch = "arm"))]
    {
        let cs = unsafe { cortex_m::interrupt::CriticalSection::new() };
        let _ = EVENT_QUEUE.borrow(&cs).borrow_mut().push_back(event);
    }
}

pub fn dequeue_event() -> Option<Event> {
    #[cfg(target_arch = "arm")]
    {
        cortex_m::interrupt::free(|cs| EVENT_QUEUE.borrow(cs).borrow_mut().pop_front())
    }
    #[cfg(not(target_arch = "arm"))]
    {
        let cs = unsafe { cortex_m::interrupt::CriticalSection::new() };
        EVENT_QUEUE.borrow(&cs).borrow_mut().pop_front()
    }
}

#[inline(always)]
fn wake_and_notify_activity(hw: &mut SystemHardware) {
    hw.backlight.wake();
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
    }
    hw.orchestrator.notify_activity();
}

use crate::orchestrator::record_proc_activation;

fn is_plugged_in(hw: &SystemHardware) -> bool {
    #[cfg(feature = "usb")]
    let is_usb_plugged = hw.usb.is_connected();
    #[cfg(not(feature = "usb"))]
    let is_usb_plugged = false;
    is_usb_plugged || unsafe { (*core::ptr::addr_of!(POWER_SERVICE)).is_charging() }
}

#[inline(never)]
fn drain_keypad_matrix(hw: &mut SystemHardware) {
    while drivers::keyboard::KeypadScanner::poll_matrix().is_some() {
        py32_hal::wdt::Watchdog::feed();
        py32_hal::delay::delay_ms(10);
    }
    drivers::keyboard::KeypadScanner::reset_gpio_pins();
    hw.orchestrator.reset_hold_tracking();
    hw.key_scanner.clear();
}

#[inline(never)]
fn handle_deep_sleep_mode(hw: &mut SystemHardware) {
    // 1. Play gentle descending power-down cue
    hw.radio
        .play_dialog_sound(crate::services::melody::DialogSoundType::Lock);
    py32_hal::delay::delay_ms(100);

    // 2. Shut down power-hungry subsystems
    hw.backlight.turn_off();
    hw.lcd.turn_off();
    hw.audio.disable();
    hw.radio.sleep();
    hw.radio.set_status_led(bsp::StatusLed::Off);
    #[cfg(feature = "usb")]
    {
        // Avoid USB bus disturbance during deep sleep: release soft connect and clear endpoints
        unsafe {
            use py32_hal::pac::usbd::*;
            let pwr = USBD_POWER_U8.read_volatile();
            USBD_POWER_U8.write_volatile(pwr & !USB_POWER_SOFTCONN);
        }
    }

    // 3. Mark power service profile as deep sleep
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).enter_deep_sleep();
    }

    // 4. Wait for key release before entering sleep loop to prevent immediate wake
    drain_keypad_matrix(hw);

    // 5. Downclock to direct 16 MHz HSI, shut off analog PLL, and set flash to 0WS
    // This dramatically cuts dynamic core power and eliminates PLL analog consumption.
    py32_hal::rcc::set_sysclk_16mhz();
    #[cfg(feature = "uart")]
    py32_hal::uart::set_baud(115200);

    // 6. Deep Sleep Retention Loop (WFI / Low-Power sleep)
    // Feeds watchdog periodically while keeping displays, audio PA, and RF powered down.
    // Blinks Red status LED periodically (10ms strobe every 5s) to indicate low-power sleep with minimal battery consumption.
    // Wakes ONLY when the Exit key is held down continuously for 1.2 seconds (with solid Red LED feedback) or RigWire connects.
    let mut next_blink_ms = py32_hal::delay::millis().wrapping_add(5000);
    let mut last_scan_ms = py32_hal::delay::millis();
    let mut exit_press_start: Option<u32> = None;
    const WAKE_HOLD_MS: u32 = 1200;

    'sleep_loop: loop {
        py32_hal::wdt::Watchdog::feed();

        #[cfg(feature = "uart")]
        if py32_hal::uart::is_readable() {
            break 'sleep_loop;
        }

        let now = py32_hal::delay::millis();

        // Throttle hardware keyboard matrix polling to 20ms intervals (50 Hz).
        // This eliminates continuous spin-delays on GPIOs and allows Cortex-M0+ to remain in WFI.
        if now.wrapping_sub(last_scan_ms) >= 20 {
            last_scan_ms = now;
            let key = drivers::keyboard::KeypadScanner::poll_matrix();

            if key == Some(KeyCode::Exit) {
                match exit_press_start {
                    None => {
                        exit_press_start = Some(now);
                        // Solid Red LED provides immediate tactile feedback that hold-to-wake is active
                        hw.radio.set_status_led(bsp::StatusLed::Red);
                    }
                    Some(start) => {
                        if now.wrapping_sub(start) >= WAKE_HOLD_MS {
                            break 'sleep_loop;
                        }
                    }
                }
            } else if exit_press_start.is_some() {
                // Exit released before reaching hold target: turn off feedback LED
                exit_press_start = None;
                hw.radio.set_status_led(bsp::StatusLed::Off);
            }
        }

        // Heartbeat blink only when user is not actively holding Exit
        if exit_press_start.is_none() && now.wrapping_sub(next_blink_ms) < 0x8000_0000 {
            // Heartbeat: strobe Red status LED briefly (10ms) via BK4829 GPIO1
            hw.radio.set_status_led(bsp::StatusLed::Red);
            py32_hal::delay::delay_ms(10);
            hw.radio.set_status_led(bsp::StatusLed::Off);
            next_blink_ms = now.wrapping_add(5000);
        }

        if exit_press_start.is_some() {
            py32_hal::delay::delay_ms(10);
        } else {
            cortex_m::asm::wfi();
        }
    }

    hw.radio.set_status_led(bsp::StatusLed::Off);

    // Wait for key release upon waking to prevent spurious key events
    drain_keypad_matrix(hw);

    // 7. Wakeup & Hardware Restoration
    #[cfg(feature = "uart")]
    py32_hal::uart::clear_errors();
    #[cfg(feature = "usb")]
    hw.usb.init();
    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    hw.wire.reset_collector();

    // Re-initialize LCD panel and clear dirty state
    hw.lcd.init();
    let canvas = crate::system_canvas();
    canvas.mark_all_dirty();
    hw.lcd.blit(canvas);

    // Wake RF modem and backlight
    hw.radio.wake();
    hw.backlight.wake();

    // Re-apply listening VFO channel to RF hardware
    {
        use kernel::service::radio::RfChannel;
        hw.radio
            .configure_channel(crate::radio_state().coord.listening_channel());
    }

    // Restore clocks, UART baud, audio, and display hardware from master config
    crate::master_config().hw_dirty = 0x1F;
    hw.apply_dirty_configs();

    // Sound unlock confirmation chime
    hw.radio
        .play_dialog_sound(crate::services::melody::DialogSoundType::Unlock);

    // Ensure matrix is clean before returning to event dispatch
    drain_keypad_matrix(hw);

    // Reset power service inactivity tracking
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

fn handle_one_second_tick(hw: &mut SystemHardware, low_batt_warning_timer: &mut u8) {
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).tick_second();
    }

    let pwr = unsafe { &*core::ptr::addr_of!(POWER_SERVICE) };
    if pwr.is_low() && !pwr.is_charging() && pwr.uptime_secs() > 5 && pwr.battery_percentage() <= 15
    {
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
    #[cfg(feature = "radio")]
    let mut last_radio_tick = py32_hal::delay::millis();
    #[cfg(feature = "grayscale")]
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
                match usb_ev {
                    kernel::event::UsbEvent::Connected | kernel::event::UsbEvent::Resumed => {
                        wake_and_notify_activity(hw);
                        if py32_hal::rcc::sysclk_hz() != 48_000_000 {
                            py32_hal::rcc::set_sysclk_48mhz();
                            #[cfg(feature = "uart")]
                            bsp::Board::uart_set_baud(
                                crate::master_config_ref().system.baud_rate(),
                            );
                        }
                    }
                    kernel::event::UsbEvent::Disconnected => {
                        let target_clk = crate::master_config_ref().system.cpu_clock;
                        py32_hal::rcc::set_sysclk_index(target_clk);
                        #[cfg(feature = "uart")]
                        bsp::Board::uart_set_baud(crate::master_config_ref().system.baud_rate());
                    }
                    _ => {}
                }
                enqueue_event(Event::Usb(usb_ev));
            }
        }

        let now = py32_hal::delay::millis();

        // 0b. Poll RigWire serial protocol (UART or USB CDC)
        #[cfg(any(feature = "uart", feature = "usb_cdc"))]
        if let Some((header, _raw_len)) = hw.wire.poll(now) {
            wake_and_notify_activity(hw);
            dirty_render = true;
            if !crate::services::wire::dispatch_wire_record(&header, hw, now) {
                enqueue_event(Event::Sys(kernel::ipc::SysEvent {
                    target: header.service,
                    command: header.opcode,
                    arg: header.sequence as u16,
                }));
            }
        }

        // 0c. High-priority Radio Coordinator & Squelch Tail Elimination (STE) tick every 10ms
        #[cfg(feature = "radio")]
        if now.wrapping_sub(last_radio_tick) >= 10 {
            last_radio_tick = now;
            hw.tick_radio_services();
        }

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
            record_proc_activation(0xF3);
            hw.key_scanner.scan_with_dt(dt);
            while let Some((code, state)) = hw.key_scanner.pop_event() {
                let is_locked = hw.orchestrator.is_keypad_locked();
                let should_wake = !is_locked || matches!(code, KeyCode::Ptt);

                if should_wake {
                    hw.backlight.wake();
                }
                unsafe {
                    (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
                }

                enqueue_event(Event::Key { code, state });
            }
        }

        let is_display_awake = hw.backlight.is_active();
        let is_anim = hw.orchestrator.is_animating() && is_display_awake;

        // Keep microkernel PowerService state in sync with display activity
        unsafe {
            let pwr = &mut *core::ptr::addr_of_mut!(POWER_SERVICE);
            if is_display_awake {
                pwr.state = kernel::power::PowerState::Active;
            } else if pwr.state != kernel::power::PowerState::DeepSleep {
                pwr.state = kernel::power::PowerState::Standby;
            }
        }

        // 2. Fully Adaptive Refresh Rate & Physical LCD Timing:
        // - Active UI animations: tick interval derived from active display refresh_rate:
        //   0 (20 Hz): 50ms tick (Battery saving)
        //   1 (60 Hz): 16ms tick (Std, matches f4hwn flashmap 15-16ms LC response time)
        //   2 (75 Hz): 13ms tick (Turbo, matches ST7565 internal panel scan ~76 Hz)
        //   3 (90 Hz): 11ms tick (Ultra)
        // - Screen awake & idle: 50ms tick (20 Hz)
        // - Screen asleep (Standby): 100ms tick (10 Hz) to maximize Cortex-M0+ low-power sleep.
        #[cfg(feature = "animations")]
        let ui_tick_interval = if is_anim {
            match crate::master_config_ref().display.refresh_rate {
                0 => 50u32,
                1 => 16u32,
                2 => 13u32,
                3 => 11u32,
                _ => 16u32,
            }
        } else if is_display_awake {
            50u32
        } else {
            100u32
        };
        #[cfg(not(feature = "animations"))]
        let ui_tick_interval = if is_display_awake { 50u32 } else { 100u32 };

        if now.wrapping_sub(last_ui_tick) >= ui_tick_interval {
            let delta = now.wrapping_sub(last_ui_tick);
            last_ui_tick = now;
            record_proc_activation(0xF0);
            hw.backlight.tick(delta as u16);
            hw.audio.tick(delta as u16);
            enqueue_event(Event::Tick);
        }

        // High-speed temporal FRC subframe refresh:
        // When in Grayscale mode (shading_mode == 0), display is awake, not actively animating,
        // and no canvas re-render is pending:
        // Pushes temporal subframes at rock-solid 2ms intervals (500 Hz subframe / 166.7 Hz optical cycle).
        #[cfg(feature = "grayscale")]
        if is_display_awake
            && !is_anim
            && !dirty_render
            && crate::master_config_ref().display.shading_mode == 0
            && now.wrapping_sub(last_subframe_tick) >= 2
        {
            last_subframe_tick = now;
            hw.refresh_subframe();
        }

        // 3. Periodic ADC battery/temp sampling:
        // - 200ms (5 Hz) when display is awake or audio active for rapid meter updates & charge detection
        // - 1000ms (1 Hz) when screen is asleep to cut ADC activations by 80%
        let batt_interval = if is_display_awake || hw.audio.is_active() {
            200u32
        } else {
            1000u32
        };
        if now.wrapping_sub(last_battery_tick) >= batt_interval {
            last_battery_tick = now;
            record_proc_activation(0xF2);
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

            // Check for low battery edge transition -> display warning modal & sound alert
            let low_bat_edge =
                unsafe { (*core::ptr::addr_of_mut!(POWER_SERVICE)).poll_low_battery_transition() };
            if low_bat_edge {
                let pwr = unsafe { &*core::ptr::addr_of!(POWER_SERVICE) };
                if pwr.uptime_secs() >= 3 && pwr.battery_percentage() <= 15 {
                    wake_and_notify_activity(hw);
                    hw.radio
                        .play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
                    crate::orchestrator::compositor::show_low_battery_popup();
                    dirty_render = true;
                }
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

            let is_plugged = is_plugged_in(hw);

            // Reset inactivity if backlight is on, audio amplifier is active, or radio is plugged into USB/charger
            if is_plugged || hw.backlight.is_active() || hw.audio.is_active() {
                unsafe {
                    (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
                }
            }

            // Auto-Power-Off (APO) timeout -> automatically enter deep sleep
            // Strictly guarded: only fires if user explicitly configured APO in settings (> 0)
            // and device is NOT plugged into USB or charging.
            let apo_fired = unsafe {
                let pwr = &mut *core::ptr::addr_of_mut!(POWER_SERVICE);
                let fired = pwr.apo_triggered;
                pwr.apo_triggered = false;
                fired
            };
            if apo_fired && !is_plugged && crate::master_config_ref().battery.auto_power_off_min > 0
            {
                handle_deep_sleep_mode(hw);
            }
        }

        while let Some(ev) = dequeue_event() {
            let consumed = hw.dispatch_event_and_actions(ev);
            match ev {
                Event::Tick => {
                    if consumed || is_anim {
                        dirty_render = true;
                    }
                }
                _ => {
                    dirty_render = true;
                }
            }
        }

        // Check if deep sleep mode was requested (e.g. via long-press Exit).
        if hw.orchestrator.take_sleep_requested() {
            hw.render_frame();
            handle_deep_sleep_mode(hw);
            dirty_render = true;
        }

        // Render loop: skip expensive CPU view traversal and 1024-byte SPI blits when display is dark
        if dirty_render && is_display_awake {
            hw.render_frame();
            dirty_render = false;
            #[cfg(feature = "grayscale")]
            {
                last_subframe_tick = py32_hal::delay::millis();
            }
        }

        // Sleep until next event/interrupt, unless USB is actively connected or has pending transfer,
        // high-speed animation is active, or Grayscale FRC is actively cycling subframes while display is awake
        #[cfg(feature = "usb")]
        if hw.usb.is_connected() || hw.usb.has_pending_transfer() {
            continue;
        }

        #[cfg(feature = "animations")]
        if is_anim {
            continue;
        }

        #[cfg(feature = "grayscale")]
        if is_display_awake && crate::master_config_ref().display.shading_mode == 0 {
            continue;
        }

        // Sleep until next event/interrupt
        cortex_m::asm::wfi();
    }
}
