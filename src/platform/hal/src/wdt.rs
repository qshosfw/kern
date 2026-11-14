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

            // Calculate reload count (timeout_ms * 32 / 64 == timeout_ms >> 1)
            let reload = (timeout_ms >> 1).min(0xFFF);
            IWDG_RLR.write_volatile(reload);

            // Refresh counter
            IWDG_KR.write_volatile(KR_KEY_RELOAD);
        }
    }

    /// Feed / Kick the watchdog timer to prevent system reset
    #[inline(never)]
    pub fn feed() {
        unsafe {
            IWDG_KR.write_volatile(KR_KEY_RELOAD);
        }
    }

    /// Read raw `RCC_CSR` reset status bits for diagnostics / crash capture.
    #[inline(always)]
    pub fn raw_reset_flags() -> u32 {
        unsafe { pac::rcc::RCC_CSR.read_volatile() }
    }

    /// Introspect the MCU reset cause from `RCC_CSR` hardware flags.
    pub fn reset_cause() -> ResetCause {
        let csr = Self::raw_reset_flags();

        if (csr & pac::rcc::CSR_IWDGRSTF) != 0 {
            ResetCause::IndependentWatchdog
        } else if (csr & pac::rcc::CSR_WWDGRSTF) != 0 {
            ResetCause::WindowWatchdog
        } else if (csr & pac::rcc::CSR_SFTRSTF) != 0 {
            ResetCause::SoftwareReset
        } else if (csr & pac::rcc::CSR_PWRRSTF) != 0 {
            ResetCause::PowerOn
        } else if (csr & pac::rcc::CSR_PINRSTF) != 0 {
            ResetCause::PinReset
        } else if (csr & pac::rcc::CSR_OBLRSTF) != 0 {
            ResetCause::LowPower
        } else {
            ResetCause::Unknown
        }
    }

    /// Clear the hardware reset flags in `RCC_CSR` (Bit 23 `RMVF`: Remove Reset Flag).
    pub fn clear_reset_flags() {
        unsafe {
            let csr = pac::rcc::RCC_CSR.read_volatile();
            pac::rcc::RCC_CSR.write_volatile(csr | pac::rcc::CSR_RMVF);
        }
    }
}
