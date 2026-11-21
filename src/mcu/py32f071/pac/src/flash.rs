//! Flash controller registers for PY32F071.
//!
//! Used for flash wait-state configuration and option byte access.

use crate::AHB1_BASE;

pub const FLASH_BASE: u32 = AHB1_BASE + 0x2000; // 0x4002_2000

/// Flash access control register
pub const FLASH_ACR: *mut u32 = FLASH_BASE as *mut u32;
/// Flash key register (0x08)
pub const FLASH_KEYR: *mut u32 = (FLASH_BASE + 0x08) as *mut u32;
/// Flash option key register (0x0C)
pub const FLASH_OPTKEYR: *mut u32 = (FLASH_BASE + 0x0C) as *mut u32;
/// Flash status register (0x10)
pub const FLASH_SR: *mut u32 = (FLASH_BASE + 0x10) as *mut u32;
/// Flash control register (0x14)
pub const FLASH_CR: *mut u32 = (FLASH_BASE + 0x14) as *mut u32;
/// Flash Option register (0x20)
pub const FLASH_OPTR: *const u32 = (FLASH_BASE + 0x20) as *const u32;
/// Flash Option BORCR register (0x24)
pub const FLASH_BORCR: *const u32 = (FLASH_BASE + 0x24) as *const u32;
/// Flash Option Write Protection register (0x2C)
pub const FLASH_WRPR: *const u32 = (FLASH_BASE + 0x2C) as *const u32;

/// Option Bytes base address in System Memory
pub const OB_BASE: u32 = 0x1FFF_3100;

// ── ACR Bit Definitions ────────────────────────────────────────────────────

/// Flash latency (wait states): 0=0WS (≤24MHz), 1=1WS (≤48MHz), 2=2WS (≤72MHz)
pub const ACR_LATENCY_MASK: u32 = 0b111;
pub const ACR_LATENCY_0WS: u32 = 0;
pub const ACR_LATENCY_1WS: u32 = 1;
pub const ACR_LATENCY_2WS: u32 = 2;

// ── OPTR Bit Definitions ───────────────────────────────────────────────────

pub const FLASH_OPTR_RDP_POS: u32 = 0;
pub const FLASH_OPTR_RDP_MASK: u32 = 0xFF;

/// Option Byte RDP Level 0: No Protection (Flash and SRAM accessible by debugger)
pub const FLASH_OPTR_RDP_LEVEL_0: u8 = 0xAA;
/// Option Byte RDP Level 1: Readout Protection Active (Debugger cannot read Flash)
pub const FLASH_OPTR_RDP_LEVEL_1: u8 = 0x55;

/// Watchdog mode: 0 = Hardware watchdog, 1 = Software watchdog
pub const FLASH_OPTR_IWDG_SW: u32 = 1 << 11;
/// Window watchdog mode: 0 = Hardware window watchdog, 1 = Software window watchdog
pub const FLASH_OPTR_WWDG_SW: u32 = 1 << 12;
/// NRST pin mode: 0 = Reset input pin, 1 = Standard GPIO
pub const FLASH_OPTR_NRST_MODE: u32 = 1 << 13;
/// Boot pin selection bit 1
pub const FLASH_OPTR_NBOOT1: u32 = 1 << 14;
/// IWDG counter behavior in STOP mode: 0 = Freeze, 1 = Active
pub const FLASH_OPTR_IWDG_STOP: u32 = 1 << 15;

// ── SR / CR Bit Definitions ────────────────────────────────────────────────

pub const SR_BSY: u32 = 1 << 0;
pub const SR_EOP: u32 = 1 << 1;
pub const SR_WRPERR: u32 = 1 << 4;
pub const SR_PGERR: u32 = 1 << 5;
pub const SR_ERR_MASK: u32 = SR_WRPERR | SR_PGERR;

pub const CR_PG: u32 = 1 << 0;
pub const CR_PER: u32 = 1 << 1;
pub const CR_MER: u32 = 1 << 2;
pub const CR_PGSTRT: u32 = 1 << 16;
pub const CR_EOPIE: u32 = 1 << 24;
pub const CR_LOCK: u32 = 1 << 31;

pub const KEY1: u32 = 0x4567_0123;
pub const KEY2: u32 = 0xCDEF_89AB;
