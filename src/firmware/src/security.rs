//! KERN Firmware Security Subsystem

pub mod attributes;
#[cfg(feature = "passcode")]
pub mod auth;
#[cfg(feature = "passcode")]
pub mod constants;
pub mod kdf;
#[cfg(feature = "passcode")]
pub mod keychain;
#[cfg(feature = "passcode")]
pub mod passcode;
pub mod port_lock;

pub use attributes::FileSecurityLevel;
#[cfg(feature = "passcode")]
pub use auth::{AuthToken, TrustScope, authenticate, check_authorization};
pub use kdf::ProgressHook;
#[cfg(feature = "passcode")]
pub use keychain::{
    PackedDeviceKey, PackedPasscodeKey, PackedStorageKey, derive_passcode_kek, ensure_keychain,
};
#[cfg(feature = "passcode")]
pub use passcode::PasscodeState;
pub use port_lock::set_swd_enabled;

#[cfg(not(feature = "passcode"))]
pub mod passcode {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PasscodeError {
        InvalidLength,
        WrongPasscode,
        LockedOut,
        StorageError,
    }

    pub struct PasscodeState {
        pub is_locked: bool,
    }

    impl PasscodeState {
        pub const fn empty() -> Self {
            Self { is_locked: false }
        }

        pub const fn new() -> Self {
            Self { is_locked: false }
        }

        #[inline(always)]
        pub fn lock(&mut self) {
            self.is_locked = true;
        }

        #[inline(always)]
        pub fn unlock(&mut self) {
            self.is_locked = false;
        }
    }

    #[inline(always)]
    pub fn is_passcode_set(filesystem: &mut crate::services::SystemStorage) -> bool {
        let mut hdr = [0u8; 6];
        if let Ok(n) = filesystem.read_file("/sys/passcode.key", &mut hdr) {
            return n >= 6 && &hdr[..4] == b"PIN1" && (hdr[4] & 1) != 0;
        }
        false
    }

    #[inline(always)]
    pub fn init_passcode_state(
        _filesystem: &mut crate::services::SystemStorage,
        state: &mut PasscodeState,
    ) {
        state.unlock();
    }
}

#[cfg(not(feature = "passcode"))]
pub use passcode::PasscodeState;

#[cfg(not(feature = "passcode"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustScope {
    SystemConfig,
    SecuritySettings,
    FactoryReset,
    KeyManagement,
    Calibration,
    FirmwareUpdate,
}

#[cfg(not(feature = "passcode"))]
#[derive(Debug)]
pub struct AuthToken {
    scope: TrustScope,
}

#[cfg(not(feature = "passcode"))]
impl AuthToken {
    #[inline(always)]
    pub(crate) fn new(scope: TrustScope) -> Self {
        Self { scope }
    }

    #[inline(always)]
    pub fn scope(&self) -> TrustScope {
        self.scope
    }
}

#[cfg(not(feature = "passcode"))]
#[inline(always)]
pub fn check_authorization(
    _state: &PasscodeState,
    _filesystem: &mut crate::services::SystemStorage,
    scope: TrustScope,
) -> Result<AuthToken, passcode::PasscodeError> {
    Ok(AuthToken::new(scope))
}

#[cfg(not(feature = "passcode"))]
#[inline(always)]
pub fn authenticate(
    _state: &mut PasscodeState,
    _filesystem: &mut crate::services::SystemStorage,
    _passcode: &[u8],
    scope: TrustScope,
) -> Result<AuthToken, passcode::PasscodeError> {
    Ok(AuthToken::new(scope))
}

#[cfg(not(feature = "passcode"))]
#[inline(always)]
pub fn ensure_keychain(_filesystem: &mut crate::services::SystemStorage) {}
