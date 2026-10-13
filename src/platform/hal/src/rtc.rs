//! Real-Time Clock (RTC), Calendar, and Backup Domain for PY32F071.
//!
//! Base Address: 0x4000_2800 (APB1)
//! Clock source: LSI (32.768 kHz) or LSE.
//!
//! Features:
//! - Sub-second, second, minute, hour, day, month, year BCD calendar.
//! - Alarm interrupts.
//! - Battery/Reset-persistent 32-bit backup registers (BKP0R..BKP4R).

const RTC_BASE: usize = 0x4000_2800;
const RTC_TR: *mut u32 = RTC_BASE as *mut u32;
const RTC_DR: *mut u32 = (RTC_BASE + 0x04) as *mut u32;
#[allow(dead_code)]
const RTC_CR: *mut u32 = (RTC_BASE + 0x08) as *mut u32;
const RTC_ISR: *mut u32 = (RTC_BASE + 0x0C) as *mut u32;
const RTC_PRER: *mut u32 = (RTC_BASE + 0x10) as *mut u32;
const RTC_WPR: *mut u32 = (RTC_BASE + 0x24) as *mut u32;
const RTC_BKP0R: *mut u32 = (RTC_BASE + 0x50) as *mut u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16, // e.g. 2026
    pub month: u8, // 1..=12
    pub day: u8,   // 1..=31
    pub hour: u8,  // 0..=23
    pub min: u8,   // 0..=59
    pub sec: u8,   // 0..=59
}

fn bcd_to_bin(bcd: u8) -> u8 {
    ((bcd >> 4) * 10) + (bcd & 0x0F)
}

fn bin_to_bcd(bin: u8) -> u8 {
    ((bin / 10) << 4) | (bin % 10)
}

pub struct RealTimeClock;

impl RealTimeClock {
    #[inline(always)]
    fn unlock() {
        unsafe {
            RTC_WPR.write_volatile(0xCA);
            RTC_WPR.write_volatile(0x53);
        }
    }

    #[inline(always)]
    fn lock() {
        unsafe {
            RTC_WPR.write_volatile(0xFF);
        }
    }

    /// Initializes the RTC with standard LSI 32.768 kHz prescalers (128 async, 256 sync).
    pub fn init() {
        Self::unlock();
        unsafe {
            // Enter init mode
            RTC_ISR.write_volatile(RTC_ISR.read_volatile() | (1 << 7));
            while (RTC_ISR.read_volatile() & (1 << 6)) == 0 {} // Wait for INITF

            // Set prescalers: PREDIV_A = 127, PREDIV_S = 255 for 1Hz
            RTC_PRER.write_volatile((127 << 16) | 255);

            // Exit init mode
            RTC_ISR.write_volatile(RTC_ISR.read_volatile() & !(1 << 7));
        }
        Self::lock();
    }

    /// Reads the current calendar date and time.
    pub fn get_time() -> DateTime {
        let (tr, dr) = unsafe {
            let tr = RTC_TR.read_volatile();
            let dr = RTC_DR.read_volatile();
            (tr, dr)
        };

        let sec = bcd_to_bin((tr & 0x7F) as u8);
        let min = bcd_to_bin(((tr >> 8) & 0x7F) as u8);
        let hour = bcd_to_bin(((tr >> 16) & 0x3F) as u8);

        let day = bcd_to_bin((dr & 0x3F) as u8);
        let month = bcd_to_bin(((dr >> 8) & 0x1F) as u8);
        let year = 2000 + (bcd_to_bin(((dr >> 16) & 0xFF) as u8) as u16);

        DateTime {
            year,
            month,
            day,
            hour,
            min,
            sec,
        }
    }

    /// Sets the calendar date and time.
    pub fn set_time(dt: &DateTime) {
        Self::unlock();
        unsafe {
            // Enter init mode
            RTC_ISR.write_volatile(RTC_ISR.read_volatile() | (1 << 7));
            while (RTC_ISR.read_volatile() & (1 << 6)) == 0 {}

            let tr = ((bin_to_bcd(dt.hour) as u32) << 16)
                | ((bin_to_bcd(dt.min) as u32) << 8)
                | (bin_to_bcd(dt.sec) as u32);
            RTC_TR.write_volatile(tr);

            let yr_2digit = (dt.year % 100) as u8;
            let dr = ((bin_to_bcd(yr_2digit) as u32) << 16)
                | ((bin_to_bcd(dt.month) as u32) << 8)
                | (bin_to_bcd(dt.day) as u32);
            RTC_DR.write_volatile(dr);

            // Exit init mode
            RTC_ISR.write_volatile(RTC_ISR.read_volatile() & !(1 << 7));
        }
        Self::lock();
    }

    /// Reads one of the 5 hardware backup registers (idx 0..=4).
    /// Preserved across soft resets and sleep modes as long as VDD is present.
    pub fn read_backup(idx: usize) -> u32 {
        if idx > 4 {
            return 0;
        }
        unsafe {
            let reg = (RTC_BKP0R as usize + idx * 4) as *const u32;
            reg.read_volatile()
        }
    }

    /// Writes one of the 5 hardware backup registers (idx 0..=4).
    pub fn write_backup(idx: usize, val: u32) {
        if idx > 4 {
            return;
        }
        Self::unlock();
        unsafe {
            let reg = (RTC_BKP0R as usize + idx * 4) as *mut u32;
            reg.write_volatile(val);
        }
        Self::lock();
    }
}
