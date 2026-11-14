//! Hardware CRC32 calculation unit registers for PY32F071.

use crate::AHB1_BASE;

pub const CRC_BASE: u32 = AHB1_BASE + 0x3000; // 0x4002_3000

pub const CRC_DR: *mut u32 = CRC_BASE as *mut u32;
pub const CRC_IDR: *mut u8 = (CRC_BASE + 0x04) as *mut u8;
pub const CRC_CR: *mut u32 = (CRC_BASE + 0x08) as *mut u32;

pub const CR_RESET: u32 = 1 << 0;
