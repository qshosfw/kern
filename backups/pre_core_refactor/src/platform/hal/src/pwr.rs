//! Power Management Driver

use pac::pwr::{PWR_CR, PWR_CR_CWUF, PWR_CR_PDDS, PWR_SR};

pub fn enter_sleep() {
    // Wait For Interrupt
    cortex_m::asm::wfi();
}

pub fn enter_stop() {
    unsafe {
        // Configure for stop mode: clear PDDS (Power Down Deep Sleep)
        // PDDS = 0 means enter Stop mode when Cortex-M0+ enters deepsleep.
        let val = PWR_CR.read_volatile();
        PWR_CR.write_volatile(val & !PWR_CR_PDDS);

        // Set SLEEPDEEP bit in Cortex-M0+ System Control Register (SCR)
        let scb = &*cortex_m::peripheral::SCB::PTR;
        scb.scr.modify(|r| r | (1 << 2)); // SLEEPDEEP is bit 2
    }

    // Execute WFI
    cortex_m::asm::wfi();

    unsafe {
        // Clear SLEEPDEEP bit after waking up
        let scb = &*cortex_m::peripheral::SCB::PTR;
        scb.scr.modify(|r| r & !(1 << 2));
    }
}

pub fn enter_standby() {
    unsafe {
        // Clear Wakeup Flag
        let val = PWR_CR.read_volatile();
        PWR_CR.write_volatile(val | PWR_CR_CWUF);

        // Configure for standby mode: set PDDS (Power Down Deep Sleep)
        let val2 = PWR_CR.read_volatile();
        PWR_CR.write_volatile(val2 | PWR_CR_PDDS);

        // Set SLEEPDEEP bit in Cortex-M0+ System Control Register (SCR)
        let scb = &*cortex_m::peripheral::SCB::PTR;
        scb.scr.modify(|r| r | (1 << 2));
    }

    // Execute WFI
    cortex_m::asm::wfi();
}

pub fn is_wakeup_flag_set() -> bool {
    unsafe {
        // Read WUF (Wakeup Flag) which is typically bit 0 in SR
        (PWR_SR.read_volatile() & 1) != 0
    }
}

pub fn clear_wakeup_flag() {
    unsafe {
        let val = PWR_CR.read_volatile();
        PWR_CR.write_volatile(val | PWR_CR_CWUF);
    }
}
