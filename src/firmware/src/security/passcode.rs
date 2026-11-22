//! Passcode Lock Engine & Session Lifecycle Management.
//!
//! Provides envelope encryption, passcode verification, key derivation, and FNV-1a hashing.

pub mod ops;
pub mod state;

pub use ops::*;
pub use state::*;

use super::constants::*;
pub use super::kdf::ProgressHook;
use super::keychain::{PackedPasscodeKey, PackedStorageKey};
use crate::services::SystemStorage;
use crypto::buffer::SecretBuf;
use crypto::trng::HardwareTrng;

/// Initializes Passcode state on boot:
#[inline(never)]
pub fn unwrap_master_key(
    filesystem: &mut SystemStorage,
    uid: &[u8; 12],
    passcode: &[u8],
    iterations: u32,
    state: &mut PasscodeState,
    progress: Option<ProgressHook>,
) -> bool {
    let mut sto_key = PackedStorageKey::empty();
    if let Ok(sn) = filesystem.read_struct(PATH_STORAGE_KEY, &mut sto_key) {
        if sn >= core::mem::size_of::<PackedStorageKey>() && sto_key.ident == *FILE_ID_STORAGE_KEY {
            let mut mk = sto_key.encrypted_mk;
            super::keychain::xor_derived_key(
                uid,
                passcode,
                &sto_key.salt,
                iterations,
                &mut mk,
                progress,
            );
            state.master_key.as_mut_slice().copy_from_slice(&mk);
            return true;
        }
    }
    false
}

/// - If passcode is active: locks session and zeroizes master key.
/// - If passcode is inactive: derives default storage key and unlocks session.
#[inline(never)]
pub fn init_passcode_state(filesystem: &mut SystemStorage, state: &mut PasscodeState) {
    if is_passcode_set(filesystem) {
        state.lock();
    } else {
        let uid = bsp::uid::read_mcu_uid();
        if unwrap_master_key(
            filesystem,
            &uid,
            DEFAULT_PASSCODE_SEED,
            DEFAULT_PBKDF_ITERATIONS,
            state,
            None,
        ) {
            state.unlock();
        }
    }
    let _ = super::sync_filesystem_security(filesystem, state);
}

/// Verifies user passcode against `/sys/passcode.key` with optional progress reporting.
#[inline(never)]
pub fn verify_passcode(
    filesystem: &mut SystemStorage,
    passcode: &[u8],
    state: &mut PasscodeState,
    progress: Option<ProgressHook>,
) -> Result<bool, PasscodeError> {
    let mut key = PackedPasscodeKey::empty();
    read_passcode_key(
        filesystem,
        &mut key,
        core::mem::size_of::<PackedPasscodeKey>(),
    )?;

    let it = match key.iterations {
        0 | 0xFFFF_FFFF => DEFAULT_PBKDF_ITERATIONS,
        val => val.clamp(100, 50_000),
    };
    let max_tries = if key.max_tries == 0 {
        DEFAULT_MAX_TRIES
    } else {
        key.max_tries
    };

    if key.failed_attempts >= max_tries {
        return Err(PasscodeError::LockedOut);
    }

    let uid_current = bsp::uid::read_mcu_uid();
    let mut computed = SecretBuf::<32>::new();

    super::keychain::derive_passcode_kek(
        &uid_current,
        passcode,
        &key.salt,
        it,
        &mut computed,
        progress,
    );

    // Constant-time verifier comparison
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= computed.as_slice()[i] ^ key.passcode_verifier[i];
    }

    let success = diff == 0;
    if success {
        unwrap_master_key(filesystem, &uid_current, passcode, it, state, progress);
        key.failed_attempts = 0;
        state.unlock();
    } else {
        let new_fails = key.failed_attempts.saturating_add(1);
        state.tries_failed = new_fails;
        key.failed_attempts = new_fails;
    }
    let _ = filesystem.write_struct(PATH_PASSCODE_KEY, &key);
    if success {
        let _ = super::sync_filesystem_security(filesystem, state);
    }
    if let Some(ref hook) = progress {
        hook.report(100);
    }
    Ok(success)
}

/// Sets or updates passcode preferences and re-wraps storage master key with optional progress reporting.
#[inline(never)]
pub fn set_passcode(
    filesystem: &mut SystemStorage,
    state: &mut PasscodeState,
    new_passcode: &[u8],
    expose_len: bool,
    stealth: bool,
    digits_only: bool,
    max_tries: u8,
    progress: Option<ProgressHook>,
) -> Result<(), PasscodeError> {
    let is_enable = !new_passcode.is_empty();
    if is_enable && (new_passcode.len() < 3 || new_passcode.len() > 16) {
        return Err(PasscodeError::InvalidLength);
    }
    let seed = if is_enable {
        new_passcode
    } else {
        DEFAULT_PASSCODE_SEED
    };

    if let Some(ref hook) = progress {
        hook.report(5);
    }

    let uid = bsp::uid::read_mcu_uid();
    let mut trng = HardwareTrng::new();
    trng.init_with_uid(&uid, bsp::entropy::sample_hardware_noise());

    let flags = PASSCODE_FLAG_ENABLED
        | PASSCODE_FLAG_LOCK_BOOT
        | (if expose_len {
            PASSCODE_FLAG_EXPOSE_LEN
        } else {
            0
        })
        | (if stealth { PASSCODE_FLAG_STEALTH } else { 0 })
        | (if digits_only {
            PASSCODE_FLAG_DIGITS_ONLY
        } else {
            0
        });

    super::keychain::write_passcode_key(
        filesystem,
        &mut trng,
        &uid,
        new_passcode,
        flags,
        max_tries,
        progress,
    )
    .map_err(|_| PasscodeError::StorageError)?;

    super::keychain::write_storage_key(
        filesystem,
        &mut trng,
        &uid,
        seed,
        state.master_key.as_mut_slice(),
        progress,
    )
    .map_err(|_| PasscodeError::StorageError)?;

    if let Some(ref hook) = progress {
        hook.report(100);
    }

    state.unlock();
    let _ = super::sync_filesystem_security(filesystem, state);
    Ok(())
}

/// Disables passcode protection and reverts master key wrapping to default seed.
#[inline(always)]
pub fn disable_passcode(
    filesystem: &mut SystemStorage,
    state: &mut PasscodeState,
) -> Result<(), PasscodeError> {
    set_passcode(
        filesystem,
        state,
        &[],
        false,
        false,
        true,
        DEFAULT_MAX_TRIES,
        None,
    )
}
