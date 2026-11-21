//! # Hardware Abstraction Layer for PY32F071
//!
//! Thin, safe-ish wrappers over raw PAC registers. Each module provides
//! a focused API for one peripheral, hiding register bit manipulation
//! behind clear function signatures.
//!
//! ## Modules
//!
//! - [`rcc`] — Clock tree initialization and peripheral clock gating
//! - [`gpio`] — Type-safe GPIO pin control (BSRR set/reset, IDR read, MODER config)
//! - [`spi`] — Hardware SPI1 master for LCD communication
//! - [`delay`] — SysTick-based blocking delays (ms and µs precision)
//! - [`time`] — TIM2 free-running 32-bit counter for timestamps
//! - [`uart`] — USART1 polled TX/RX for debug console

#![no_std]

pub use pac;

pub mod adc;
pub mod comp;
pub mod crc;
pub mod delay;
pub mod dma;
pub mod exti;
pub mod flash;
pub mod gpio;
pub mod i2c;
pub mod lptim;
pub mod pwr;
pub mod rcc;
pub mod rtc;
pub mod spi;
pub mod time;
pub mod uart;
pub mod uid;
pub mod wdt;
