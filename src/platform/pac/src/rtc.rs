//! Real-Time Clock (RTC) registers for PY32F071.
//!
//! Uses a 32-bit programmable counter (`CNTH/CNTL`) with prescaler (`PRLH/PRLL`)
//! and alarm (`ALRH/ALRL`), plus `BKP_RTCCR`.

use crate::APB1_BASE;

pub const RTC_BASE: u32 = APB1_BASE + 0x2800; // 0x4000_2800

pub const RTC_CRH: *mut u32 = RTC_BASE as *mut u32;
pub const RTC_CRL: *mut u32 = (RTC_BASE + 0x04) as *mut u32;
pub const RTC_PRLH: *mut u32 = (RTC_BASE + 0x08) as *mut u32;
pub const RTC_PRLL: *mut u32 = (RTC_BASE + 0x0C) as *mut u32;
pub const RTC_DIVH: *const u32 = (RTC_BASE + 0x10) as *const u32;
pub const RTC_DIVL: *const u32 = (RTC_BASE + 0x14) as *const u32;
pub const RTC_CNTH: *mut u32 = (RTC_BASE + 0x18) as *mut u32;
pub const RTC_CNTL: *mut u32 = (RTC_BASE + 0x1C) as *mut u32;
pub const RTC_ALRH: *mut u32 = (RTC_BASE + 0x20) as *mut u32;
pub const RTC_ALRL: *mut u32 = (RTC_BASE + 0x24) as *mut u32;
pub const RTC_BKP_RTCCR: *mut u32 = (RTC_BASE + 0x2C) as *mut u32;

pub const CRH_SECIE: u32 = 1 << 0;
pub const CRH_ALRIE: u32 = 1 << 1;
pub const CRH_OWIE: u32 = 1 << 2;

pub const CRL_SECF: u32 = 1 << 0;
pub const CRL_ALRF: u32 = 1 << 1;
pub const CRL_OWF: u32 = 1 << 2;
pub const CRL_RSF: u32 = 1 << 3;
pub const CRL_CNF: u32 = 1 << 4;
pub const CRL_RTOFF: u32 = 1 << 5;
