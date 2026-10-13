//! Low-Power Timer (LPTIM1) for PY32F071.
//!
//! Operational in Stop mode down to sub-microamp consumption.
//! Clocked by 32.768 kHz LSI or external crystal.
//!
//! Base Address: 0x4000_7C00 (APB1)

const LPTIM1_BASE: usize = 0x4000_7C00;
const LPTIM_ISR: *mut u32 = LPTIM1_BASE as *mut u32;
const LPTIM_ICR: *mut u32 = (LPTIM1_BASE + 0x04) as *mut u32;
#[allow(dead_code)]
const LPTIM_IER: *mut u32 = (LPTIM1_BASE + 0x08) as *mut u32;
const LPTIM_CFGR: *mut u32 = (LPTIM1_BASE + 0x0C) as *mut u32;
const LPTIM_CR: *mut u32 = (LPTIM1_BASE + 0x10) as *mut u32;
const LPTIM_ARR: *mut u32 = (LPTIM1_BASE + 0x18) as *mut u32;
const LPTIM_CNT: *mut u32 = (LPTIM1_BASE + 0x1C) as *mut u32;

pub struct LowPowerTimer;

impl LowPowerTimer {
    /// Initializes LPTIM1 with internal LSI 32.768 kHz clock source and /32 prescaler (~1024 Hz tick).
    pub fn init() {
        unsafe {
            // Enable LPTIM
            LPTIM_CR.write_volatile(1); // ENABLE = 1
            // Prescaler /32 (bit 11:9 = 0b101 = 5)
            LPTIM_CFGR.write_volatile(5 << 9);
        }
    }

    /// Sets up a single-shot timer for `ticks` and starts counting.
    pub fn start_single_shot(ticks: u16) {
        unsafe {
            LPTIM_CR.write_volatile(1); // Ensure enabled
            LPTIM_ARR.write_volatile(ticks as u32);
            LPTIM_CR.write_volatile(1 | (1 << 1)); // ENABLE + SNGSTRT
        }
    }

    /// Checks if the timer has expired (ARR match).
    #[inline(always)]
    pub fn is_expired() -> bool {
        unsafe { (LPTIM_ISR.read_volatile() & (1 << 1)) != 0 }
    }

    /// Clears the timer expired flag.
    #[inline(always)]
    pub fn clear_expired() {
        unsafe {
            LPTIM_ICR.write_volatile(1 << 1);
        }
    }

    /// Reads the current 16-bit counter value.
    #[inline(always)]
    pub fn read_counter() -> u16 {
        unsafe { (LPTIM_CNT.read_volatile() & 0xFFFF) as u16 }
    }
}
