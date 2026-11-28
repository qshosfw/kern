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
/// ADC watchdog higher threshold register
pub const ADC_HTR: *mut u32 = (ADC1_BASE + 0x28) as *mut u32;
/// ADC watchdog lower threshold register
pub const ADC_LTR: *mut u32 = (ADC1_BASE + 0x2C) as *mut u32;
/// ADC regular sequence register 1
pub const ADC_SQR1: *mut u32 = (ADC1_BASE + 0x30) as *mut u32;
/// ADC regular sequence register 2
pub const ADC_SQR2: *mut u32 = (ADC1_BASE + 0x34) as *mut u32;
/// ADC regular sequence register 3
pub const ADC_SQR3: *mut u32 = (ADC1_BASE + 0x38) as *mut u32;
/// ADC injected sequence register
pub const ADC_JSQR: *mut u32 = (ADC1_BASE + 0x3C) as *mut u32;
/// ADC injected data register 1
pub const ADC_JDR1: *const u32 = (ADC1_BASE + 0x40) as *const u32;
/// ADC injected data register 2
pub const ADC_JDR2: *const u32 = (ADC1_BASE + 0x44) as *const u32;
/// ADC injected data register 3
pub const ADC_JDR3: *const u32 = (ADC1_BASE + 0x48) as *const u32;
/// ADC injected data register 4
pub const ADC_JDR4: *const u32 = (ADC1_BASE + 0x4C) as *const u32;
/// ADC regular data register
pub const ADC_DR: *const u32 = (ADC1_BASE + 0x50) as *const u32;

// ── SR Bit Definitions ─────────────────────────────────────────────────────

/// Analog watchdog flag
pub const SR_AWD: u32 = 1 << 0;
/// End of conversion
pub const SR_EOC: u32 = 1 << 1;
/// Injected channel end of conversion
pub const SR_JEOC: u32 = 1 << 2;

// ── CR1 Bit Definitions ────────────────────────────────────────────────────

/// Analog watchdog interrupt enable
pub const CR1_AWDIE: u32 = 1 << 6;
/// Scan mode enable
pub const CR1_SCAN: u32 = 1 << 8;
/// Analog watchdog enable on regular channels
pub const CR1_AWDEN: u32 = 1 << 23;
/// Single channel analog watchdog mode
pub const CR1_AWDSGL: u32 = 1 << 9;
/// Automatic injected group conversion
pub const CR1_JAUTO: u32 = 1 << 10;

// ── CR2 Bit Definitions ────────────────────────────────────────────────────

/// ADC enable
pub const CR2_ADON: u32 = 1 << 0;
/// External trigger event selection for injected group: JSWSTART (bits 14:12 = 0b111)
pub const CR2_JEXTSEL_JSWSTART: u32 = 7 << 12;
/// External trigger enable for injected channels
pub const CR2_JEXTTRIG: u32 = 1 << 15;
/// Start conversion of injected channels
pub const CR2_JSWSTART: u32 = 1 << 21;
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
