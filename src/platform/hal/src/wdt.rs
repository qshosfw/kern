//! Independent Watchdog (IWDG) & Reset Cause Introspection for PY32F071xB

use pac::iwdg::*;

/// Reset cause detected at boot time
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetCause {
    PowerOn,
    PinReset,
    SoftwareReset,
    IndependentWatchdog,
    WindowWatchdog,
    LowPower,
    Unknown,
}

/// Independent Watchdog driver for system fault protection
pub struct Watchdog;

impl Watchdog {
    /// Start the Independent Watchdog timer with specified timeout in milliseconds (e.g. 2000ms)
    pub fn start(timeout_ms: u32) {
        unsafe {
            // Enable IWDG
            IWDG_KR.write_volatile(KR_KEY_ENABLE);
            // Unlock registers
            IWDG_KR.write_volatile(KR_KEY_UNLOCK);

            // Prescaler 64 (~500Hz clock from LSI 32kHz)
            IWDG_PR.write_volatile(0x04);

            // Calculate reload count (timeout_ms * 32 / 64)
            let reload = (timeout_ms * 32 / 64).min(0xFFF);
            IWDG_RLR.write_volatile(reload);

            // Refresh counter
            IWDG_KR.write_volatile(KR_KEY_RELOAD);
        }
    }

    /// Feed / Kick the watchdog timer to prevent system reset
    #[inline(always)]
    pub fn feed() {
        unsafe {
            IWDG_KR.write_volatile(KR_KEY_RELOAD);
        }
    }

    /// Introspect the MCU reset cause from hardware flags
    pub fn reset_cause() -> ResetCause {
        // Source RCC_CSR at 0x4002_1024
        let csr_ptr = 0x4002_1024 as *const u32;
        let csr = unsafe { csr_ptr.read_volatile() };

        if (csr & (1 << 29)) != 0 {
            ResetCause::IndependentWatchdog
        } else if (csr & (1 << 30)) != 0 {
            ResetCause::WindowWatchdog
        } else if (csr & (1 << 28)) != 0 {
            ResetCause::SoftwareReset
        } else if (csr & (1 << 27)) != 0 {
            ResetCause::PowerOn
        } else if (csr & (1 << 26)) != 0 {
            ResetCause::PinReset
        } else if (csr & (1 << 31)) != 0 {
            ResetCause::LowPower
        } else {
            ResetCause::Unknown
        }
    }

    /// Clear the hardware reset flags in RCC_CSR (Bit 24 RMVF: Remove Reset Flag)
    pub fn clear_reset_flags() {
        let csr_ptr = 0x4002_1024 as *mut u32;
        unsafe {
            csr_ptr.write_volatile(csr_ptr.read_volatile() | (1 << 24));
        }
    }
}
