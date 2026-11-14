//! I2C1 / I2C2 peripheral registers for PY32F071.

use crate::APB1_BASE;

pub const I2C1_BASE: u32 = APB1_BASE + 0x5400; // 0x4000_5400
pub const I2C2_BASE: u32 = APB1_BASE + 0x5800; // 0x4000_5800

pub const I2C1_CR1: *mut u32 = I2C1_BASE as *mut u32;
pub const I2C1_CR2: *mut u32 = (I2C1_BASE + 0x04) as *mut u32;
pub const I2C1_OAR1: *mut u32 = (I2C1_BASE + 0x08) as *mut u32;
pub const I2C1_OAR2: *mut u32 = (I2C1_BASE + 0x0C) as *mut u32;
pub const I2C1_DR: *mut u32 = (I2C1_BASE + 0x10) as *mut u32;
pub const I2C1_SR1: *mut u32 = (I2C1_BASE + 0x14) as *mut u32;
pub const I2C1_SR2: *const u32 = (I2C1_BASE + 0x18) as *const u32;
pub const I2C1_CCR: *mut u32 = (I2C1_BASE + 0x1C) as *mut u32;
pub const I2C1_TRISE: *mut u32 = (I2C1_BASE + 0x20) as *mut u32;

pub const CR1_PE: u32 = 1 << 0;
pub const CR1_START: u32 = 1 << 8;
pub const CR1_STOP: u32 = 1 << 9;
pub const CR1_ACK: u32 = 1 << 10;
pub const CR1_SWRST: u32 = 1 << 15;

pub const SR1_SB: u32 = 1 << 0;
pub const SR1_ADDR: u32 = 1 << 1;
pub const SR1_BTF: u32 = 1 << 2;
pub const SR1_RXNE: u32 = 1 << 6;
pub const SR1_TXE: u32 = 1 << 7;
pub const SR1_BERR: u32 = 1 << 8;
pub const SR1_ARLO: u32 = 1 << 9;
pub const SR1_AF: u32 = 1 << 10;

pub const SR2_MSL: u32 = 1 << 0;
pub const SR2_BUSY: u32 = 1 << 1;
