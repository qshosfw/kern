//! Power management: Deep Sleep (STOP), hold-to-wake, critical battery cutoff and APO.

use super::remote::is_remote_control_active;
use super::{is_headless, wake_and_notify_activity};
use crate::POWER_SERVICE;
use crate::hardware::SystemHardware;
use crate::services::RadioMelodyExt;
use crate::services::power::PowerService;
use py32_hal::adc::Adc;

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
pub(super) fn handle_deep_sleep_mode(hw: &mut SystemHardware) {
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

    // Wake LCD first so SPI blit succeeds (`render_frame` skips the blit when headless)
    if !is_headless() {
        hw.lcd.exit_sleep();
    }

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
pub(super) fn handle_critical_battery_cutoff(hw: &mut SystemHardware, adc: &Adc) {
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
    if is_headless() {
        hw.lcd.enter_sleep();
    } else {
        hw.backlight.wake();
    }
}

#[inline(never)]
pub(super) fn handle_one_second_tick(hw: &mut SystemHardware, low_batt_warning_timer: &mut u8) {
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
    let now = py32_hal::delay::millis();
    let is_remote = is_remote_control_active(hw, now);
    super::remote::enforce_tx_failsafe(hw, now);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_hold_to_wake_transitions() {
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
