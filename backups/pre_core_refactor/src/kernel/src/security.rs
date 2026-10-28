//! Core Security Definitions and Lockdown State Machine
//!
//! Provides multi-byte glitch-resistant state constants and the Passcode Metadata definition.

/// Multi-byte magic constants for Glitch-Resistant Lockdown State Machine
pub const STATE_BOOT_INIT: u32 = 0x1122_3344; // Initial boot, verifying flash
pub const STATE_UNLOCKED: u32 = 0x5AA5_A55A; // User authenticated, full access
pub const STATE_SOFT_LOCK: u32 = 0xDEAD_BEEF; // Screen locked, keys in RAM but UI restricted
pub const STATE_HARD_LOCK: u32 = 0x8BAD_F00D; // Keys zeroized, requires PIN
pub const STATE_TAMPER_HALT: u32 = 0x0000_0000; // Hardware mismatch, system halted

pub struct SecurityContext {
    state: u32,
}

impl SecurityContext {
    pub const fn new() -> Self {
        Self {
            state: STATE_BOOT_INIT,
        }
    }

    #[inline(always)]
    pub fn get_state(&self) -> u32 {
        // Double read to prevent single-cycle voltage glitches
        let s1 = unsafe { core::ptr::read_volatile(&self.state as *const u32) };
        let s2 = unsafe { core::ptr::read_volatile(&self.state as *const u32) };
        if s1 == s2 {
            s1
        } else {
            STATE_TAMPER_HALT // Glitch detected!
        }
    }

    pub fn transition(&mut self, new_state: u32) {
        unsafe {
            core::ptr::write_volatile(&mut self.state as *mut u32, new_state);
        }
    }
}

/// Hardware debug isolation state (SWD pins and Flash Readout Protection)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardwareSecurityState {
    /// Fully locked down: SWD port pins disabled AND Flash RDP Level 1 active.
    FullyLocked,
    /// Software isolated: SWD port pins disabled, but Flash RDP Level 0 (open).
    PinsIsolated,
    /// RDP protected: Flash RDP Level 1 active, but SWD pins in AF mode.
    RdpProtectedOnly,
    /// Unprotected: SWD pins active and Flash RDP Level 0.
    Unprotected,
}

impl HardwareSecurityState {
    #[inline(always)]
    pub const fn is_secure(&self) -> bool {
        matches!(
            self,
            HardwareSecurityState::FullyLocked | HardwareSecurityState::RdpProtectedOnly
        )
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            HardwareSecurityState::FullyLocked => "Fully Locked",
            HardwareSecurityState::PinsIsolated => "Pins Isolated",
            HardwareSecurityState::RdpProtectedOnly => "RDP Protected",
            HardwareSecurityState::Unprotected => "Unprotected",
        }
    }
}

/// Evaluates combined hardware security status from SWD pin mode and Flash RDP state.
#[inline(always)]
pub fn evaluate_hardware_security(swd_open: bool, rdp_protected: bool) -> HardwareSecurityState {
    match (swd_open, rdp_protected) {
        (false, true) => HardwareSecurityState::FullyLocked,
        (false, false) => HardwareSecurityState::PinsIsolated,
        (true, true) => HardwareSecurityState::RdpProtectedOnly,
        (true, false) => HardwareSecurityState::Unprotected,
    }
}
