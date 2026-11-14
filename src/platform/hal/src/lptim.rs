//! Low-Power Timer (LPTIM1) for PY32F071.
//!
//! Operational in STOP mode when clocked from 32.768 kHz LSI (`RCC_CCIPR.LPTIM1SEL = 01`).
//! Prescaler `/32` yields `~1024 Hz` ticks (e.g. `4096` ticks = ~4.0 s wakeup cadence).

use pac::lptim::*;

pub struct LowPowerTimer;

impl LowPowerTimer {
    /// Initializes LPTIM1 with LSI 32.768 kHz clock source and `/32` prescaler (`~1024 Hz`).
    pub fn init() {
        crate::rcc::enable_lptim1_lsi();
        unsafe {
            LPTIM_CR.write_volatile(0);
            LPTIM_CFGR.write_volatile(5 << 9); // PRESC = /32
            LPTIM_ICR.write_volatile(ICR_ARRMCF);
            LPTIM_CR.write_volatile(CR_ENABLE);
        }
    }

    /// Starts a single-shot wakeup timer for `ticks` (~1 ms/tick) and optionally enables ARRM IRQ.
    pub fn start_single_shot(ticks: u16) {
        Self::start_single_shot_irq(ticks, false);
    }

    /// Starts a single-shot wakeup timer with optional ARRM interrupt generation (for STOP wakeups).
    pub fn start_single_shot_irq(ticks: u16, enable_irq: bool) {
        crate::rcc::enable_lptim1_lsi();
        unsafe {
            LPTIM_CR.write_volatile(CR_ENABLE);
            LPTIM_ICR.write_volatile(ICR_ARRMCF);
            LPTIM_IER.write_volatile(if enable_irq { IER_ARRMIE } else { 0 });
            LPTIM_ARR.write_volatile(ticks.max(1) as u32);
            LPTIM_CR.write_volatile(CR_ENABLE | CR_SNGSTRT);
        }
    }

    /// Checks if the timer has expired (ARR match).
    #[inline(always)]
    pub fn is_expired() -> bool {
        unsafe { (LPTIM_ISR.read_volatile() & ISR_ARRM) != 0 }
    }

    /// Clears the timer expired flag.
    #[inline(always)]
    pub fn clear_expired() {
        unsafe {
            LPTIM_ICR.write_volatile(ICR_ARRMCF);
        }
    }

    /// Reads the current 16-bit counter value.
    #[inline(always)]
    pub fn read_counter() -> u16 {
        unsafe { (LPTIM_CNT.read_volatile() & 0xFFFF) as u16 }
    }

    /// Stops LPTIM1 cleanly and clears pending interrupt.
    pub fn stop() {
        unsafe {
            LPTIM_IER.write_volatile(0);
            LPTIM_ICR.write_volatile(ICR_ARRMCF);
            LPTIM_CR.write_volatile(0);
        }
    }
}
