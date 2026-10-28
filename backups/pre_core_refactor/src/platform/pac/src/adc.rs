//! ADC1 peripheral registers for PY32F071.
//!
//! ADC1 is on APB2. Used for battery voltage measurement on Channel 8 (PB0).

use crate::APB2_BASE;

pub const ADC1_BASE: u32 = APB2_BASE + 0x2400; // 0x4001_2400

// ── Register Addresses ──────────────────────────────────────────────────────

/// ADC status register
pub const ADC_SR: *mut u32 = ADC1_BASE as *mut u32;
/// ADC control register 1
pub const ADC_CR1: *mut u32 = (ADC1_BASE + 0x04) as *mut u32;
/// ADC control register 2
pub const ADC_CR2: *mut u32 = (ADC1_BASE + 0x08) as *mut u32;
/// ADC sample time register 1
pub const ADC_SMPR1: *mut u32 = (ADC1_BASE + 0x0C) as *mut u32;
/// ADC sample time register 2
pub const ADC_SMPR2: *mut u32 = (ADC1_BASE + 0x10) as *mut u32;
/// ADC sample time register 3
pub const ADC_SMPR3: *mut u32 = (ADC1_BASE + 0x14) as *mut u32;
/// ADC regular sequence register 1
pub const ADC_SQR1: *mut u32 = (ADC1_BASE + 0x30) as *mut u32;
/// ADC regular sequence register 2
pub const ADC_SQR2: *mut u32 = (ADC1_BASE + 0x34) as *mut u32;
/// ADC regular sequence register 3
pub const ADC_SQR3: *mut u32 = (ADC1_BASE + 0x38) as *mut u32;
/// ADC regular data register
pub const ADC_DR: *const u32 = (ADC1_BASE + 0x50) as *const u32;

// ── SR Bit Definitions ─────────────────────────────────────────────────────

/// End of conversion
pub const SR_EOC: u32 = 1 << 1;

// ── CR2 Bit Definitions ────────────────────────────────────────────────────

/// ADC enable
pub const CR2_ADON: u32 = 1 << 0;
/// Start conversion of regular channels
pub const CR2_SWSTART: u32 = 1 << 22;
/// External trigger enable for regular channels
pub const CR2_EXTTRIG: u32 = 1 << 20;
/// External trigger event selection: SWSTART (bits 19:17 = 0b111)
pub const CR2_EXTSEL_SWSTART: u32 = 7 << 17;
/// Calibration
pub const CR2_CAL: u32 = 1 << 2;
/// Temperature sensor and VREFINT enable
pub const CR2_TSVREFE: u32 = 1 << 23;
