//! Microkernel Priority Classes.

/// Scheduling Priority Band.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Background telemetry, housekeeping, diagnostics
    Idle = 0,
    /// User interface and rendering tasks
    Ui = 1,
    /// Radio state machine, squelch, tone detection
    Radio = 2,
    /// Packet capture, high-speed audio, DSP buffers
    Capture = 3,
    /// Emergency power cutoff, thermal/battery safety
    Safety = 4,
}

impl Priority {
    /// Convert a raw byte priority value back into a `Priority` variant.
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if val <= 4 {
            Some(unsafe { core::mem::transmute::<u8, Self>(val) })
        } else {
            None
        }
    }
}
