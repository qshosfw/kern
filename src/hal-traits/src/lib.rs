//! Platform Abstraction Traits for the Kern microkernel.
//!
//! These traits define the contract between portable firmware code
//! and hardware-specific implementations. The kernel, drivers, services,
//! and apps code against these traits — never against raw HAL.
//!
//! For hardware targets, the BSP implements these traits over real peripherals.
//! For the desktop simulator, SDL2/software backends implement them.

#![no_std]

pub mod audio;
pub mod crc;
pub mod display;
pub mod flash;
pub mod gpio;
pub mod input;
pub mod radio;
pub mod rtc;
pub mod serial;
pub mod spi;
pub mod storage;
pub mod sysinfo;
pub mod time;
pub mod usb;
pub mod wdt;
