//! Power Management Driver for PY32F071 (`PWR_CR1` / `PWR_CR2` / `PWR_SR`).

use pac::pwr::{
    PWR_CR1, PWR_CR1_DBP, PWR_CR1_FLS_SLPTIME_2US, PWR_CR1_FLS_SLPTIME_MASK, PWR_CR1_LPR, PWR_CR2,
    PWR_CR2_FLTEN, PWR_CR2_FLT_TIME_MASK, PWR_CR2_FLT_TIME_SHIFT, PWR_CR2_PVDE, PWR_CR2_PVDT_MASK,
    PWR_CR2_PVDT_SHIFT, PWR_CR2_SRCSEL, PWR_SR, PWR_SR_PVDO,
};

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

/// Enter PY32F071 deep STOP mode with low-power regulator (`LPR = 1`), Flash sleep delay,
/// ADC analog power-down, and `SysTick` suspended + pending bit cleared.
/// Caller must have configured wake interrupts (`EXTI`, `LPTIM1`, `RTC`) beforehand
/// and is responsible for restoring system clock/PLL afterward if desired.
pub fn enter_stop() {
    enable_pwr();
    // Shut down ADC analog bias (150-300 uA) before STOP; `Adc::read_channel()` wakes lazily.
    crate::adc::Adc::power_down();
    unsafe {
        // 1. Enable low-power regulator during Stop (LPR = 1) and 2us Flash wake delay.
        let cr1 = PWR_CR1.read_volatile();
        PWR_CR1.write_volatile(
            (cr1 & !PWR_CR1_FLS_SLPTIME_MASK) | PWR_CR1_LPR | PWR_CR1_FLS_SLPTIME_2US,
        );

        // 2. Suspend SysTick interrupt (TICKINT bit 1) and clear any already-pending SysTick
        //    so a stale 1 ms tick cannot abort STOP immediately on `wfi()`.
        let syst_csr = pac::SYST_CSR.read_volatile();
        pac::SYST_CSR.write_volatile(syst_csr & !(1 << 1));
        (0xE000_ED04 as *mut u32).write_volatile(1 << 25); // SCB_ICSR.PENDSTCLR

        // 3. Arm Cortex-M0+ SLEEPDEEP before WFI.
        let scb = &*cortex_m::peripheral::SCB::PTR;
        scb.scr.modify(|r| r | (1 << 2));

        cortex_m::asm::dsb();
        cortex_m::asm::wfi();
        cortex_m::asm::isb();

        // 4. Disarm SLEEPDEEP and restore SysTick TICKINT + normal regulator.
        scb.scr.modify(|r| r & !(1 << 2));
        pac::SYST_CSR.write_volatile(syst_csr);
        PWR_CR1.write_volatile(cr1 & !PWR_CR1_LPR);
    }
}

/// Arm hardware Programmable Voltage Detector (`PVD`) on `VDD` with optional digital glitch filter
/// and rising-edge `EXTI16` interrupt (fires when `VDD` drops below the selected threshold).
///
/// PY32F071 `PVDT[2:0]` levels on `VDD`: `0`=2.0V, `1`=2.2V, `2`=2.4V, `3`=2.6V,
/// `4`=2.8V, `5`=3.0V, `6`=3.2V (typical falling thresholds).
pub fn enable_pvd(level: u8, filter_enable: bool) {
    enable_pwr();
    unsafe {
        let mut cr2 = PWR_CR2.read_volatile()
            & !(PWR_CR2_PVDE
                | PWR_CR2_SRCSEL
                | PWR_CR2_PVDT_MASK
                | PWR_CR2_FLTEN
                | PWR_CR2_FLT_TIME_MASK);
        cr2 |= ((level as u32 & 0b111) << PWR_CR2_PVDT_SHIFT) | PWR_CR2_PVDE;
        if filter_enable {
            // Digital glitch filter enabled with moderate debounce window (`FLT_TIME = 0b010`).
            cr2 |= PWR_CR2_FLTEN | (0b010 << PWR_CR2_FLT_TIME_SHIFT);
        }
        PWR_CR2.write_volatile(cr2);
    }
    // EXTI line 16 rising edge fires when PVDO transitions 0 -> 1 (VDD fell below threshold).
    crate::exti::enable_line(crate::exti::LINE_PVD, crate::exti::TriggerEdge::Rising);
    crate::exti::clear_pending(crate::exti::LINE_PVD);
}

/// Disable hardware PVD and mask `EXTI16`.
pub fn disable_pvd() {
    crate::exti::disable_line(crate::exti::LINE_PVD);
    crate::exti::clear_pending(crate::exti::LINE_PVD);
    enable_pwr();
    unsafe {
        let cr2 = PWR_CR2.read_volatile();
        PWR_CR2.write_volatile(cr2 & !PWR_CR2_PVDE);
    }
}

pub fn pvd_below_threshold() -> bool {
    unsafe { (PWR_SR.read_volatile() & PWR_SR_PVDO) != 0 }
}
