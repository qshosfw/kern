//! GPIO peripheral registers for PY32F071.
//!
//! The PY32F071 has 4 GPIO ports: A, B, C, F.
//! Each port occupies 0x400 bytes starting at IOPORT_BASE (0x5000_0000).
//!
//! Register layout per port (offsets from port base):
//!   0x00  MODER   — Mode register (2 bits per pin)
//!   0x04  OTYPER  — Output type register
//!   0x08  OSPEEDR — Output speed register
//!   0x0C  PUPDR   — Pull-up/pull-down register
//!   0x10  IDR     — Input data register (read-only)
//!   0x14  ODR     — Output data register
//!   0x18  BSRR    — Bit set/reset register (write-only)
//!   0x20  AFRL    — Alternate function low (pins 0-7)
//!   0x24  AFRH    — Alternate function high (pins 8-15)

use crate::IOPORT_BASE;

// ── Port Base Addresses ─────────────────────────────────────────────────────

pub const GPIOA_BASE: u32 = IOPORT_BASE + 0x0000;
pub const GPIOB_BASE: u32 = IOPORT_BASE + 0x0400;
pub const GPIOC_BASE: u32 = IOPORT_BASE + 0x0800;
pub const GPIOF_BASE: u32 = IOPORT_BASE + 0x1400;

// ── Register Offsets ────────────────────────────────────────────────────────

pub const MODER_OFFSET: u32 = 0x00;
pub const OTYPER_OFFSET: u32 = 0x04;
pub const OSPEEDR_OFFSET: u32 = 0x08;
pub const PUPDR_OFFSET: u32 = 0x0C;
pub const IDR_OFFSET: u32 = 0x10;
pub const ODR_OFFSET: u32 = 0x14;
pub const BSRR_OFFSET: u32 = 0x18;
pub const AFRL_OFFSET: u32 = 0x20;
pub const AFRH_OFFSET: u32 = 0x24;

// ── Pin Mode Values (2 bits per pin in MODER) ───────────────────────────────

/// Input mode (reset state for most pins)
pub const MODE_INPUT: u32 = 0b00;
/// General-purpose output
pub const MODE_OUTPUT: u32 = 0b01;
/// Alternate function (SPI, UART, etc.)
pub const MODE_AF: u32 = 0b10;
/// Analog mode (ADC input, lowest power for unused pins)
pub const MODE_ANALOG: u32 = 0b11;

// ── Output Type Values (1 bit per pin in OTYPER) ────────────────────────────

pub const OTYPE_PUSHPULL: u32 = 0;
pub const OTYPE_OPENDRAIN: u32 = 1;

// ── Output Speed Values (2 bits per pin in OSPEEDR) ─────────────────────────

pub const SPEED_LOW: u32 = 0b00;
pub const SPEED_MEDIUM: u32 = 0b01;
pub const SPEED_HIGH: u32 = 0b11;

// ── Pull-Up/Down Values (2 bits per pin in PUPDR) ───────────────────────────

pub const PULL_NONE: u32 = 0b00;
pub const PULL_UP: u32 = 0b01;
pub const PULL_DOWN: u32 = 0b10;

// ── Alternate Function IDs ──────────────────────────────────────────────────

/// AF0: SPI1 (PA5=SCK, PA7=MOSI), USART2, TIM1
pub const AF0: u32 = 0;
/// AF1: USART1 (PA9=TX, PA10=RX), I2C, TIM3
pub const AF1: u32 = 1;
/// AF2: TIM2, TIM16, TIM17
pub const AF2: u32 = 2;
pub const AF3: u32 = 3;
pub const AF4: u32 = 4;
pub const AF5: u32 = 5;
pub const AF6: u32 = 6;
pub const AF7: u32 = 7;
pub const AF8: u32 = 8;
pub const AF9: u32 = 9;
pub const AF10: u32 = 10;
pub const AF11: u32 = 11;
pub const AF12: u32 = 12;
pub const AF13: u32 = 13;
pub const AF14: u32 = 14;
pub const AF15: u32 = 15;

// ── Pin Mask Constants ──────────────────────────────────────────────────────

pub const PIN_0: u32 = 1 << 0;
pub const PIN_1: u32 = 1 << 1;
pub const PIN_2: u32 = 1 << 2;
pub const PIN_3: u32 = 1 << 3;
pub const PIN_4: u32 = 1 << 4;
pub const PIN_5: u32 = 1 << 5;
pub const PIN_6: u32 = 1 << 6;
pub const PIN_7: u32 = 1 << 7;
pub const PIN_8: u32 = 1 << 8;
pub const PIN_9: u32 = 1 << 9;
pub const PIN_10: u32 = 1 << 10;
pub const PIN_11: u32 = 1 << 11;
pub const PIN_12: u32 = 1 << 12;
pub const PIN_13: u32 = 1 << 13;
pub const PIN_14: u32 = 1 << 14;
pub const PIN_15: u32 = 1 << 15;
