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

/// 6-bit DMA peripheral request selector mask per channel inside `SYSCFG_CFGR3`/`CFGR4`.
pub const DMA_MAP_MASK: u32 = 0x3F;

/// Returns `(cfgr_reg_ptr, bit_shift)` for DMA1 channel `ch` (`1..=7`).
#[inline(always)]
pub const fn dma_map_reg_and_shift(ch: u8) -> (*mut u32, u32) {
    if ch <= 4 {
        (SYSCFG_CFGR3, ((ch - 1) as u32) << 3)
    } else {
        (SYSCFG_CFGR4, ((ch - 5) as u32) << 3)
    }
}
