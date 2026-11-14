//! Power Management Driver for PY32F071 (`PWR_CR1` / `PWR_CR2` / `PWR_SR`).

use pac::pwr::{PWR_CR1, PWR_CR1_DBP, PWR_CR1_LPR, PWR_SR, PWR_SR_PVDO};

#[inline(always)]
pub fn enable_pwr() {
    crate::rcc::enable_pwr();
}

/// Enable or disable write access to the RTC/Backup domain (`PWR_CR1.DBP`).
#[inline]
pub fn set_backup_write_enabled(enable: bool) {
    enable_pwr();
    unsafe {
        let cr1 = PWR_CR1.read_volatile();
        if enable {
            PWR_CR1.write_volatile(cr1 | PWR_CR1_DBP);
        } else {
            PWR_CR1.write_volatile(cr1 & !PWR_CR1_DBP);
        }
    }
}

pub fn enter_sleep() {
    cortex_m::asm::wfi();
}

/// Enter PY32F071 deep STOP mode with low-power regulator (`LPR = 1`) and `SysTick` suspended.
/// Caller must have configured wake interrupts (`EXTI`, `LPTIM1`, `RTC`) beforehand
/// and is responsible for restoring system clock/PLL afterward if desired.
pub fn enter_stop() {
    enable_pwr();
    unsafe {
        // 1. Enable low-power regulator during Stop (LPR = 1).
        let cr1 = PWR_CR1.read_volatile();
        PWR_CR1.write_volatile(cr1 | PWR_CR1_LPR);

        // 2. Suspend SysTick interrupt (TICKINT bit 1) so 1 kHz SysTick won't fight wakeups.
        let syst_csr = pac::SYST_CSR.read_volatile();
        pac::SYST_CSR.write_volatile(syst_csr & !(1 << 1));

        // 3. Arm Cortex-M0+ SLEEPDEEP before WFI.
        let scb = &*cortex_m::peripheral::SCB::PTR;
        scb.scr.modify(|r| r | (1 << 2));

        cortex_m::asm::dsb();
        cortex_m::asm::wfi();

        // 4. Disarm SLEEPDEEP and restore SysTick TICKINT + normal regulator.
        scb.scr.modify(|r| r & !(1 << 2));
        pac::SYST_CSR.write_volatile(syst_csr);
        PWR_CR1.write_volatile(cr1 & !PWR_CR1_LPR);
    }
}

pub fn pvd_below_threshold() -> bool {
    unsafe { (PWR_SR.read_volatile() & PWR_SR_PVDO) != 0 }
}
