//! TIM2 free-running 32-bit microsecond counter.
//!
//! Provides high-resolution timestamps for performance measurement
//! and precise delay loops in drivers (BK4819 SPI bit-bang timing).
//!
//! TIM2 is configured as a free-running up-counter clocked at 48 MHz
//! with prescaler=47, giving exactly 1 tick per microsecond.

use crate::rcc;
use pac::tim;

/// Initialize TIM2 as a free-running microsecond counter.
///
/// After calling this, [`now_us`] returns the elapsed microseconds
/// since initialization (wraps at ~71.6 minutes for u32).
pub fn init() {
    rcc::enable_tim2();

    unsafe {
        // Prescaler: 48MHz / (47+1) = 1 MHz → 1 tick = 1 µs
        tim::TIM2_PSC.write_volatile(47);
        // Auto-reload at maximum (free-running)
        tim::TIM2_ARR.write_volatile(0xFFFF_FFFF);
        // Force an update event to load the prescaler immediately
        tim::TIM2_EGR.write_volatile(tim::EGR_UG);
        // Enable the counter
        let cr1 = tim::TIM2_CR1.read_volatile();
        tim::TIM2_CR1.write_volatile(cr1 | tim::CR1_CEN);
    }
}

/// Read the current TIM2 counter value in microseconds.
#[inline(always)]
pub fn now_us() -> u32 {
    unsafe { tim::TIM2_CNT.read_volatile() }
}

/// Spin-wait for exactly `us` microseconds using the TIM2 counter.
///
/// More precise than [`crate::delay::delay_us`] for short delays
/// because it doesn't depend on SysTick reload math.
#[inline]
pub fn spin_us(us: u32) {
    if us == 0 {
        return;
    }
    let start = now_us();
    while now_us().wrapping_sub(start) < us {
        core::hint::spin_loop();
    }
}
