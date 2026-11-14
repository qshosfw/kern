//! Passcode Security State, Enums, and FNV-1a Hashing.

use super::super::constants::*;
use crypto::buffer::SecretBuf;

pub const PASSCODE_FLAG_ENABLED: u8 = 1 << 0;
pub const PASSCODE_FLAG_LOCK_BOOT: u8 = 1 << 1;
pub const PASSCODE_FLAG_EXPOSE_LEN: u8 = 1 << 2;
pub const PASSCODE_FLAG_STEALTH: u8 = 1 << 3;
pub const PASSCODE_FLAG_DIGITS_ONLY: u8 = 1 << 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasscodeError {
    InvalidLength,
    WrongPasscode,
    LockedOut,
    StorageError,
}

pub struct PasscodeState {
    pub is_locked: bool,
    pub tries_failed: u8,
    pub max_tries: u8,
    pub master_key: SecretBuf<32>,
}

impl PasscodeState {
    pub const fn empty() -> Self {
        Self {
            is_locked: false,
            tries_failed: 0,
            max_tries: 0,
            master_key: SecretBuf::new(),
        }
    }

    pub const fn new() -> Self {
        Self {
            is_locked: false,
            tries_failed: 0,
            max_tries: DEFAULT_MAX_TRIES,
            master_key: SecretBuf::new(),
        }
    }

    /// Locks session and zeroizes master key buffer.
    pub fn lock(&mut self) {
        self.master_key.zeroize();
        self.is_locked = true;
        super::super::port_lock::set_swd_enabled(false);
    }

    /// Unlocks session upon successful passcode authentication.
    pub fn unlock(&mut self) {
        self.is_locked = false;
        self.tries_failed = 0;
        super::super::port_lock::set_swd_enabled(true);
    }
}

/// FNV-1a 32-bit non-cryptographic hash (matching DeltaFW `Passcode_GetMasterKeyHash`)
pub fn fnv1a_32(data: &[u8]) -> u32 {
    let mut hash = 0x811C9DC5u32;
    for &b in data {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

#[derive(Debug, Clone, Copy)]
pub struct PasscodeConfigInfo {
    pub is_set: bool,
    pub passcode_len: u8,
    pub expose_len: bool,
    pub stealth: bool,
    pub digits_only: bool,
    pub max_tries: u8,
    pub failed_attempts: u8,
}

pub struct SecuritySummary {
    pub pcfg: PasscodeConfigInfo,
    pub passcode_hash: Option<u32>,
    pub dev_key_hash: Option<u32>,
    pub master_key_hash: Option<u32>,
    pub swd_open: bool,
    pub rdp: py32_hal::flash::RdpLevel,
}
