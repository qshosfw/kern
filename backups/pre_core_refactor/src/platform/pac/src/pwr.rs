//! Power Controller (PWR) registers for PY32F071.

pub const PWR_BASE: u32 = 0x4000_7000;

pub const PWR_CR: *mut u32 = PWR_BASE as *mut u32;
pub const PWR_SR: *const u32 = (PWR_BASE + 0x04) as *const u32;

// ── PWR_CR Bits ─────────────────────────────────────────────────────────
pub const PWR_CR_LPDS: u32 = 1 << 0; // Low-power deep sleep
pub const PWR_CR_PDDS: u32 = 1 << 1; // Power down deep sleep (Standby)
pub const PWR_CR_CWUF: u32 = 1 << 2; // Clear wakeup flag
pub const PWR_CR_CSBF: u32 = 1 << 3; // Clear standby flag
pub const PWR_CR_PVDE: u32 = 1 << 4; // PVD enable
pub const PWR_CR_PLS_SHIFT: u32 = 5; // PVD level select (bits 7:5)
