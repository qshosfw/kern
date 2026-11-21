//! IWDG (Independent Watchdog) Register Definitions for PY32F071xB

pub const IWDG_BASE: u32 = 0x4000_3000;

pub const IWDG_KR: *mut u32 = IWDG_BASE as *mut u32;
pub const IWDG_PR: *mut u32 = (IWDG_BASE + 0x04) as *mut u32;
pub const IWDG_RLR: *mut u32 = (IWDG_BASE + 0x08) as *mut u32;
pub const IWDG_SR: *mut u32 = (IWDG_BASE + 0x0C) as *mut u32;

pub const KR_KEY_RELOAD: u32 = 0xAAAA;
pub const KR_KEY_ENABLE: u32 = 0xCCCC;
pub const KR_KEY_UNLOCK: u32 = 0x5555;
