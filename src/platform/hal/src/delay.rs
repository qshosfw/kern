//! SysTick-based blocking delays for PY32F071.
//!
//! Uses the ARM Cortex-M0+ SysTick timer configured for 1 kHz (1 ms) ticks.

use portable_atomic::{AtomicU32, Ordering};

/// Global millisecond counter, incremented every SysTick tick (1ms).
static MILLIS: AtomicU32 = AtomicU32::new(0);

/// SysTick interrupt frequency in Hz (1000 Hz = 1 ms interval).
pub const SYSTICK_HZ: u32 = 1_000;

/// Ticks per microsecond (48 at 48 MHz, 72 at 72 MHz).
static TICKS_PER_US: AtomicU32 = AtomicU32::new(48);

/// Initialize SysTick for 1 ms (1 kHz) interrupts.
pub fn init() {
    update_systick_freq(crate::rcc::sysclk_hz());
}

/// Update SysTick reload register and tick scaling for a new system clock frequency.
pub fn update_systick_freq(sysclk_hz: u32) {
    // Fast reciprocal division:
    // sysclk_hz / 1000: ((x * 0x4189374C) >> 40)
    // sysclk_hz / 1_000_000: (((x >> 12) * 17_180) >> 22) in 32-bit math
    let div1k = ((sysclk_hz as u64 * 0x4189374C) >> 40) as u32;
    let div1m = ((sysclk_hz >> 12) * 17_180) >> 22;
    let reload = div1k.saturating_sub(1);
    let ticks_us = div1m.max(1);
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

/// Called by the SysTick exception handler at 1 kHz to increment the ms counter every 1ms.
#[inline(always)]
pub fn on_systick() {
    MILLIS.fetch_add(1, Ordering::Relaxed);
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

    if !cortex_m::register::primask::read().is_inactive() {
        let mut rem = ms;
        while rem > 0 {
            let chunk = rem.min(50);
            delay_us(chunk * 1000);
            rem -= chunk;
            unsafe {
                pac::iwdg::IWDG_KR.write_volatile(pac::iwdg::KR_KEY_RELOAD);
            }
        }
        return;
    }

    let start = millis();
    while millis().wrapping_sub(start) < ms {
        cortex_m::asm::nop();
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
            if elapsed > 0 {
                remaining -= elapsed;
                prev = curr;
            } else {
                // Guarantee forward progress if counter hasn't advanced between back-to-back reads
                remaining = remaining.saturating_sub(4);
            }
        }
    }
}
