//! USART1 peripheral registers for PY32F071.
//!
//! USART1 is on APB2 and is used for the debug serial console and Rigwire.
//! Pin mapping: PA9=TX (AF1), PA10=RX (AF1).

use crate::APB2_BASE;

pub const USART1_BASE: u32 = APB2_BASE + 0x3800; // 0x4001_3800

// ── Register Addresses ──────────────────────────────────────────────────────

/// Status register
pub const USART_SR: *const u32 = USART1_BASE as *const u32;
/// Data register
pub const USART_DR: *mut u32 = (USART1_BASE + 0x04) as *mut u32;
/// Baud rate register
pub const USART_BRR: *mut u32 = (USART1_BASE + 0x08) as *mut u32;
/// Control register 1
pub const USART_CR1: *mut u32 = (USART1_BASE + 0x0C) as *mut u32;
/// Control register 2
pub const USART_CR2: *mut u32 = (USART1_BASE + 0x10) as *mut u32;
/// Control register 3
pub const USART_CR3: *mut u32 = (USART1_BASE + 0x14) as *mut u32;

// ── SR Bit Definitions ─────────────────────────────────────────────────────

/// Transmit data register empty
pub const SR_TXE: u32 = 1 << 7;
/// Transmission complete
pub const SR_TC: u32 = 1 << 6;
/// Read data register not empty
pub const SR_RXNE: u32 = 1 << 5;
/// Overrun error
pub const SR_ORE: u32 = 1 << 3;
/// Noise error
pub const SR_NE: u32 = 1 << 2;
/// Framing error
pub const SR_FE: u32 = 1 << 1;
/// Parity error
pub const SR_PE: u32 = 1 << 0;

// ── CR1 Bit Definitions ────────────────────────────────────────────────────

/// USART enable
pub const CR1_UE: u32 = 1 << 13;
/// Word length: 0=8 data bits, 1=9 data bits
pub const CR1_M: u32 = 1 << 12;
/// Transmitter enable
pub const CR1_TE: u32 = 1 << 3;
/// Receiver enable
pub const CR1_RE: u32 = 1 << 2;
/// RXNE interrupt enable
pub const CR1_RXNEIE: u32 = 1 << 5;
