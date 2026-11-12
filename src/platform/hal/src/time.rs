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
    update_tim2_freq(rcc::sysclk_hz());

    unsafe {
        // Auto-reload at maximum (free-running)
        tim::TIM2_ARR.write_volatile(0xFFFF_FFFF);
        // Force an update event to load the prescaler immediately
        tim::TIM2_EGR.write_volatile(tim::EGR_UG);
        // Enable the counter
        let cr1 = tim::TIM2_CR1.read_volatile();
        tim::TIM2_CR1.write_volatile(cr1 | tim::CR1_CEN);
    }
}

/// Dynamically update TIM2 prescaler to ensure 1 µs tick precision at any system clock frequency.
pub fn update_tim2_freq(sysclk_hz: u32) {
    // Fast reciprocal division: sysclk_hz / 1_000_000: ((x * 0x10C6F7A1) >> 48)
    let div1m = ((sysclk_hz as u64 * 0x10C6F7A1) >> 48) as u32;
    let psc = div1m.saturating_sub(1);
    unsafe {
        tim::TIM2_PSC.write_volatile(psc);
        tim::TIM2_EGR.write_volatile(tim::EGR_UG);
    }
}

/// Read the current TIM2 counter value in microseconds.
#[inline(always)]
pub fn now_us() -> u32 {
    unsafe { tim::TIM2_CNT.read_volatile() }
}

/// Spin-wait for exactly `us` microseconds using the SysTick countdown timer.
#[inline]
pub fn spin_us(us: u32) {
    crate::delay::delay_us(us);
}
