//! External Interrupt Controller (EXTI) registers for PY32F071.

pub const EXTI_BASE: u32 = 0x4001_0400;

pub const EXTI_IMR: *mut u32 = EXTI_BASE as *mut u32; // Interrupt mask
pub const EXTI_EMR: *mut u32 = (EXTI_BASE + 0x04) as *mut u32; // Event mask
pub const EXTI_RTSR: *mut u32 = (EXTI_BASE + 0x08) as *mut u32; // Rising trigger selection
pub const EXTI_FTSR: *mut u32 = (EXTI_BASE + 0x0C) as *mut u32; // Falling trigger selection
pub const EXTI_SWIER: *mut u32 = (EXTI_BASE + 0x10) as *mut u32; // Software interrupt event
pub const EXTI_PR: *mut u32 = (EXTI_BASE + 0x14) as *mut u32; // Pending register
