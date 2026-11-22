//! System event loop & main execution engine.
//!
//! * [`events`]   — interrupt-safe event queue
//! * [`headless`] — headless node mode (wire-only control plane)
//! * [`remote`]   — RigWire liveness, sleep inhibition, TX link-loss failsafe
//! * [`power`]    — Deep Sleep / STOP, critical battery cutoff, APO

mod events;
mod headless;
mod power;
mod remote;

pub use events::{dequeue_event, enqueue_event};
pub use headless::{
    admits_physical_input, drain_headless_notes, is_headless, push_headless_notes,
    restore_from_backup, restore_from_config, set_headless,
};
pub use power::WAKE_EXTI_FLAGS;
pub use remote::{note_wire_activity, set_wire_tx};

use crate::POWER_SERVICE;
use crate::hardware::SystemHardware;
use crate::orchestrator::record_proc_activation;
use crate::services::RadioMelodyExt;
use crate::services::power::PowerService;
use kernel::event::Event;
use py32_hal::adc::Adc;

#[inline(never)]
fn wake_and_notify_activity(hw: &mut SystemHardware) {
    hw.backlight.wake();
    unsafe {
        (*core::ptr::addr_of_mut!(POWER_SERVICE)).notify_activity();
    }
    hw.orchestrator.notify_activity();
}

#[allow(unused_variables)]
#[inline(never)]
pub(crate) fn poll_usb_and_wire(hw: &mut SystemHardware, now: u32, dirty_render: &mut bool) {
    #[cfg(feature = "usb")]
    {
        #[cfg(feature = "radio")]
        let tx_active = crate::radio_state_ref().coord.is_transmitting;
        #[cfg(not(feature = "radio"))]
        let tx_active = false;
        hw.usb.set_rf_tx_active(tx_active);

        if let Some(usb_ev) = hw.usb.poll() {
            *dirty_render = true;
            let new_clk = match usb_ev {
                kernel::event::UsbEvent::Connected | kernel::event::UsbEvent::Resumed => {
                    wake_and_notify_activity(hw);
                    // Index 3 = 48 MHz (HSI 16 MHz * PLL 3X, CTC SOF-locked).
                    (py32_hal::rcc::sysclk_hz() != 48_000_000).then_some(3)
                }
                kernel::event::UsbEvent::Disconnected if !tx_active => {
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
    }

    #[cfg(all(feature = "wire", any(feature = "uart", feature = "usb_cdc")))]
    for _ in 0..4 {
        let Some((header, _raw_len)) = hw.wire.poll(now) else {
            break;
        };
        note_wire_activity(now);
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
        if is_charging && !is_headless() {
            wake_and_notify_activity(hw);
        }
        *dirty_render = true;
        enqueue_event(Event::Charging(is_charging));
    }

    if pwr.poll_low_battery_transition() && pwr.uptime_secs() >= 3 && pwr.battery_percentage() <= 15
    {
        if !is_headless() {
            wake_and_notify_activity(hw);
            crate::orchestrator::compositor::show_low_battery_popup();
        }
        hw.radio
            .play_dialog_sound(crate::services::melody::DialogSoundType::Warning);
        *dirty_render = true;
    }

    if pwr.is_critical() && !pwr.is_charging() {
        power::handle_critical_battery_cutoff(hw, adc);
        *dirty_render = true;
    }
}

#[inline(always)]
fn should_keep_awake_on_keypad(
    admit: bool,
    keypad_locked: bool,
    had_event: bool,
    any_key_down: bool,
) -> bool {
    admit && !keypad_locked && (had_event || any_key_down)
}

#[inline(never)]
fn poll_keypad_tick(hw: &mut SystemHardware, dt: u16) {
    record_proc_activation(0xF3);
    hw.key_scanner.scan_with_dt(dt);
    let admit = admits_physical_input();
    let release_guard = admit && headless::waiting_escape_release(hw.key_scanner.is_any_key_down());
    let mut had_event = false;
    while let Some((code, state)) = hw.key_scanner.pop_event() {
        // Headless: the matrix keeps being scanned (debounce state stays coherent) but no
        // physical key ever reaches the system; RigWire is the sole source of input.
        if !admit || release_guard {
            continue;
        }
        had_event = true;
        enqueue_event(Event::Key { code, state });
    }
    if !admit {
        let escape_held = hw.key_scanner.is_key_down(kernel::event::KeyCode::Menu)
            && hw.key_scanner.is_key_down(kernel::event::KeyCode::Exit);
        if headless::poll_physical_escape(dt, escape_held) {
            set_headless(hw, false);
            hw.radio
                .play_dialog_sound(crate::services::melody::DialogSoundType::Unlock);
            unsafe {
                (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                    .show_toast("Headless mode off", 1800);
            }
            wake_and_notify_activity(hw);
        }
        return;
    }
    if release_guard {
        return;
    }
    // Keep backlight and idle/auto-lock timers awake on every key transition AND
    // continuously while any key remains held (long press or key-repeat scrolling).
    if should_keep_awake_on_keypad(
        admit,
        hw.orchestrator.is_keypad_locked(),
        had_event,
        hw.key_scanner.is_any_key_down(),
    ) {
        wake_and_notify_activity(hw);
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
        power::handle_deep_sleep_mode(hw);
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
    let mut dirty_render = false;
    let mut dimmed_dirty = false;
    let mut crash_loop_cleared = false;
    let mut low_batt_warning_timer = 0u8;

    // Complete initial OS frame before entering the event loop so LCD never stays blank.
    hw.render_frame();
    if !is_headless() {
        hw.lcd.turn_on();
        hw.backlight.wake();
    }
    #[cfg(feature = "grayscale")]
    let mut last_subframe_tick = py32_hal::delay::millis();

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
            gfx::gui::repeater::set_tick_dt_ms(delta as u16);
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
            power::handle_one_second_tick(hw, &mut low_batt_warning_timer);
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

        // USB IRQ (IRQ 31) wakes `wfi()` immediately on SOF/EP traffic; only skip `wfi()`
        // when EP0/CDC work is actively pending so idle-connected docks don't busy-spin at 48 MHz.
        #[cfg(feature = "usb")]
        if hw.usb.has_pending_transfer() {
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
    fn keypad_hold_and_repeat_keep_backlight_and_idle_awake() {
        // Active press or release wakes backlight and resets idle timers.
        assert!(should_keep_awake_on_keypad(true, false, true, false));
        // Continuous hold (no new edge event, key still down) keeps backlight/idle awake.
        assert!(should_keep_awake_on_keypad(true, false, false, true));
        // Idle with no key down does not wake.
        assert!(!should_keep_awake_on_keypad(true, false, false, false));
        // Locked keypad and headless mode never wake the panel on physical keys.
        assert!(!should_keep_awake_on_keypad(true, true, true, true));
        assert!(!should_keep_awake_on_keypad(false, false, true, true));
    }
}
