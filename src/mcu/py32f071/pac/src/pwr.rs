//! Power Controller (PWR) registers for PY32F071.

use crate::APB1_BASE;

pub const PWR_BASE: u32 = APB1_BASE + 0x7000; // 0x4000_7000

pub const PWR_CR1: *mut u32 = PWR_BASE as *mut u32; // 0x00
pub const PWR_CR2: *mut u32 = (PWR_BASE + 0x04) as *mut u32; // 0x04
pub const PWR_SR: *const u32 = (PWR_BASE + 0x14) as *const u32; // 0x14

// ── PWR_CR1 Bits ────────────────────────────────────────────────────────
pub const PWR_CR1_DBP: u32 = 1 << 8; // Disable backup domain write protection
pub const PWR_CR1_VOS_MASK: u32 = 0b11 << 9;
pub const PWR_CR1_FLS_SLPTIME_MASK: u32 = 0b11 << 12;
pub const PWR_CR1_FLS_SLPTIME_2US: u32 = 0b01 << 12; // Flash wake delay 2us after STOP
pub const PWR_CR1_LPR: u32 = 1 << 14; // Low-power regulator mode in Run/Stop

// ── PWR_CR2 Bits ────────────────────────────────────────────────────────
pub const PWR_CR2_PVDE: u32 = 1 << 0; // PVD enable
pub const PWR_CR2_SRCSEL: u32 = 1 << 2; // PVD voltage source select (0 = VDD, 1 = PB7)
pub const PWR_CR2_PVDT_SHIFT: u32 = 4; // PVD threshold bits 6:4
pub const PWR_CR2_PVDT_MASK: u32 = 0b111 << PWR_CR2_PVDT_SHIFT;
pub const PWR_CR2_FLTEN: u32 = 1 << 8; // PVD digital filter enable
pub const PWR_CR2_FLT_TIME_SHIFT: u32 = 9; // PVD digital filter time bits 11:9
pub const PWR_CR2_FLT_TIME_MASK: u32 = 0b111 << PWR_CR2_FLT_TIME_SHIFT;

// ── PWR_SR Bits ─────────────────────────────────────────────────────────
pub const PWR_SR_PVDO: u32 = 1 << 11; // PVD output status
