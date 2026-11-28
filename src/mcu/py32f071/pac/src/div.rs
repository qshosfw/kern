//! Hardware 32-bit Divider (`DIV`) registers for PY32F071 (`0x4002_3800`).
//!
//! Completes 32÷32 unsigned and two's-complement signed division in ~8 AHB cycles,
//! eliminating software `__aeabi_uidiv` / `__aeabi_idiv` loops on Cortex-M0+.

use crate::AHB1_BASE;

pub const DIV_BASE: u32 = AHB1_BASE + 0x3800; // 0x4002_3800

/// Dividend register (`+0x00`)
pub const DIV_DEND: *mut u32 = DIV_BASE as *mut u32;
/// Divisor register (`+0x04` — writing `SOR` starts hardware calculation)
pub const DIV_SOR: *mut u32 = (DIV_BASE + 0x04) as *mut u32;
/// Quotient result register (`+0x08`)
pub const DIV_QUOT: *const u32 = (DIV_BASE + 0x08) as *const u32;
/// Remainder result register (`+0x0C`)
pub const DIV_REMA: *const u32 = (DIV_BASE + 0x0C) as *const u32;
/// Sign mode register (`+0x10`: `0` = unsigned, `1` = signed)
pub const DIV_SIGN: *mut u32 = (DIV_BASE + 0x10) as *mut u32;
/// Status register (`+0x14`)
pub const DIV_STAT: *const u32 = (DIV_BASE + 0x14) as *const u32;

/// Division complete flag
pub const STAT_DIV_END: u32 = 1 << 0;
/// Divide-by-zero flag
pub const STAT_DIV_ZERO: u32 = 1 << 1;
