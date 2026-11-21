//! # PY32F071 Peripheral Access Crate
//!
//! Minimal, hand-rolled register definitions for the PY32F071xB MCU.
//! Only peripherals actively used by kern are defined here.
//!
//! All addresses are sourced from the official Puya CMSIS header:
//! `py32f071xB.h` (see `inspo/fagci-k1-fw/src/external/CMSIS/`)
//!
//! ## Memory Map (PY32F071xB)
//!
//! | Region       | Base Address   | Size  |
//! |-------------|----------------|-------|
//! | Flash       | `0x0800_0000`  | 128K  |
//! | SRAM        | `0x2000_0000`  | 16K   |
//! | APB1        | `0x4000_0000`  |       |
//! | APB2        | `0x4001_0000`  |       |
//! | AHB1        | `0x4002_0000`  |       |
//! | IOPORT      | `0x5000_0000`  |       |
//! | Cortex-M0+  | `0xE000_0000`  |       |

#![no_std]

pub mod interrupt;
pub use interrupt::{Interrupt, NVIC_PRIO_BITS};

pub mod adc;
pub mod comp;
pub mod crc;
pub mod dac;
pub mod dma;
pub mod exti;
pub mod flash;
pub mod gpio;
pub mod i2c;
pub mod iwdg;
pub mod lptim;
pub mod pwr;
pub mod rcc;
pub mod rtc;
pub mod spi;
pub mod syscfg;
pub mod tim;
pub mod usart;
pub mod usbd;
pub mod wwdg;

// ── Base Addresses ──────────────────────────────────────────────────────────

/// APB1 peripheral base
pub const APB1_BASE: u32 = 0x4000_0000;

/// APB2 peripheral base
pub const APB2_BASE: u32 = 0x4001_0000;

/// AHB1 peripheral base
pub const AHB1_BASE: u32 = 0x4002_0000;

/// IOPORT peripheral base (GPIO lives here, not on AHB like STM32)
pub const IOPORT_BASE: u32 = 0x5000_0000;

// ── ARM Cortex-M0+ System Registers ─────────────────────────────────────────

/// SysTick Control and Status Register
pub const SYST_CSR: *mut u32 = 0xE000_E010 as *mut u32;
/// SysTick Reload Value Register
pub const SYST_RVR: *mut u32 = 0xE000_E014 as *mut u32;
/// SysTick Current Value Register
pub const SYST_CVR: *mut u32 = 0xE000_E018 as *mut u32;

/// SCB Interrupt Control and State Register
pub const SCB_ICSR: *const u32 = 0xE000_ED04 as *const u32;
/// SCB Vector Table Offset Register
pub const SCB_VTOR: *mut u32 = 0xE000_ED08 as *mut u32;
/// CoreDebug Halting Control and Status Register
pub const COREDEBUG_DHCSR: *const u32 = 0xE000_EDF0 as *const u32;
/// CoreDebug Halting Debug Enable bit (`C_DEBUGEN`)
pub const COREDEBUG_C_DEBUGEN: u32 = 1 << 0;

/// NVIC Interrupt Set-Enable Register
pub const NVIC_ISER: *mut u32 = 0xE000_E100 as *mut u32;
/// NVIC Interrupt Clear-Enable Register
pub const NVIC_ICER: *mut u32 = 0xE000_E180 as *mut u32;
/// NVIC Interrupt Clear-Pending Register
pub const NVIC_ICPR: *mut u32 = 0xE000_E280 as *mut u32;

/// Enable an NVIC interrupt line (`0..32` on ARMv6-M Cortex-M0+).
#[inline(always)]
pub unsafe fn nvic_enable(irq_num: u8) {
    if irq_num < 32 {
        unsafe {
            NVIC_ISER.write_volatile(1u32 << irq_num);
        }
    }
}

/// Disable an NVIC interrupt line (`0..32` on ARMv6-M Cortex-M0+).
#[inline(always)]
pub unsafe fn nvic_disable(irq_num: u8) {
    if irq_num < 32 {
        unsafe {
            NVIC_ICER.write_volatile(1u32 << irq_num);
        }
    }
}

/// Clear a pending NVIC interrupt flag (`0..32` on ARMv6-M Cortex-M0+).
#[inline(always)]
pub unsafe fn nvic_clear_pending(irq_num: u8) {
    if irq_num < 32 {
        unsafe {
            NVIC_ICPR.write_volatile(1u32 << irq_num);
        }
    }
}
