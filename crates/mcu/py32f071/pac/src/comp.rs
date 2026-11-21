//! Analog Comparators (COMP1..3) registers for PY32F071.

use crate::APB2_BASE;

pub const COMP1_BASE: u32 = APB2_BASE + 0x0200; // 0x4001_0200
pub const COMP2_BASE: u32 = APB2_BASE + 0x0210; // 0x4001_0210
pub const COMP3_BASE: u32 = APB2_BASE + 0x0220; // 0x4001_0220

pub const COMP1_CSR: *mut u32 = COMP1_BASE as *mut u32;
pub const COMP1_FR: *mut u32 = (COMP1_BASE + 0x04) as *mut u32;
pub const COMP2_CSR: *mut u32 = COMP2_BASE as *mut u32;
pub const COMP2_FR: *mut u32 = (COMP2_BASE + 0x04) as *mut u32;
pub const COMP3_CSR: *mut u32 = COMP3_BASE as *mut u32;
pub const COMP3_FR: *mut u32 = (COMP3_BASE + 0x04) as *mut u32;

pub const CSR_EN: u32 = 1 << 0;
pub const CSR_COMP_OUT: u32 = 1 << 30;
