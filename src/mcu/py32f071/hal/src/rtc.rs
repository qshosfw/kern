//! Real-Time Clock (RTC) for PY32F071 (`0x4000_2800`, 32-bit LSI counter).
//!
//! Uses PY32F071's 32-bit `CNTH/CNTL` seconds counter with prescaler `32767` (~1 Hz on LSI)
//! and `PWR_CR1.DBP` unlock + `CRL.RSF` / `CRL.RTOFF` synchronization.

use pac::rtc::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16, // >= 2000
    pub month: u8, // 1..=12
    pub day: u8,   // 1..=31
    pub hour: u8,  // 0..=23
    pub min: u8,   // 0..=59
    pub sec: u8,   // 0..=59
}

pub struct RealTimeClock;

impl RealTimeClock {
    fn wait_rtoff() {
        unsafe {
            let mut t = 50_000u32;
            while (RTC_CRL.read_volatile() & CRL_RTOFF) == 0 && t > 0 {
                t -= 1;
            }
        }
    }

    fn wait_rsf() {
        unsafe {
            RTC_CRL.write_volatile(RTC_CRL.read_volatile() & !CRL_RSF);
            let mut t = 50_000u32;
            while (RTC_CRL.read_volatile() & CRL_RSF) == 0 && t > 0 {
                t -= 1;
            }
        }
    }

    fn write_pair(reg_hi: *mut u32, reg_lo: *mut u32, val: u32) {
        crate::rcc::enable_rtc_lsi();
        Self::wait_rtoff();
        unsafe {
            RTC_CRL.write_volatile(RTC_CRL.read_volatile() | CRL_CNF);
            reg_hi.write_volatile(val >> 16);
            reg_lo.write_volatile(val & 0xFFFF);
            RTC_CRL.write_volatile(RTC_CRL.read_volatile() & !CRL_CNF);
        }
        Self::wait_rtoff();
    }

    /// Initializes the RTC with LSI clock and `PRL = 32767` (~1 Hz tick).
    pub fn init() {
        crate::rcc::enable_rtc_lsi();
        Self::wait_rsf();
        Self::write_pair(RTC_PRLH, RTC_PRLL, 32_767);
    }

    /// Reads the raw 32-bit seconds counter after `CRL.RSF` synchronization.
    pub fn get_seconds() -> u32 {
        crate::rcc::enable_rtc_lsi();
        Self::wait_rsf();
        unsafe {
            let hi1 = RTC_CNTH.read_volatile() & 0xFFFF;
            let lo = RTC_CNTL.read_volatile() & 0xFFFF;
            let hi2 = RTC_CNTH.read_volatile() & 0xFFFF;
            if hi1 == hi2 {
                (hi1 << 16) | lo
            } else {
                (hi2 << 16) | (RTC_CNTL.read_volatile() & 0xFFFF)
            }
        }
    }

    /// Sets the 32-bit RTC seconds counter.
    pub fn set_seconds(secs: u32) {
        Self::write_pair(RTC_CNTH, RTC_CNTL, secs);
    }

    /// Reads the 32-bit RTC alarm register (`ALRH/ALRL`) retained in the backup domain.
    pub fn read_alarm_raw() -> u32 {
        crate::rcc::enable_pwr();
        crate::pwr::set_backup_write_enabled(true);
        unsafe { ((RTC_ALRH.read_volatile() & 0xFFFF) << 16) | (RTC_ALRL.read_volatile() & 0xFFFF) }
    }

    /// Writes the 32-bit RTC alarm register (`ALRH/ALRL`).
    pub fn write_alarm_raw(val: u32) {
        Self::write_pair(RTC_ALRH, RTC_ALRL, val);
    }

    /// Decodes the 32-bit counter (seconds since 2000-01-01 00:00:00) into UTC `DateTime`.
    pub fn get_time() -> DateTime {
        static DAYS_IN_MONTH: [u8; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        let mut s = Self::get_seconds();
        let sec = (s % 60) as u8;
        s /= 60;
        let min = (s % 60) as u8;
        s /= 60;
        let hour = (s % 24) as u8;
        let mut days = s / 24;

        let mut year = 2000u16;
        loop {
            let diy = if (year & 3) == 0 { 366 } else { 365 };
            if days < diy {
                break;
            }
            days -= diy;
            year += 1;
        }
        let mut month = 1u8;
        for (i, &base_dim) in DAYS_IN_MONTH.iter().enumerate() {
            let dim = base_dim as u32 + u32::from(i == 1 && (year & 3) == 0);
            if days < dim {
                month = (i as u8) + 1;
                break;
            }
            days -= dim;
        }
        DateTime {
            year,
            month,
            day: (days as u8) + 1,
            hour,
            min,
            sec,
        }
    }
}
