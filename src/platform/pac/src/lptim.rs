//! Low-Power Timer (LPTIM1) registers for PY32F071.

use crate::APB1_BASE;

pub const LPTIM1_BASE: u32 = APB1_BASE + 0x7C00; // 0x4000_7C00

pub const LPTIM_ISR: *const u32 = LPTIM1_BASE as *const u32;
pub const LPTIM_ICR: *mut u32 = (LPTIM1_BASE + 0x04) as *mut u32;
pub const LPTIM_IER: *mut u32 = (LPTIM1_BASE + 0x08) as *mut u32;
pub const LPTIM_CFGR: *mut u32 = (LPTIM1_BASE + 0x0C) as *mut u32;
pub const LPTIM_CR: *mut u32 = (LPTIM1_BASE + 0x10) as *mut u32;
pub const LPTIM_ARR: *mut u32 = (LPTIM1_BASE + 0x18) as *mut u32;
pub const LPTIM_CNT: *const u32 = (LPTIM1_BASE + 0x1C) as *const u32;

pub const ISR_ARRM: u32 = 1 << 1;
pub const ICR_ARRMCF: u32 = 1 << 1;
pub const IER_ARRMIE: u32 = 1 << 1;

pub const CR_ENABLE: u32 = 1 << 0;
pub const CR_SNGSTRT: u32 = 1 << 1;
pub const CR_CNTSTRT: u32 = 1 << 2;
pub const CR_RSTARE: u32 = 1 << 4;
