//! Window Watchdog (WWDG) registers for PY32F071.

use crate::APB1_BASE;

pub const WWDG_BASE: u32 = APB1_BASE + 0x2C00; // 0x4000_2C00

pub const WWDG_CR: *mut u32 = WWDG_BASE as *mut u32;
pub const WWDG_CFR: *mut u32 = (WWDG_BASE + 0x04) as *mut u32;
pub const WWDG_SR: *mut u32 = (WWDG_BASE + 0x08) as *mut u32;
