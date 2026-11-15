//! System Event Loop & Main Execution Engine.

use crate::hardware::SystemHardware;
use crate::services::RadioMelodyExt;
use crate::services::power::PowerService;
use crate::{EVENT_QUEUE, POWER_SERVICE};
use kernel::event::{Event, KeyCode};
use py32_hal::adc::Adc;

#[inline(never)]
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

#[inline(never)]
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

#[inline(never)]
fn wake_and_notify_activity(hw: &mut SystemHardware) {
    hw.backlight.wake();
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
    }
    hw.orchestrator.notify_activity();
}

use crate::orchestrator::record_proc_activation;

#[allow(unused_variables)]
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

const WAKE_HOLD_STEPS: u8 = 20; // ~20 x 50 ms (~1.0 s) hold to wake
const LPTIM_IDLE_TICKS: u16 = 1536; // ~1.5 s @ 1024 Hz LSI/32 (>2x safety margin under 6.0 s IWDG across LSI spread)
const LPTIM_HOLD_TICKS: u16 = 52; // ~50 ms @ 1024 Hz

/// Advances hold-to-wake state for `KeyCode::Exit` during STOP sleep.
/// Returns `None` once held across `WAKE_HOLD_STEPS`, or `Some(lptim_ticks)` for the next sleep window.
#[inline(always)]
fn next_stop_sleep_ticks(exit_held: bool, hold_steps: &mut u8) -> Option<u16> {
    if exit_held {
        *hold_steps = hold_steps.saturating_add(1);
        if *hold_steps >= WAKE_HOLD_STEPS {
            return None;
        }
        Some(LPTIM_HOLD_TICKS)
    } else {
        *hold_steps = 0;
        Some(LPTIM_IDLE_TICKS)
    }
}

/// Latched `EXTI4_15` pending mask captured inside ISR for STOP wakeup dispatch.
pub static WAKE_EXTI_FLAGS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[inline(never)]
fn handle_deep_sleep_mode(hw: &mut SystemHardware) {
    // 1. Play gentle descending power-down cue
    hw.radio
        .play_dialog_sound(crate::services::melody::DialogSoundType::Lock);
    py32_hal::delay::delay_ms(100);

    // 2. Shut down power-hungry subsystems and enter chip standby states
    hw.backlight.turn_off();
    hw.lcd.enter_sleep();
    hw.audio.disable();
    hw.radio.sleep();
    hw.radio.set_status_led(bsp::StatusLed::Off);
    bsp::pins::FLASHLIGHT.set_low();
    #[cfg(feature = "filesystem")]
    hw.filesystem.fs.dev.deep_power_down();
    #[cfg(feature = "usb")]
    hw.usb.disconnect_for_sleep();

    // 3. Mark power service profile as deep sleep
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).enter_deep_sleep();
    }

    // 4. Wait for key release before entering sleep loop to prevent immediate wake
    drain_keypad_matrix(hw);

    // 5. Downclock to direct 16 MHz HSI, shut off analog PLL, and set flash to 0WS
    // (automatically resynchronizes USART1 BRR to 16 MHz).
    py32_hal::rcc::set_sysclk_16mhz();

    // 6. Park peripheral/matrix pins and arm LPTIM1 + EXTI15 (Exit) [+ EXTI10 UART] for true STOP mode
    run_stop_sleep_cycle();

    #[cfg(feature = "filesystem")]
    hw.filesystem.fs.dev.release_power_down();

    #[cfg(feature = "uart")]
    py32_hal::uart::clear_errors();
    #[cfg(feature = "usb")]
    hw.usb.init();
    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    hw.wire.reset_collector();

    // Restore clocks, UART baud, audio, and display hardware, render clean UI, then exit LCD sleep
    crate::master_config().hw_dirty = 0x1F;
    hw.apply_dirty_configs();
    hw.render_frame();
    hw.lcd.exit_sleep();

    // Wake RF modem and re-apply listening VFO channel to RF hardware
    hw.radio.wake();
    {
        use kernel::service::radio::RfChannel;
        hw.radio
            .configure_channel(crate::radio_state().coord.listening_channel());
    }

    // Sound unlock confirmation chime
    hw.radio
        .play_dialog_sound(crate::services::melody::DialogSoundType::Unlock);

    // Ensure matrix is clean before returning to event dispatch
    drain_keypad_matrix(hw);

    // Wake backlight and reset power service inactivity tracking
    wake_and_notify_activity(hw);
}

#[inline(never)]
fn run_stop_sleep_cycle() {
    py32_hal::lptim::LowPowerTimer::init();
    let saved_pins = bsp::pins::park_for_stop(cfg!(feature = "uart"));

    py32_hal::exti::select_port(15, py32_hal::exti::Port::GpioB);
    py32_hal::exti::enable_line(15, py32_hal::exti::TriggerEdge::Falling);
    #[cfg(feature = "uart")]
    {
        py32_hal::exti::select_port(10, py32_hal::exti::Port::GpioA);
        py32_hal::exti::enable_line(10, py32_hal::exti::TriggerEdge::Falling);
    }
    let exti_mask: u32 = if cfg!(feature = "uart") {
        (1 << 15) | (1 << 10)
    } else {
        1 << 15
    };
    let wakeup_mask: u32 = exti_mask | (1 << py32_hal::exti::LINE_LPTIM1);
    py32_hal::exti::enable_lines(1 << py32_hal::exti::LINE_LPTIM1);
    py32_hal::exti::clear_pending_mask(wakeup_mask);

    use py32_hal::pac::Interrupt;
    unsafe {
        py32_hal::pac::nvic_clear_pending(Interrupt::EXTI4_15 as u8);
        py32_hal::pac::nvic_clear_pending(Interrupt::TIM6_LPTIM1_DAC as u8);
        py32_hal::pac::nvic_enable(Interrupt::EXTI4_15 as u8);
        py32_hal::pac::nvic_enable(Interrupt::TIM6_LPTIM1_DAC as u8);
    }

    let mut exit_hold_steps: u8 = 0;

    'sleep_loop: loop {
        py32_hal::wdt::Watchdog::feed();

        let wake_flags = WAKE_EXTI_FLAGS.load(core::sync::atomic::Ordering::Acquire);
        let mut exit_held = bsp::pins::is_exit_held_parked();
        if !exit_held && (wake_flags & (1 << 15)) != 0 {
            cortex_m::asm::delay(160_000);
            exit_held = bsp::pins::is_exit_held_parked();
        }

        let Some(mut sleep_ticks) = next_stop_sleep_ticks(exit_held, &mut exit_hold_steps) else {
            break 'sleep_loop;
        };
        if !exit_held && (wake_flags & (1 << 15)) != 0 {
            sleep_ticks = LPTIM_HOLD_TICKS;
        }

        cortex_m::interrupt::free(|_| {
            WAKE_EXTI_FLAGS.store(0, core::sync::atomic::Ordering::Release);
            py32_hal::lptim::LowPowerTimer::clear_expired();
            py32_hal::exti::clear_pending_mask(wakeup_mask);
            unsafe {
                py32_hal::pac::nvic_clear_pending(Interrupt::EXTI4_15 as u8);
                py32_hal::pac::nvic_clear_pending(Interrupt::TIM6_LPTIM1_DAC as u8);
            }
            py32_hal::lptim::LowPowerTimer::start_single_shot_irq(sleep_ticks, true);
            py32_hal::pwr::enter_stop();
        });

        py32_hal::wdt::Watchdog::feed();

        #[cfg(feature = "uart")]
        if (WAKE_EXTI_FLAGS.load(core::sync::atomic::Ordering::Acquire) & (1 << 10)) != 0 {
            py32_hal::delay::delay_ms(2);
            if py32_hal::uart::is_readable() {
                break 'sleep_loop;
            }
        }
    }

    // 7. Disarm sleep wake IRQs and restore GPIOs
    unsafe {
        py32_hal::pac::nvic_disable(Interrupt::EXTI4_15 as u8);
        py32_hal::pac::nvic_disable(Interrupt::TIM6_LPTIM1_DAC as u8);
    }
    py32_hal::exti::disable_lines(wakeup_mask);
    py32_hal::lptim::LowPowerTimer::stop();

    bsp::pins::restore_from_stop(&saved_pins);
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

        let led = if ((cur >> 8) & 0x07) == 0 {
            bsp::StatusLed::Red
        } else {
            bsp::StatusLed::Off
        };
        hw.radio.set_status_led(led);

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

#[inline(never)]
fn handle_one_second_tick(hw: &mut SystemHardware, low_batt_warning_timer: &mut u8) {
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).tick_second();
    }

    let pwr = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };
    if pwr.is_low() && !pwr.is_charging() && pwr.uptime_secs() > 5 && pwr.battery_percentage() <= 15
    {
        *low_batt_warning_timer = low_batt_warning_timer.saturating_add(1);
        if *low_batt_warning_timer >= 30 {
            *low_batt_warning_timer = 0;
            if crate::master_config_ref().audio.key_beep != 0 {
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

    let is_plugged = is_plugged_in(hw);
    if is_plugged || hw.backlight.is_active() || hw.audio.is_active() {
        pwr.notify_activity();
    }

    let apo_fired = pwr.apo_triggered;
    pwr.apo_triggered = false;
    if apo_fired && !is_plugged && crate::master_config_ref().battery.auto_power_off_min > 0 {
        handle_deep_sleep_mode(hw);
    }
}

#[allow(unused_variables)]
#[inline(never)]
fn poll_usb_and_wire(hw: &mut SystemHardware, now: u32, dirty_render: &mut bool) {
    #[cfg(feature = "usb")]
    if let Some(usb_ev) = hw.usb.poll() {
        *dirty_render = true;
        let new_clk = match usb_ev {
            kernel::event::UsbEvent::Connected | kernel::event::UsbEvent::Resumed => {
                wake_and_notify_activity(hw);
                (py32_hal::rcc::sysclk_hz() != 48_000_000).then_some(1)
            }
            kernel::event::UsbEvent::Disconnected => {
                Some(crate::master_config_ref().system.cpu_clock)
            }
            _ => None,
        };
        if let Some(clk_idx) = new_clk {
            py32_hal::rcc::set_sysclk_index(clk_idx);
            #[cfg(feature = "uart")]
            bsp::Board::uart_set_baud(crate::master_config_ref().system.baud_rate());
        }
        enqueue_event(Event::Usb(usb_ev));
    }

    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    if let Some((header, _raw_len)) = hw.wire.poll(now) {
        wake_and_notify_activity(hw);
        *dirty_render = true;
        if !crate::services::wire::dispatch_wire_record(&header, hw, now) {
            enqueue_event(Event::Sys(kernel::ipc::SysEvent {
                target: header.service,
                command: header.opcode,
                arg: header.sequence as u16,
            }));
        }
    }
}

#[inline(never)]
fn poll_battery_tick(hw: &mut SystemHardware, adc: &Adc, dirty_render: &mut bool) {
    record_proc_activation(0xF2);
    crate::services::telemetry::sample_battery_and_system(adc, hw.audio.is_active());

    let pwr = unsafe { &mut *core::ptr::addr_of_mut!(POWER_SERVICE) };

    if let Some(is_charging) = pwr.poll_charging_transition() {
        if is_charging {
            wake_and_notify_activity(hw);
        }
        *dirty_render = true;
        enqueue_event(Event::Charging(is_charging));
    }

    if pwr.poll_low_battery_transition() && pwr.uptime_secs() >= 3 && pwr.battery_percentage() <= 15
    {
        wake_and_notify_activity(hw);
        hw.radio
            .play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
        crate::orchestrator::compositor::show_low_battery_popup();
        *dirty_render = true;
    }

    if pwr.is_critical() && !pwr.is_charging() {
        handle_critical_battery_cutoff(hw, adc);
        *dirty_render = true;
    }
}

#[inline(never)]
fn poll_keypad_tick(hw: &mut SystemHardware, dt: u16) {
    record_proc_activation(0xF3);
    hw.key_scanner.scan_with_dt(dt);
    while let Some((code, state)) = hw.key_scanner.pop_event() {
        if !hw.orchestrator.is_keypad_locked() || matches!(code, KeyCode::Ptt) {
            hw.backlight.wake();
        }
        unsafe {
            (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
        }
        enqueue_event(Event::Key { code, state });
    }
}

pub fn run_system_loop(hw: &mut SystemHardware, adc: Adc, _boot_start_ms: u32) -> ! {
    let t0 = py32_hal::delay::millis();
    let mut last_tick = t0;
    let mut last_battery_tick = t0;
    let mut last_key_tick = t0;
    let mut last_ui_tick = t0;
    #[cfg(feature = "radio")]
    let mut last_radio_tick = t0;
    #[cfg(feature = "grayscale")]
    let mut last_subframe_tick = t0;
    let mut dirty_render = true;
    let mut crash_loop_cleared = false;
    let mut low_batt_warning_timer = 0u8;

    loop {
        // Feed Independent Watchdog
        py32_hal::wdt::Watchdog::feed();

        let now = py32_hal::delay::millis();
        poll_usb_and_wire(hw, now, &mut dirty_render);

        // 0c. High-priority Radio Coordinator & Squelch Tail Elimination (STE) tick every 10ms
        #[cfg(feature = "radio")]
        if now.wrapping_sub(last_radio_tick) >= 10 {
            last_radio_tick = now;
            hw.tick_radio_services();
        }

        // Clear crash-loop strike counter only after >= 10 s of healthy main loop execution (§38.4)
        if !crash_loop_cleared && now.wrapping_sub(_boot_start_ms) >= 10_000 {
            crate::boot::clear_crash_strikes();
            crash_loop_cleared = true;
        }

        // 1. Poll hardware keyboard matrix every 5ms (DeltaFW / fagci-k1-fw standard)
        if now.wrapping_sub(last_key_tick) >= 5 {
            let dt = now.wrapping_sub(last_key_tick).min(50) as u16;
            last_key_tick = now;
            poll_keypad_tick(hw, dt);
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
            const RATE_MS: [u8; 4] = [50, 16, 13, 11];
            let r = crate::master_config_ref().display.refresh_rate;
            RATE_MS[if r < 4 { r as usize } else { 1 }] as u32
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
            poll_battery_tick(hw, &adc, &mut dirty_render);
        }

        // 4. 1-second system tick for uptime, telemetry, activity & Auto-Power-Off (APO)
        if now.wrapping_sub(last_tick) >= 1000 {
            last_tick = now;
            handle_one_second_tick(hw, &mut low_batt_warning_timer);
            dirty_render = true;
        }

        while let Some(ev) = dequeue_event() {
            let consumed = hw.dispatch_event_and_actions(ev);
            if !matches!(ev, Event::Tick) || consumed || is_anim {
                dirty_render = true;
            }
        }

        // Check if deep sleep mode was requested (e.g. via long-press Exit).
        if hw.orchestrator.take_sleep_requested() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stop_hold_to_wake_transitions() {
        let mut steps = 0u8;
        assert_eq!(
            next_stop_sleep_ticks(false, &mut steps),
            Some(LPTIM_IDLE_TICKS)
        );
        assert_eq!(steps, 0);

        for expected in 1..WAKE_HOLD_STEPS {
            assert_eq!(
                next_stop_sleep_ticks(true, &mut steps),
                Some(LPTIM_HOLD_TICKS)
            );
            assert_eq!(steps, expected);
        }

        // Releasing before threshold drops immediately back to idle STOP ticks
        assert_eq!(
            next_stop_sleep_ticks(false, &mut steps),
            Some(LPTIM_IDLE_TICKS)
        );
        assert!(
            LPTIM_IDLE_TICKS <= 3500,
            "Keep STOP sleep window well under 6s IWDG"
        );
        assert_eq!(steps, 0);

        // Holding through full threshold triggers wakeup (`None`)
        for _ in 1..WAKE_HOLD_STEPS {
            assert_eq!(
                next_stop_sleep_ticks(true, &mut steps),
                Some(LPTIM_HOLD_TICKS)
            );
        }
        assert_eq!(next_stop_sleep_ticks(true, &mut steps), None);
    }
}
