//! System Event Loop & Main Execution Engine.

use crate::hardware::SystemHardware;
use crate::services::RadioMelodyExt;
use crate::services::power::PowerService;
use crate::{EVENT_QUEUE, POWER_SERVICE};
use kernel::event::Event;
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

/// Headless mode flag: physically powers down ST7565 panel & backlight and suppresses all
/// display rendering / SPI blits for minimal battery draw while keeping full RigWire control.
static mut HEADLESS_MODE: bool = false;

#[inline(always)]
pub fn is_headless() -> bool {
    unsafe { *core::ptr::addr_of!(HEADLESS_MODE) }
}

/// Enable or disable zero-overhead headless remote-control mode.
#[inline(never)]
pub fn set_headless(hw: &mut SystemHardware, enable: bool) {
    let was = is_headless();
    unsafe {
        *core::ptr::addr_of_mut!(HEADLESS_MODE) = enable;
    }
    if enable {
        hw.backlight.turn_off();
        hw.lcd.enter_sleep();
    } else if was {
        hw.lcd.exit_sleep();
        hw.backlight.wake();
        hw.render_frame();
    }
}

#[inline(never)]
fn wake_and_notify_activity(hw: &mut SystemHardware) {
    if !is_headless() {
        hw.backlight.wake();
    }
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
    }
    hw.orchestrator.notify_activity();
}

use crate::orchestrator::record_proc_activation;

/// Hold-off window (ms) after the last valid RigWire frame during which Deep Sleep and APO
/// are inhibited so a remote operator over USB-C or 2-pin UART cannot lock themselves out.
const REMOTE_SLEEP_INHIBIT_MS: u32 = 30_000;
static mut LAST_WIRE_ACTIVITY_MS: u32 = 0;

#[inline(always)]
fn should_inhibit_deep_sleep(
    usb_connected: bool,
    wire_session_id: u32,
    last_wire_ms: u32,
    now: u32,
) -> bool {
    if usb_connected {
        return true;
    }
    wire_session_id != 0
        && last_wire_ms != 0
        && now.wrapping_sub(last_wire_ms) < REMOTE_SLEEP_INHIBIT_MS
}

#[allow(unused_variables)]
fn is_remote_control_active(hw: &SystemHardware, now: u32) -> bool {
    #[cfg(feature = "usb")]
    let usb_connected = hw.usb.is_connected();
    #[cfg(not(feature = "usb"))]
    let usb_connected = false;

    #[cfg(any(feature = "uart", feature = "usb_cdc"))]
    let (session_id, last_ms) = (hw.wire.session.active_session_id, unsafe {
        *core::ptr::addr_of!(LAST_WIRE_ACTIVITY_MS)
    });
    #[cfg(not(any(feature = "uart", feature = "usb_cdc")))]
    let (session_id, last_ms) = (0u32, 0u32);

    should_inhibit_deep_sleep(usb_connected, session_id, last_ms, now)
}

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
    // Hardened remote-control guard: never sever an active USB-C or RigWire remote link
    // by entering STOP mode (which disconnects USB and requires a physical button to wake).
    if is_remote_control_active(hw, py32_hal::delay::millis()) {
        hw.orchestrator.reset_hold_tracking();
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_toast("Remote active: sleep locked", 1800);
        }
        return;
    }

    // 1. Play gentle descending power-down cue
    hw.radio
        .play_dialog_sound(crate::services::melody::DialogSoundType::Lock);
    py32_hal::delay::delay_ms(100);

    // Clear any active hold UI before shutting down display
    hw.orchestrator.reset_hold_tracking();

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

    // Restore clocks, UART baud, audio, and display hardware
    crate::master_config().hw_dirty = 0x1F;
    hw.apply_dirty_configs();

    // Reset hold tracking so no leftover hold sheet or toast lingers
    hw.orchestrator.reset_hold_tracking();

    // Wake LCD first so SPI blit succeeds
    hw.lcd.exit_sleep();

    // Render clean UI frame into the awake LCD
    hw.render_frame();

    // Immediately wake backlight so screen appears instantly to the user
    hw.backlight.wake();

    // Wake RF modem and re-apply listening VFO channel to RF hardware
    hw.radio.wake();
    {
        use kernel::service::radio::RfChannel;
        hw.radio
            .configure_channel(crate::radio_state().coord.listening_channel());
    }

    // Sound unlock confirmation chime with display already active and responsive
    hw.radio
        .play_dialog_sound(crate::services::melody::DialogSoundType::Unlock);

    // Ensure matrix is clean before returning to event dispatch
    drain_keypad_matrix(hw);

    // Reset power service inactivity tracking
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
    let is_remote = is_remote_control_active(hw, py32_hal::delay::millis());
    if is_plugged || is_remote || hw.backlight.is_active() || hw.audio.is_active() {
        pwr.notify_activity();
    }

    let apo_fired = pwr.apo_triggered;
    pwr.apo_triggered = false;
    if apo_fired
        && !is_plugged
        && !is_remote
        && crate::master_config_ref().battery.auto_power_off_min > 0
    {
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

    #[cfg(all(feature = "wire", any(feature = "uart", feature = "usb_cdc")))]
    for _ in 0..4 {
        let Some((header, _raw_len)) = hw.wire.poll(now) else {
            break;
        };
        unsafe {
            *core::ptr::addr_of_mut!(LAST_WIRE_ACTIVITY_MS) = now.max(1);
        }
        let is_passive_read = header.service == kernel::wire::SERVICE_CONTROL
            && (header.opcode == kernel::wire::OP_HANDLE_GET
                || header.opcode == kernel::wire::OP_HANDLE_STREAM);
        if !is_passive_read {
            wake_and_notify_activity(hw);
            *dirty_render = true;
        }
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
        if !hw.orchestrator.is_keypad_locked() {
            if !is_headless() {
                hw.backlight.wake();
            }
            unsafe {
                (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
            }
        }
        enqueue_event(Event::Key { code, state });
    }
}

#[inline(never)]
fn timer_due(last: &mut u32, now: u32, period: u32) -> Option<u32> {
    let dt = now.wrapping_sub(*last);
    if dt >= period {
        *last = now;
        Some(dt)
    } else {
        None
    }
}

#[inline(never)]
fn drain_events_and_render(
    hw: &mut SystemHardware,
    is_anim: bool,
    is_ui_live: bool,
    dirty_render: &mut bool,
    dimmed_dirty: &mut bool,
) -> bool {
    while let Some(ev) = dequeue_event() {
        let had_lock = hw.orchestrator.is_lock_overlay_active();
        let consumed = hw.dispatch_event_and_actions(&ev);
        if had_lock || hw.orchestrator.is_lock_overlay_active() {
            *dimmed_dirty = true;
        }
        if !matches!(ev, Event::Tick) || consumed || is_anim {
            *dirty_render = true;
        }
    }

    if hw.orchestrator.take_sleep_requested() {
        handle_deep_sleep_mode(hw);
        *dirty_render = true;
    }

    if *dirty_render && (is_ui_live || *dimmed_dirty) {
        hw.render_frame();
        *dirty_render = false;
        *dimmed_dirty = false;
        return true;
    }
    false
}

/// Returns `(is_ui_live, is_anim)` for the UI tick scheduler.
///
/// The UI is observed either on the panel or remotely over RigWire (headless), so tick
/// cadence follows `is_ui_live`, never the panel power state alone.
#[inline(always)]
fn ui_cadence(
    headless: bool,
    display_awake: bool,
    ui_animating: bool,
    lock_animating: bool,
) -> (bool, bool) {
    let is_ui_live = display_awake || headless;
    (is_ui_live, (ui_animating && is_ui_live) || lock_animating)
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
    let mut dirty_render = false;
    let mut dimmed_dirty = false;
    let mut crash_loop_cleared = false;
    let mut low_batt_warning_timer = 0u8;

    // Complete initial OS frame before entering the event loop so LCD never stays blank.
    hw.render_frame();
    hw.lcd.turn_on();
    hw.backlight.wake();
    #[cfg(feature = "grayscale")]
    {
        last_subframe_tick = py32_hal::delay::millis();
    }

    loop {
        py32_hal::wdt::Watchdog::feed();

        let now = py32_hal::delay::millis();
        poll_usb_and_wire(hw, now, &mut dirty_render);

        // High-priority Radio Coordinator & Squelch Tail Elimination (STE) tick every 10ms
        #[cfg(feature = "radio")]
        if timer_due(&mut last_radio_tick, now, 10).is_some()
            && !crate::orchestrator::is_proc_paused(0xF1)
        {
            hw.tick_radio_services();
        }

        // Clear crash-loop strike counter only after >= 10 s of healthy main loop execution (§38.4)
        if !crash_loop_cleared && now.wrapping_sub(_boot_start_ms) >= 10_000 {
            crate::boot::clear_crash_strikes();
            crash_loop_cleared = true;
        }

        // 1. Poll hardware keyboard matrix every 5ms
        if let Some(dt) = timer_due(&mut last_key_tick, now, 5) {
            poll_keypad_tick(hw, dt.min(50) as u16);
        }

        let is_display_awake = !is_headless() && hw.backlight.is_active();
        let (is_ui_live, is_anim) = ui_cadence(
            is_headless(),
            is_display_awake,
            hw.orchestrator.is_animating(),
            hw.orchestrator.is_lock_overlay_animating(),
        );

        // Keep microkernel PowerService state in sync with display activity
        unsafe {
            let pwr = &mut *core::ptr::addr_of_mut!(POWER_SERVICE);
            if is_display_awake {
                pwr.state = kernel::power::PowerState::Active;
            } else if pwr.state != kernel::power::PowerState::DeepSleep {
                pwr.state = kernel::power::PowerState::Standby;
            }
        }

        // 2. Adaptive Refresh Rate & UI Tick
        #[cfg(feature = "animations")]
        let ui_tick_interval = if is_anim {
            crate::config::display::anim_step_ms(crate::master_config_ref().display.refresh_rate)
        } else if is_ui_live {
            50u32
        } else {
            100u32
        };
        #[cfg(not(feature = "animations"))]
        let ui_tick_interval = if is_ui_live { 50u32 } else { 100u32 };

        if let Some(delta) = timer_due(&mut last_ui_tick, now, ui_tick_interval) {
            record_proc_activation(0xF0);
            hw.backlight.tick(delta as u16);
            hw.audio.tick(delta as u16);
            enqueue_event(Event::Tick);
        }

        // High-speed temporal FRC subframe refresh (Grayscale mode)
        #[cfg(feature = "grayscale")]
        if is_display_awake
            && !is_anim
            && !dirty_render
            && crate::master_config_ref().display.shading_mode == 0
            && timer_due(&mut last_subframe_tick, now, 2).is_some()
        {
            hw.refresh_subframe();
        }

        // 3. Periodic ADC battery/temp sampling (200ms awake/audio, 1000ms asleep)
        let batt_interval = if is_display_awake || hw.audio.is_active() {
            200u32
        } else {
            1000u32
        };
        if timer_due(&mut last_battery_tick, now, batt_interval).is_some()
            && !crate::orchestrator::is_proc_paused(0xF2)
        {
            poll_battery_tick(hw, &adc, &mut dirty_render);
        }

        // 4. 1-second system tick for uptime, telemetry, activity & Auto-Power-Off (APO)
        if timer_due(&mut last_tick, now, 1000).is_some() {
            handle_one_second_tick(hw, &mut low_batt_warning_timer);
            dirty_render = true;
        }

        let rendered = drain_events_and_render(
            hw,
            is_anim,
            is_ui_live,
            &mut dirty_render,
            &mut dimmed_dirty,
        );
        #[cfg(feature = "grayscale")]
        if rendered {
            last_subframe_tick = py32_hal::delay::millis();
        }
        #[cfg(not(feature = "grayscale"))]
        let _ = rendered;

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

        cortex_m::asm::wfi();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_ui_runs_at_the_same_cadence_as_the_panel() {
        // Panel on and animating.
        assert_eq!(ui_cadence(false, true, true, false), (true, true));
        // Headless: panel is off but the UI is watched remotely, so cadence is identical.
        assert_eq!(ui_cadence(true, false, true, false), (true, true));
        assert_eq!(ui_cadence(true, false, false, false), (true, false));
        // Panel asleep and not headless: idle cadence, no animation ticks.
        assert_eq!(ui_cadence(false, false, true, false), (false, false));
        // The lock overlay animates regardless of panel state.
        assert_eq!(ui_cadence(false, false, false, true), (false, true));
    }

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

    #[test]
    fn test_remote_sleep_inhibit_window() {
        // Active USB connection always inhibits deep sleep
        assert!(should_inhibit_deep_sleep(true, 0, 0, 1000));
        // Disconnected USB with no RigWire session allows deep sleep
        assert!(!should_inhibit_deep_sleep(false, 0, 0, 1000));
        // Active RigWire session over UART inhibits sleep inside 30 s window
        assert!(should_inhibit_deep_sleep(
            false,
            0x4B45_524E,
            10_000,
            39_999
        ));
        // Once idle >= 30 s with USB disconnected, deep sleep is permitted again
        assert!(!should_inhibit_deep_sleep(
            false,
            0x4B45_524E,
            10_000,
            40_000
        ));
        // Handles u32 uptime wraparound cleanly
        assert!(should_inhibit_deep_sleep(
            false,
            0x4B45_524E,
            u32::MAX - 5_000,
            10_000
        ));
        assert!(!should_inhibit_deep_sleep(
            false,
            0x4B45_524E,
            u32::MAX - 5_000,
            30_000
        ));
    }
}
