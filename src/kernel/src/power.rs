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

impl RebootMode {
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Bootloader,
            2 => Self::SafeRecovery,
            _ => Self::Firmware,
        }
    }
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

/// Bitmask of subsystems pinning the MCU clock floor.
pub mod clock_reason {
    pub const USB_ACTIVE: u8 = 1 << 0;
    pub const WIRE_LINK: u8 = 1 << 1;
    pub const AUDIO_DSP: u8 = 1 << 2;
    pub const SPECTRUM: u8 = 1 << 3;
    pub const USER_BOOST: u8 = 1 << 4;
}

/// Dynamic MCU clock governor (16 / 24 / 32 / 48 / 72 MHz) with hardware-safe floor enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockGovernor {
    pub preferred_mhz: u8,
    pub reasons: u8,
    pub allow_overclock: bool,
}

impl ClockGovernor {
    pub const fn new(preferred_mhz: u8, allow_overclock: bool) -> Self {
        Self {
            preferred_mhz,
            reasons: 0,
            allow_overclock,
        }
    }

    #[inline(always)]
    pub fn acquire(&mut self, reason: u8) {
        self.reasons |= reason;
    }

    #[inline(always)]
    pub fn release(&mut self, reason: u8) {
        self.reasons &= !reason;
    }

    /// Resolve effective MCU clock in MHz respecting USB exact-48 MHz PLL lock and boost hints.
    pub const fn target_mhz(&self) -> u8 {
        // PY32F071 USBD Full-Speed requires exact 48 MHz (HSI 16 MHz * PLL 3X, CTC SOF-locked).
        if (self.reasons & clock_reason::USB_ACTIVE) != 0 {
            return 48;
        }
        let mut target = self.preferred_mhz;
        if (self.reasons & clock_reason::WIRE_LINK) != 0 && target < 48 {
            target = 48;
        }
        if (self.reasons & (clock_reason::SPECTRUM | clock_reason::USER_BOOST)) != 0
            && self.allow_overclock
        {
            target = 72;
        }
        if !self.allow_overclock && target > 48 {
            target = 48;
        }
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_governor_enforces_usb_exact_48mhz_and_overclock_gate() {
        let mut gov = ClockGovernor::new(16, false);
        assert_eq!(gov.target_mhz(), 16);
        gov.acquire(clock_reason::USB_ACTIVE);
        assert_eq!(gov.target_mhz(), 48);
        gov.acquire(clock_reason::SPECTRUM);
        assert_eq!(gov.target_mhz(), 48); // overclock disabled
        gov.allow_overclock = true;
        // USB_ACTIVE must clamp ceiling to exact 48 MHz even when overclock is enabled
        assert_eq!(gov.target_mhz(), 48);
        gov.release(clock_reason::USB_ACTIVE);
        assert_eq!(gov.target_mhz(), 72);
        gov.release(clock_reason::SPECTRUM);
        assert_eq!(gov.target_mhz(), 16);
    }

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
