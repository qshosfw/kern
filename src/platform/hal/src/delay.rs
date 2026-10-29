//! SysTick-based blocking delays for PY32F071.
//!
//! Uses the ARM Cortex-M0+ SysTick timer configured for 10 kHz (100 µs) ticks.
//! This drives a 1000 Hz software PWM for LCD/keyboard backlight to eliminate
//! low-frequency beating against the ST7565 LCD panel multiplex rate (~70 Hz).
//!
//! A 10-count sub-millisecond accumulator maintains the global millisecond counter.

use portable_atomic::{AtomicU8, AtomicU32, Ordering};

/// Global millisecond counter, incremented every 10 SysTick ticks (1ms).
static MILLIS: AtomicU32 = AtomicU32::new(0);

/// Sub-millisecond tick accumulator (0..9 at 10 kHz).
static SUB_MS_COUNT: AtomicU8 = AtomicU8::new(0);

/// SysTick interrupt frequency in Hz (20 kHz = 50 µs interval).
pub const SYSTICK_HZ: u32 = 20_000;
pub const TICKS_PER_MS: u8 = 20;

/// Ticks per microsecond (48 at 48 MHz, 72 at 72 MHz).
static TICKS_PER_US: AtomicU32 = AtomicU32::new(48);

/// Initialize SysTick for 50 µs (20 kHz) interrupts.
pub fn init() {
    update_systick_freq(crate::rcc::sysclk_hz());
}

/// Update SysTick reload register and tick scaling for a new system clock frequency.
pub fn update_systick_freq(sysclk_hz: u32) {
    let reload = (sysclk_hz / SYSTICK_HZ).saturating_sub(1);
    let ticks_us = (sysclk_hz / 1_000_000).max(1);
    TICKS_PER_US.store(ticks_us, Ordering::Relaxed);

    unsafe {
        // Set reload value
        pac::SYST_RVR.write_volatile(reload);
        // Clear current value
        pac::SYST_CVR.write_volatile(0);
        // Enable SysTick with processor clock + interrupt
        // Bit 0: ENABLE, Bit 1: TICKINT, Bit 2: CLKSOURCE (processor clock)
        pac::SYST_CSR.write_volatile(0b111);
    }
}

/// Called by the SysTick exception handler at 10 kHz to increment the ms counter every 1ms.
#[inline(always)]
pub fn on_systick() {
    let count = SUB_MS_COUNT.load(Ordering::Relaxed);
    if count >= TICKS_PER_MS - 1 {
        SUB_MS_COUNT.store(0, Ordering::Relaxed);
        MILLIS.fetch_add(1, Ordering::Relaxed);
    } else {
        SUB_MS_COUNT.store(count + 1, Ordering::Relaxed);
    }
}

/// Get the current millisecond count since boot.
#[inline(always)]
pub fn millis() -> u32 {
    MILLIS.load(Ordering::Relaxed)
}

/// Get the current microsecond count since boot with hardware TIM2 resolution.
#[inline(always)]
pub fn micros() -> u32 {
    crate::time::now_us()
}

/// Blocking delay for `ms` milliseconds using hardware-synchronized SysTick.
///
/// If interrupts are disabled (e.g. inside a panic/fault handler or critical section),
/// falls back to register-polled countdown loops via `delay_us` so execution never deadlocks.
/// Automatically feeds the Independent Watchdog (IWDG) during long delays in <= 50ms chunks.
pub fn delay_ms(ms: u32) {
    if ms == 0 {
        return;
    }

    // If interrupts are disabled (PRIMASK bit is set) or executing inside an exception handler
    // (where SysTick cannot preempt), SysTick ISR cannot fire.
    // Use hardware register-polled countdown loops to avoid deadlock.
    let in_exception = cortex_m::peripheral::SCB::vect_active()
        != cortex_m::peripheral::scb::VectActive::ThreadMode;
    if cortex_m::register::primask::read().is_active() || in_exception {
        let mut rem = ms;
        while rem > 0 {
            let chunk = rem.min(25);
            for _ in 0..chunk {
                delay_us(1000);
            }
            rem -= chunk;
            unsafe {
                pac::iwdg::IWDG_KR.write_volatile(pac::iwdg::KR_KEY_RELOAD);
            }
        }
        return;
    }

    let mut rem = ms;
    while rem > 0 {
        let chunk = rem.min(50);
        let start = millis();
        while millis().wrapping_sub(start) < chunk {
            cortex_m::asm::nop();
        }
        rem -= chunk;
        unsafe {
            pac::iwdg::IWDG_KR.write_volatile(pac::iwdg::KR_KEY_RELOAD);
        }
    }
}

/// Blocking delay for `us` microseconds using SysTick countdown polling.
///
/// Precision: ±1 tick (~20.8 ns at 48 MHz). Does not use interrupts.
pub fn delay_us(us: u32) {
    if us == 0 {
        return;
    }

    let ticks_needed = us * TICKS_PER_US.load(Ordering::Relaxed);
    let mut remaining = ticks_needed;

    unsafe {
        let mut prev = pac::SYST_CVR.read_volatile() & 0x00FF_FFFF;

        while remaining > 0 {
            let curr = pac::SYST_CVR.read_volatile() & 0x00FF_FFFF;
            let elapsed = if curr <= prev {
                prev - curr
            } else {
                // Counter wrapped around
                prev + (pac::SYST_RVR.read_volatile() & 0x00FF_FFFF) + 1 - curr
            };

            if elapsed >= remaining {
                break;
            }
            remaining -= elapsed;
            prev = curr;
        }
    }
}
