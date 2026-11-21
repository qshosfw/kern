//! External Interrupt Controller (EXTI) registers for PY32F071.
//!
//! PY32F071 places EXTI at AHB (`0x4002_1800`) with `EXTICR1..4` inside EXTI.

use crate::AHB1_BASE;

pub const EXTI_BASE: u32 = AHB1_BASE + 0x1800; // 0x4002_1800

pub const EXTI_RTSR: *mut u32 = EXTI_BASE as *mut u32; // Rising trigger selection (0x00)
pub const EXTI_FTSR: *mut u32 = (EXTI_BASE + 0x04) as *mut u32; // Falling trigger selection (0x04)
pub const EXTI_SWIER: *mut u32 = (EXTI_BASE + 0x08) as *mut u32; // Software interrupt event (0x08)
pub const EXTI_PR: *mut u32 = (EXTI_BASE + 0x0C) as *mut u32; // Pending register (0x0C)
pub const EXTI_EXTICR1: *mut u32 = (EXTI_BASE + 0x60) as *mut u32; // Lines 0..3
pub const EXTI_EXTICR2: *mut u32 = (EXTI_BASE + 0x64) as *mut u32; // Lines 4..7
pub const EXTI_EXTICR3: *mut u32 = (EXTI_BASE + 0x68) as *mut u32; // Lines 8..11
pub const EXTI_EXTICR4: *mut u32 = (EXTI_BASE + 0x6C) as *mut u32; // Lines 12..15
pub const EXTI_IMR: *mut u32 = (EXTI_BASE + 0x80) as *mut u32; // CPU wakeup interrupt mask (0x80)
pub const EXTI_EMR: *mut u32 = (EXTI_BASE + 0x84) as *mut u32; // CPU wakeup event mask (0x84)

pub const PORT_GPIOA: u32 = 0;
pub const PORT_GPIOB: u32 = 1;
pub const PORT_GPIOC: u32 = 2;
pub const PORT_GPIOF: u32 = 3;

#[inline(always)]
pub const fn exticr(pin: u8) -> *mut u32 {
    (EXTI_BASE + 0x60 + (((pin as u32) >> 2) * 4)) as *mut u32
}
