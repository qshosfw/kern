//! Microkernel Power Management, Reset Lifecycle & Bootloader Handshake Contracts

use core::sync::atomic::{AtomicPtr, Ordering};

/// Target transition for a structured microkernel reboot request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RebootMode {
    /// Standard MCU reset into application firmware (`0x0800_2800`).
    Firmware = 0,
    /// Transition via board/firmware handler into factory bootloader (`0x0800_0000`) DFU mode.
    Bootloader = 1,
    /// Reboot into firmware safe-mode recovery UI.
    SafeRecovery = 2,
}

static REBOOT_HANDLER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Kernel power & reset lifecycle controller.
pub struct PowerControl;

impl PowerControl {
    /// Registers the board/firmware reboot handler invoked by [`Self::reboot_with`].
    ///
    /// On PY32F071 (Quansheng UV-K1 / UV-K5 v3), every reset vector enters the 10 KB
    /// factory bootloader (`0x0800_0000`), which zeroes SRAM and enters `0x0518` serial
    /// DFU only when hardware `PB10` (`PTT`) is sampled LOW across reset.
    pub fn set_reboot_handler(handler: fn(RebootMode) -> !) {
        REBOOT_HANDLER.store(handler as *mut (), Ordering::Release);
    }

    /// Reboots according to the requested [`RebootMode`], delegating to the registered
    /// board/firmware handler when present before triggering `AIRCR` `SYSRESETREQ`.
    #[inline(never)]
    pub fn reboot_with(mode: RebootMode) -> ! {
        let ptr = REBOOT_HANDLER.load(Ordering::Acquire);
        if !ptr.is_null() {
            let handler: fn(RebootMode) -> ! = unsafe { core::mem::transmute(ptr) };
            handler(mode);
        }
        Self::reboot_raw()
    }

    /// Standard reboot into application firmware (`RebootMode::Firmware`).
    #[inline(always)]
    pub fn reboot() -> ! {
        Self::reboot_with(RebootMode::Firmware)
    }

    /// Reboots into Bootloader DFU Hold mode (`RebootMode::Bootloader`).
    #[inline(always)]
    pub fn reboot_to_bootloader() -> ! {
        Self::reboot_with(RebootMode::Bootloader)
    }

    /// Raw Cortex-M0+ SCB `AIRCR` system reset (`SYSRESETREQ`) without running high-level hooks.
    pub fn reboot_raw() -> ! {
        unsafe {
            let aircr = 0xE000ED0C as *mut u32;
            core::ptr::write_volatile(aircr, (0x5FA << 16) | (1 << 2));
            loop {
                core::arch::asm!("nop");
            }
        }
    }
}

/// Operating System Power States
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// Active: Full LCD backlight and maximum responsiveness.
    Active,
    /// Standby: Backlight off, UI throttled, SPI blits suppressed.
    Standby,
    /// DeepSleep: Subsystems powered off, core downclocked to 24 MHz HSI, periodic strobe heartbeat.
    DeepSleep,
}

/// Power Management Profiles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    Balanced = 0,
    Performance = 1,
    PowerSave = 2,
    DeepSleep = 3,
}

impl PowerProfile {
    pub const fn from_u8(val: u8) -> Self {
        match val {
            0 => PowerProfile::Balanced,
            1 => PowerProfile::Performance,
            2 => PowerProfile::PowerSave,
            3 => PowerProfile::DeepSleep,
            _ => PowerProfile::Balanced,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_profile_conversion() {
        assert_eq!(PowerProfile::from_u8(0), PowerProfile::Balanced);
        assert_eq!(PowerProfile::from_u8(1), PowerProfile::Performance);
        assert_eq!(PowerProfile::from_u8(2), PowerProfile::PowerSave);
        assert_eq!(PowerProfile::from_u8(3), PowerProfile::DeepSleep);
        assert_eq!(PowerProfile::from_u8(99), PowerProfile::Balanced);
    }

    #[test]
    fn test_power_state_equality() {
        assert_eq!(PowerState::Active, PowerState::Active);
        assert_ne!(PowerState::Active, PowerState::Standby);
        assert_ne!(PowerState::Standby, PowerState::DeepSleep);
    }

    #[test]
    fn test_reboot_mode_and_handler_registration() {
        assert_eq!(RebootMode::Firmware as u8, 0);
        assert_eq!(RebootMode::Bootloader as u8, 1);
        assert_eq!(RebootMode::SafeRecovery as u8, 2);

        fn mock_reboot(mode: RebootMode) -> ! {
            panic!("reboot_mode={}", mode as u8);
        }
        PowerControl::set_reboot_handler(mock_reboot);
        assert!(!REBOOT_HANDLER.load(Ordering::Acquire).is_null());
        REBOOT_HANDLER.store(core::ptr::null_mut(), Ordering::Release);
    }
}
