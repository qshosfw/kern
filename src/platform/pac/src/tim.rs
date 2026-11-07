//! Timer peripheral registers for PY32F071.
//!
//! Includes general-purpose, advanced, and basic timers.

use crate::APB1_BASE;

pub const TIM1_BASE: u32 = 0x4001_2C00;
pub const TIM2_BASE: u32 = APB1_BASE; // 0x4000_0000
pub const TIM3_BASE: u32 = 0x4000_0400;
pub const TIM6_BASE: u32 = 0x4000_1000;
pub const TIM7_BASE: u32 = 0x4000_1400;
pub const TIM14_BASE: u32 = 0x4000_2000;
pub const TIM16_BASE: u32 = 0x4001_4400;
pub const TIM17_BASE: u32 = 0x4001_4800;

#[derive(Clone, Copy)]
pub struct TimerInstance(pub u32);

impl TimerInstance {
    #[inline(always)]
    pub fn cr1(&self) -> *mut u32 {
        self.0 as *mut u32
    }
    #[inline(always)]
    pub fn cr2(&self) -> *mut u32 {
        (self.0 + 0x04) as *mut u32
    }
    #[inline(always)]
    pub fn smcr(&self) -> *mut u32 {
        (self.0 + 0x08) as *mut u32
    }
    #[inline(always)]
    pub fn dier(&self) -> *mut u32 {
        (self.0 + 0x0C) as *mut u32
    }
    #[inline(always)]
    pub fn sr(&self) -> *mut u32 {
        (self.0 + 0x10) as *mut u32
    }
    #[inline(always)]
    pub fn egr(&self) -> *mut u32 {
        (self.0 + 0x14) as *mut u32
    }
    #[inline(always)]
    pub fn ccmr1(&self) -> *mut u32 {
        (self.0 + 0x18) as *mut u32
    }
    #[inline(always)]
    pub fn ccmr2(&self) -> *mut u32 {
        (self.0 + 0x1C) as *mut u32
    }
    #[inline(always)]
    pub fn ccer(&self) -> *mut u32 {
        (self.0 + 0x20) as *mut u32
    }
    #[inline(always)]
    pub fn cnt(&self) -> *mut u32 {
        (self.0 + 0x24) as *mut u32
    }
    #[inline(always)]
    pub fn psc(&self) -> *mut u32 {
        (self.0 + 0x28) as *mut u32
    }
    #[inline(always)]
    pub fn arr(&self) -> *mut u32 {
        (self.0 + 0x2C) as *mut u32
    }
    #[inline(always)]
    pub fn ccr1(&self) -> *mut u32 {
        (self.0 + 0x34) as *mut u32
    }
    #[inline(always)]
    pub fn ccr2(&self) -> *mut u32 {
        (self.0 + 0x38) as *mut u32
    }
    #[inline(always)]
    pub fn ccr3(&self) -> *mut u32 {
        (self.0 + 0x3C) as *mut u32
    }
    #[inline(always)]
    pub fn ccr4(&self) -> *mut u32 {
        (self.0 + 0x40) as *mut u32
    }
}

// ── Legacy Register Addresses for TIM2 ─────────────────────────────────────

/// Control register 1
pub const TIM2_CR1: *mut u32 = TIM2_BASE as *mut u32;
/// Prescaler
pub const TIM2_PSC: *mut u32 = (TIM2_BASE + 0x28) as *mut u32;
/// Auto-reload register
pub const TIM2_ARR: *mut u32 = (TIM2_BASE + 0x2C) as *mut u32;
/// Counter register
pub const TIM2_CNT: *const u32 = (TIM2_BASE + 0x24) as *const u32;
/// Event generation register
pub const TIM2_EGR: *mut u32 = (TIM2_BASE + 0x14) as *mut u32;

// ── CR1 Bit Definitions ────────────────────────────────────────────────────

/// Counter enable
pub const CR1_CEN: u32 = 1 << 0;
/// Update disable
pub const CR1_UDIS: u32 = 1 << 1;

/// EGR: Update generation (force reload of prescaler/ARR)
pub const EGR_UG: u32 = 1 << 0;

// ── DIER Bit Definitions ───────────────────────────────────────────────────

pub const DIER_UIE: u32 = 1 << 0;
pub const DIER_CC1IE: u32 = 1 << 1;
pub const DIER_UDE: u32 = 1 << 8;
