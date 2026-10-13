//! Flight Recorder — persistent crash data surviving warm resets.
//!
//! A 64-byte record stored in a `.noinit` SRAM section that is NOT
//! zeroed during startup. After a crash, the bootloader or next boot
//! can read this to determine what happened.

/// Magic value indicating a valid flight record.
/// Magic value indicating a valid flight record.
pub const FLIGHT_MAGIC: u32 = 0x464C_5447; // "FLTG"

/// Crash record persisted across warm resets.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FlightRecord {
    /// Magic number (FLIGHT_MAGIC if valid)
    pub magic: u32,
    /// RCC_CSR reset cause bits
    pub reset_cause: u32,
    /// Program counter at time of fault
    pub fault_pc: u32,
    /// Link register at time of fault
    pub fault_lr: u32,
    /// Minimum observed stack margin in bytes
    pub stack_watermark: u32,
    /// System uptime in milliseconds at fault
    pub uptime_ms: u32,
    /// Active application ID at fault
    pub app_id: u8,
    /// Consecutive crash counter across warm resets
    pub crash_counter: u8,
    /// Reserved for future use (padded to 64 bytes total)
    pub _reserved: [u8; 34],
}

impl FlightRecord {
    pub const fn new() -> Self {
        Self {
            magic: 0,
            reset_cause: 0,
            fault_pc: 0,
            fault_lr: 0,
            stack_watermark: 0,
            uptime_ms: 0,
            app_id: 0,
            crash_counter: 0,
            _reserved: [0; 34],
        }
    }

    /// Check if the record contains valid data from a previous boot.
    pub fn is_valid(&self) -> bool {
        self.magic == FLIGHT_MAGIC
    }

    /// Clear the record (call at successful boot).
    pub fn clear(&mut self) {
        self.magic = 0;
        self.crash_counter = 0;
    }

    /// Write a crash record.
    pub fn record_fault(&mut self, pc: u32, lr: u32, uptime_ms: u32, app_id: u8) {
        self.magic = FLIGHT_MAGIC;
        self.fault_pc = pc;
        self.fault_lr = lr;
        self.uptime_ms = uptime_ms;
        self.app_id = app_id;
        self.crash_counter = self.crash_counter.saturating_add(1);
    }

    /// Update the stack watermark if current margin is lower.
    pub fn update_stack_watermark(&mut self, margin: u32) {
        if self.stack_watermark == 0 || margin < self.stack_watermark {
            self.stack_watermark = margin;
        }
    }
}
