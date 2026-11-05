//! DAC1 peripheral registers for PY32F071.
//!
//! DAC1 is on APB1. Channel 1 output is on PA4 (VOICE_DAC).

use crate::APB1_BASE;

pub const DAC_BASE: u32 = APB1_BASE + 0x7400; // 0x4000_7400

pub const DAC_CR: *mut u32 = DAC_BASE as *mut u32;
pub const DAC_SWTRIGR: *mut u32 = (DAC_BASE + 0x04) as *mut u32;
pub const DAC_DHR12R1: *mut u32 = (DAC_BASE + 0x08) as *mut u32;
pub const DAC_DOR1: *const u32 = (DAC_BASE + 0x2C) as *const u32;

pub const CR_EN1: u32 = 1 << 0;
pub const CR_BOFF1: u32 = 1 << 1;
pub const SWTRIGR_SWTRIG1: u32 = 1 << 0;
