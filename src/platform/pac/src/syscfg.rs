//! System Configuration Controller (SYSCFG) registers for PY32F071.

use crate::APB2_BASE;

pub const SYSCFG_BASE: u32 = APB2_BASE; // 0x4001_0000

pub const SYSCFG_CFGR1: *mut u32 = SYSCFG_BASE as *mut u32;
pub const SYSCFG_CFGR2: *mut u32 = (SYSCFG_BASE + 0x04) as *mut u32;
pub const SYSCFG_CFGR3: *mut u32 = (SYSCFG_BASE + 0x08) as *mut u32;
pub const SYSCFG_CFGR4: *mut u32 = (SYSCFG_BASE + 0x0C) as *mut u32;
pub const SYSCFG_PAENS: *mut u32 = (SYSCFG_BASE + 0x10) as *mut u32;
pub const SYSCFG_PBENS: *mut u32 = (SYSCFG_BASE + 0x14) as *mut u32;
pub const SYSCFG_PCENS: *mut u32 = (SYSCFG_BASE + 0x18) as *mut u32;
pub const SYSCFG_PFENS: *mut u32 = (SYSCFG_BASE + 0x1C) as *mut u32;
