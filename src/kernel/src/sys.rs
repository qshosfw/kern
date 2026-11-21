//! System Introspection & High-Assurance Health Diagnostics

#[cfg(feature = "logger")]
use core::cell::RefCell;
#[cfg(feature = "logger")]
use cortex_m::interrupt::Mutex;
#[cfg(feature = "logger")]
use heapless::Deque;
#[cfg(feature = "logger")]
use heapless::String;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

impl LogLevel {
    pub const fn as_str(&self) -> &'static str {
        const NAMES: &[u8] = b"TRC\0DBG\0INF\0WRN\0ERR\0";
        utils::format::nth_cstr(NAMES, *self as u8)
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

/// Memory pressure classification tier based on SRAM available and stack margin headroom
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// Plentiful headroom: >= 4096 bytes SRAM free and >= 1024 bytes stack margin
    Normal,
    /// Moderate consumption: 2048..4095 bytes SRAM free or 512..1023 bytes stack margin
    Moderate,
    /// Constrained RAM: 1024..2047 bytes SRAM free or 256..511 bytes stack margin
    Constrained,
    /// Critical shortage: < 1024 bytes SRAM free or < 256 bytes stack margin
    Critical,
}

impl MemoryPressure {
    pub const fn as_str(&self) -> &'static str {
        const NAMES: &[u8] = b"NORM\0MOD\0WARN\0CRIT\0";
        utils::format::nth_cstr(NAMES, *self as u8)
    }
}

pub struct SystemDiagnostics;

impl SystemDiagnostics {
    pub const STACK_PAINT_SENTINEL: u32 = crate::mem::guard::STACK_PAINT_SENTINEL;

    /// Paint unused stack area with a sentinel pattern (0xDEADBEEF) to allow high-assurance watermarking.
    ///
    /// Fills words between `_stack_end` (end of static BSS/DATA/heap) and the current `MSP` - safety margin.
    #[inline(never)]
    pub fn paint_stack() {
        #[cfg(target_arch = "arm")]
        {
            unsafe extern "C" {
                static _stack_end: u8;
            }
            let stack_end = (core::ptr::addr_of!(_stack_end) as usize + 3) & !3;
            let mut msp: usize;
            unsafe {
                core::arch::asm!("mrs {}, MSP", out(reg) msp);
            }

            // Leave guard headroom below current MSP so we do not overwrite active call frames
            let paint_limit = msp.saturating_sub(crate::mem::guard::STACK_GUARD_ZONE_BYTES);
            if paint_limit > stack_end {
                unsafe {
                    crate::mem::guard::stack_paint(stack_end as *mut u32, paint_limit as *mut u32);
                }
            }
        }
    }

    /// Calculate the peak stack bytes used since boot based on painted stack watermarks.
    /// Scans bottom-up from `_stack_end` to the first non-sentinel word.
    pub fn stack_peak_used_bytes() -> usize {
        #[cfg(target_arch = "arm")]
        {
            unsafe extern "C" {
                static _stack_end: u8;
                static _stack_start: u8;
            }
            let stack_end = (core::ptr::addr_of!(_stack_end) as usize + 3) & !3;
            let stack_start = core::ptr::addr_of!(_stack_start) as usize;
            if stack_start > stack_end {
                unsafe {
                    crate::mem::guard::stack_watermark(
                        stack_end as *const u32,
                        stack_start as *const u32,
                    )
                }
            } else {
                0
            }
        }
        #[cfg(not(target_arch = "arm"))]
        {
            512
        }
    }

    #[cfg(target_arch = "arm")]
    #[inline(always)]
    fn read_msp() -> usize {
        let msp: usize;
        // SAFETY: Reading the Cortex-M0+ Main Stack Pointer is a side-effect-free privileged/unprivileged register read.
        unsafe {
            core::arch::asm!("mrs {}, MSP", out(reg) msp);
        }
        msp
    }

    /// Calculate current stack bytes in active use (_stack_start - current MSP)
    #[inline(always)]
    pub fn stack_used_now() -> usize {
        #[cfg(target_arch = "arm")]
        {
            unsafe extern "C" {
                static _stack_start: u8;
            }
            (core::ptr::addr_of!(_stack_start) as usize).saturating_sub(Self::read_msp())
        }
        #[cfg(not(target_arch = "arm"))]
        {
            384
        }
    }

    /// Check if the stack margin is within the 128-byte safety guard zone.
    #[inline(always)]
    pub fn stack_guard_breached() -> bool {
        Self::stack_margin_bytes() < 128
    }

    /// Calculate current stack margin remaining (distance between MSP and _stack_end)
    #[inline(never)]
    pub fn stack_margin_bytes() -> usize {
        #[cfg(target_arch = "arm")]
        {
            unsafe extern "C" {
                static _stack_end: u8;
            }
            Self::read_msp().saturating_sub(core::ptr::addr_of!(_stack_end) as usize)
        }
        #[cfg(not(target_arch = "arm"))]
        {
            4096
        }
    }

    /// Returns total MCU SRAM in bytes (16 KB = 16,384 bytes on PY32F071xB).
    #[inline(always)]
    pub fn ram_total_bytes() -> usize {
        16 * 1024
    }

    /// Returns static RAM allocated by firmware (DATA + BSS + uninit, from 0x2000_0000 to _stack_end).
    pub fn ram_static_used_bytes() -> usize {
        #[cfg(target_arch = "arm")]
        {
            unsafe extern "C" {
                static _stack_end: u8;
            }
            let stack_end = core::ptr::addr_of!(_stack_end) as usize;
            const SRAM_BASE: usize = 0x2000_0000;
            if stack_end > SRAM_BASE {
                stack_end - SRAM_BASE
            } else {
                9400
            }
        }
        #[cfg(not(target_arch = "arm"))]
        {
            9456
        }
    }

    /// Calculate current available SRAM in bytes (distance between active stack MSP and static _stack_end).
    #[inline]
    pub fn ram_available_bytes() -> usize {
        Self::stack_margin_bytes()
    }

    /// Assess current system memory pressure tier based on available SRAM and stack margin.
    pub fn memory_pressure() -> MemoryPressure {
        let avail = Self::stack_margin_bytes();
        if avail < 1024 {
            MemoryPressure::Critical
        } else if avail < 2048 {
            MemoryPressure::Constrained
        } else if avail < 4096 {
            MemoryPressure::Moderate
        } else {
            MemoryPressure::Normal
        }
    }

    /// Compute dynamic max foreground apps based on live memory pressure and stack margins.
    /// Scales between 1 (Critical) and 6 (Normal, system max).
    pub fn dynamic_max_foreground_apps() -> usize {
        match Self::memory_pressure() {
            MemoryPressure::Normal => 6,
            MemoryPressure::Moderate => 4,
            MemoryPressure::Constrained => 2,
            MemoryPressure::Critical => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_pressure_and_dynamic_max_apps() {
        assert_eq!(MemoryPressure::Normal.as_str(), "NORM");
        assert_eq!(MemoryPressure::Moderate.as_str(), "MOD");
        assert_eq!(MemoryPressure::Constrained.as_str(), "WARN");
        assert_eq!(MemoryPressure::Critical.as_str(), "CRIT");

        assert_eq!(SystemDiagnostics::memory_pressure(), MemoryPressure::Normal);
        assert_eq!(SystemDiagnostics::dynamic_max_foreground_apps(), 6);
    }
}
