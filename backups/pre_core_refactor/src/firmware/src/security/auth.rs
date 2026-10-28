use crate::services::SystemStorage;
// Secure Authentication & Trust API
// Capability-based authorization tokens and trust scope enforcement.

use super::passcode::{PasscodeError, PasscodeState, get_passcode_config, verify_passcode};

/// Trust scopes for operations requiring privileged authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustScope {
    /// Modifying core system settings
    SystemConfig,
    /// Configuring or toggling passcode, lockout limits, port locking
    SecuritySettings,
    /// Wiping filesystem, NVRAM, or factory defaults
    FactoryReset,
    /// Reading or writing cryptographic keys and certificates
    KeyManagement,
    /// Modifying hardware calibration tables
    Calibration,
    /// Firmware update / bootloader operations
    FirmwareUpdate,
}

/// A zero-sized, capability-based authorization token.
/// Cannot be constructed outside the security subsystem.
#[derive(Debug)]
pub struct AuthToken {
    scope: TrustScope,
    _private: (),
}

impl AuthToken {
    #[inline(always)]
    pub(crate) fn new(scope: TrustScope) -> Self {
        Self {
            scope,
            _private: (),
        }
    }

    #[inline(always)]
    pub fn scope(&self) -> TrustScope {
        self.scope
    }
}

/// Evaluates whether the current session is authorized for a given trust scope.
/// If passcode protection is not configured on this device, access is automatically granted.
pub fn check_authorization(
    state: &PasscodeState,
    vfs: &mut SystemStorage,
    scope: TrustScope,
) -> Result<AuthToken, PasscodeError> {
    let cfg = get_passcode_config(vfs);
    if !cfg.is_set || !state.is_locked {
        Ok(AuthToken::new(scope))
    } else {
        if cfg.failed_attempts >= cfg.max_tries {
            Err(PasscodeError::LockedOut)
        } else {
            Err(PasscodeError::WrongPasscode)
        }
    }
}

/// Authenticates against stored credentials using the provided passcode.
/// Returns an AuthToken capability on success.
pub fn authenticate(
    state: &mut PasscodeState,
    vfs: &mut SystemStorage,
    passcode: &[u8],
    scope: TrustScope,
) -> Result<AuthToken, PasscodeError> {
    let cfg = get_passcode_config(vfs);
    if !cfg.is_set {
        return Ok(AuthToken::new(scope));
    }

    let ok = verify_passcode(vfs, passcode, state, None)?;
    if ok {
        Ok(AuthToken::new(scope))
    } else {
        Err(PasscodeError::WrongPasscode)
    }
}
