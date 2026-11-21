//! SPI1 peripheral registers for PY32F071.
//!
//! SPI1 is on APB2 and is used for the ST7565 LCD (hardware SPI mode).
//! Pin mapping: PA5=SCK (AF0), PA7=MOSI (AF0).
//! The BK4819 uses separate bit-bang SPI on different pins.

use crate::{APB1_BASE, APB2_BASE};

pub const SPI1_BASE: u32 = APB2_BASE + 0x3000; // 0x4001_3000
pub const SPI2_BASE: u32 = APB1_BASE + 0x3800; // 0x4000_3800

// ── Register Addresses ──────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct SpiInstance {
    pub base: u32,
}

pub const SPI1: SpiInstance = SpiInstance { base: SPI1_BASE };
pub const SPI2: SpiInstance = SpiInstance { base: SPI2_BASE };

impl SpiInstance {
    #[inline(always)]
    pub fn cr1(&self) -> *mut u32 {
        self.base as *mut u32
    }
    #[inline(always)]
    pub fn cr2(&self) -> *mut u32 {
        (self.base + 0x04) as *mut u32
    }
    #[inline(always)]
    pub fn sr(&self) -> *mut u32 {
        (self.base + 0x08) as *mut u32
    }
    #[inline(always)]
    pub fn dr(&self) -> *mut u32 {
        (self.base + 0x0C) as *mut u32
    }
    #[inline(always)]
    pub fn dr_8(&self) -> *mut u8 {
        (self.base + 0x0C) as *mut u8
    }
}

// ── CR1 Bit Definitions ────────────────────────────────────────────────────

/// Clock phase: 0=first edge, 1=second edge
pub const CR1_CPHA: u32 = 1 << 0;
/// Clock polarity: 0=idle low, 1=idle high
pub const CR1_CPOL: u32 = 1 << 1;
/// Master mode selection
pub const CR1_MSTR: u32 = 1 << 2;
/// Baud rate control (3 bits): fPCLK/2^(BR+1)
pub const CR1_BR_SHIFT: u32 = 3;
pub const CR1_BR_MASK: u32 = 0b111 << CR1_BR_SHIFT;
/// SPI enable
pub const CR1_SPE: u32 = 1 << 6;
/// Frame format: 0=MSB first, 1=LSB first
pub const CR1_LSBFIRST: u32 = 1 << 7;
/// Internal slave select
pub const CR1_SSI: u32 = 1 << 8;
/// Software slave management
pub const CR1_SSM: u32 = 1 << 9;
/// Receive only mode
pub const CR1_RXONLY: u32 = 1 << 10;
/// Data frame format: 0=8bit, 1=16bit
pub const CR1_DFF: u32 = 1 << 11;
/// Bidirectional data mode enable
pub const CR1_BIDIMODE: u32 = 1 << 15;

// ── CR2 Bit Definitions ────────────────────────────────────────────────────

/// RX buffer DMA enable
pub const CR2_RXDMAEN: u32 = 1 << 0;
/// TX buffer DMA enable
pub const CR2_TXDMAEN: u32 = 1 << 1;
/// SS output enable
pub const CR2_SSOE: u32 = 1 << 2;
/// Clear TX FIFO
pub const CR2_CLRTXFIFO: u32 = 1 << 4;

// ── SR Bit Definitions ─────────────────────────────────────────────────────

/// Receive buffer not empty
pub const SR_RXNE: u32 = 1 << 0;
/// Transmit buffer empty
pub const SR_TXE: u32 = 1 << 1;
/// Busy flag
pub const SR_BSY: u32 = 1 << 7;
