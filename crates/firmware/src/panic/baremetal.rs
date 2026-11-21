#![cfg_attr(test, allow(dead_code))]
//! Bare-Metal Panic Recovery Loop.
//!
//! Heavy crash dialogs, Safe Mode recovery menus, 5-page forensics, Thumb-16 disassembly,
//! and serial UART dumps are offloaded to `/apps/panic.mod`. Firmware retains only the
//! hardware-safe watchdog-fed reset delay after quenching RF/DMA.

#[cfg(not(test))]
use py32_hal as hal;

#[cfg(not(test))]
pub fn recovery_loop() -> ! {
    bsp::set_status_led(bsp::StatusLed::Red);
    for _ in 0..150 {
        hal::wdt::Watchdog::feed();
        hal::delay::delay_ms(20);
    }
    cortex_m::peripheral::SCB::sys_reset();
}
