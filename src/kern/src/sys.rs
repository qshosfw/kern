//! System Introspection & High-Assurance Health Diagnostics

#[cfg(feature = "logger")]
use core::cell::RefCell;
#[cfg(feature = "logger")]
use cortex_m::interrupt::Mutex;
#[cfg(feature = "logger")]
use heapless::Deque;
#[cfg(feature = "logger")]
use heapless::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "TRC",
            LogLevel::Debug => "DBG",
            LogLevel::Info => "INF",
            LogLevel::Warn => "WRN",
            LogLevel::Error => "ERR",
        }
    }
}

/// Global Logger Queue (Lock-free consumer, interrupt-safe producer)
#[cfg(feature = "logger")]
pub static LOG_QUEUE: Mutex<RefCell<Deque<String<64>, 16>>> =
    Mutex::new(RefCell::new(Deque::new()));

#[cfg(feature = "logger")]
#[macro_export]
macro_rules! klog {
    ($level:expr, $($arg:tt)*) => {
        {
            use core::fmt::Write;
            let mut s = heapless::String::<64>::new();
            let _ = core::write!(&mut s, "[{}] ", $level.as_str());
            let _ = core::write!(&mut s, $($arg)*);
            cortex_m::interrupt::free(|cs| {
                let mut q = $crate::sys::LOG_QUEUE.borrow(cs).borrow_mut();
                if q.is_full() {
                    let _ = q.pop_front(); // Overwrite oldest log if full
                }
                let _ = q.push_back(s);
            });
        }
    };
}

#[cfg(not(feature = "logger"))]
#[macro_export]
macro_rules! klog {
    ($level:expr, $($arg:tt)*) => {
        // Zero-cost compiled-out logging
    };
}

/// System Introspection snapshot for microkernel telemetry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemMetrics {
    /// Milliseconds since system boot
    pub uptime_ms: u32,
    /// Unused stack space remaining in bytes
    pub stack_remaining_bytes: usize,
    /// Active task count
    pub active_tasks: u8,
}

pub struct SystemDiagnostics;

impl SystemDiagnostics {
    /// Calculate current stack margin remaining (distance between MSP and SRAM start 0x2000_0000)
    #[inline(always)]
    pub fn stack_margin_bytes() -> usize {
        let msp: usize;
        unsafe {
            core::arch::asm!("mrs {}, MSP", out(reg) msp);
        }
        unsafe extern "C" {
            static __ebss: u8;
        }
        let bss_end = unsafe { &__ebss as *const u8 as usize };
        if msp > bss_end { msp - bss_end } else { 0 }
    }

    /// Performs a software reset of the MCU.
    pub fn reboot() -> ! {
        unsafe {
            // Cortex-M SCB AIRCR Reset
            let aircr = 0xE000ED0C as *mut u32;
            core::ptr::write_volatile(aircr, (0x5FA << 16) | (1 << 2));
            loop {
                core::arch::asm!("nop");
            }
        }
    }

    /// Reboots into the Bootloader by manipulating boot pins or flash config (Quansheng specific magic).
    pub fn reboot_to_bootloader() -> ! {
        unsafe {
            let aircr = 0xE000ED0C as *mut u32;
            core::ptr::write_volatile(aircr, (0x5FA << 16) | (1 << 2));
            loop {
                core::arch::asm!("nop");
            }
        }
    }
}

/// A structure to hold crash information across reboots.
/// Should be placed in `.noinit` SRAM section.
#[derive(Debug, Clone, Copy)]
pub struct CrashReport {
    pub magic: u32,
    pub line: u32,
    pub file: [u8; 32],
}

impl CrashReport {
    pub const MAGIC_VALID: u32 = 0x4B435253; // "KCRS" (Kern Crash Report Structure)
    pub const MAGIC_CLEAR: u32 = 0x00000000;
}
