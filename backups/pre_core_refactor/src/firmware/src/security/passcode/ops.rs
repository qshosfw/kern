//! Passcode Low-Level Keychain File Operations, Hashes & Metadata Readers.

use super::super::constants::*;
use super::super::kdf::ProgressHook;
use super::super::keychain::{PackedPasscodeKey, PackedStorageKey};
use super::state::*;
use crate::services::SystemStorage;

pub fn read_passcode_key(
    vfs: &mut SystemStorage,
    key: &mut PackedPasscodeKey,
    min_len: usize,
) -> Result<(), PasscodeError> {
    *key = PackedPasscodeKey::empty();
    let n = vfs
        .read_struct(PATH_PASSCODE_KEY, key)
        .map_err(|_| PasscodeError::StorageError)?;
    if n >= min_len && key.magic == *MAGIC_PASSCODE_KEY {
        Ok(())
    } else {
        Err(PasscodeError::StorageError)
    }
}

/// Retrieves full passcode configuration metadata from `/sys/passcode.key`
pub fn get_passcode_config(vfs: &mut SystemStorage) -> PasscodeConfigInfo {
    let mut key = PackedPasscodeKey::empty();
    if read_passcode_key(vfs, &mut key, 8).is_ok() {
        return PasscodeConfigInfo {
            is_set: (key.flags & PASSCODE_FLAG_ENABLED) != 0,
            passcode_len: key.passcode_len,
            expose_len: (key.flags & PASSCODE_FLAG_EXPOSE_LEN) != 0,
            stealth: (key.flags & PASSCODE_FLAG_STEALTH) != 0,
            digits_only: (key.flags & PASSCODE_FLAG_DIGITS_ONLY) != 0,
            max_tries: if key.max_tries == 0 {
                DEFAULT_MAX_TRIES
            } else {
                key.max_tries
            },
            failed_attempts: key.failed_attempts,
        };
    }
    PasscodeConfigInfo {
        is_set: false,
        passcode_len: 0,
        expose_len: false,
        stealth: false,
        digits_only: true,
        max_tries: DEFAULT_MAX_TRIES,
        failed_attempts: 0,
    }
}

pub fn format_wrong_passcode_msg<'a>(
    failed_attempts: u8,
    max_tries: u8,
    buf: &'a mut [u8; 32],
) -> &'a str {
    let tries_left = max_tries.saturating_sub(failed_attempts);
    let suffix = if tries_left == 1 {
        " try left"
    } else {
        " tries left"
    };
    gfx::text::format_num_msg(buf, "Wrong Passcode\n", tries_left as u32, suffix)
}

/// Updates passcode security preferences (length exposure, stealth mode, digits only, and max tries threshold)
pub fn set_passcode_options(
    vfs: &mut SystemStorage,
    expose_len: bool,
    stealth: bool,
    digits_only: bool,
    max_tries: u8,
) -> Result<(), PasscodeError> {
    let mut key = PackedPasscodeKey::empty();
    read_passcode_key(vfs, &mut key, 8)?;

    key.flags = (key.flags
        & !(PASSCODE_FLAG_EXPOSE_LEN | PASSCODE_FLAG_STEALTH | PASSCODE_FLAG_DIGITS_ONLY))
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

    key.max_tries = max_tries.clamp(3, 50);
    vfs.write_struct(PATH_PASSCODE_KEY, &key)
        .map_err(|_| PasscodeError::StorageError)?;
    Ok(())
}

/// Checks if a user passcode is currently active in the keychain.
pub fn is_passcode_set(vfs: &mut SystemStorage) -> bool {
    let mut key = PackedPasscodeKey::empty();
    read_passcode_key(vfs, &mut key, 5).map_or(false, |()| (key.flags & PASSCODE_FLAG_ENABLED) != 0)
}

fn hash_file_slice(
    vfs: &mut SystemStorage,
    path: &str,
    magic: Option<&[u8; 4]>,
    offset: usize,
    len: usize,
) -> Option<u32> {
    let mut buf = [0u8; 72];
    if let Ok(n) = vfs.read_file(path, &mut buf) {
        if n >= offset + len && magic.map_or(true, |m| n >= 4 && &buf[..4] == m) {
            return Some(fnv1a_32(&buf[offset..offset + len]));
        }
    }
    None
}

/// Returns the 32-bit FNV-1a hash of the Device Key signature in `/sys/device.key`.
pub fn get_device_key_hash(vfs: &mut SystemStorage) -> Option<u32> {
    hash_file_slice(vfs, PATH_DEVICE_KEY, Some(MAGIC_DEVICE_KEY), 20, 16)
}

/// Returns the 32-bit FNV-1a hash of the Storage Master Key.
pub fn get_master_key_hash(vfs: &mut SystemStorage, state: &PasscodeState) -> Option<u32> {
    if !state.is_locked && state.master_key.as_slice().iter().any(|&b| b != 0) {
        return Some(fnv1a_32(state.master_key.as_slice()));
    }
    hash_file_slice(vfs, PATH_STORAGE_KEY, Some(MAGIC_STORAGE_KEY), 36, 32)
}

/// Returns the 32-bit FNV-1a hash of the Passcode verifier if enabled.
pub fn get_passcode_hash(vfs: &mut SystemStorage) -> Option<u32> {
    let mut key = PackedPasscodeKey::empty();
    if read_passcode_key(vfs, &mut key, 76).is_ok() && (key.flags & PASSCODE_FLAG_ENABLED) != 0 {
        return Some(fnv1a_32(&key.passcode_verifier));
    }
    None
}

/// Returns the 32-bit FNV-1a hash of the calibration signature in `/sys/calib.sig`.
pub fn get_calib_sig_hash(vfs: &mut SystemStorage) -> Option<u32> {
    hash_file_slice(vfs, "/sys/calib.sig", None, 0, 16)
}

pub fn get_security_summary(vfs: &mut SystemStorage, state: &PasscodeState) -> SecuritySummary {
    let pcfg = get_passcode_config(vfs);
    let passcode_hash = if pcfg.is_set {
        get_passcode_hash(vfs)
    } else {
        None
    };
    SecuritySummary {
        pcfg,
        passcode_hash,
        dev_key_hash: get_device_key_hash(vfs),
        master_key_hash: get_master_key_hash(vfs, state),
        swd_open: super::super::port_lock::is_swd_enabled(),
        rdp: py32_hal::flash::InternalFlash::rdp_status(),
    }
}

/// Decrements failed attempts to (max_tries - 1) after lockout timeout expires.
pub fn reset_lockout_penalty(vfs: &mut SystemStorage) {
    let mut key = PackedPasscodeKey::empty();
    if read_passcode_key(vfs, &mut key, 8).is_ok() {
        let max_t = if key.max_tries == 0 {
            DEFAULT_MAX_TRIES
        } else {
            key.max_tries
        };
        key.failed_attempts = max_t.saturating_sub(1);
        let _ = vfs.write_struct(PATH_PASSCODE_KEY, &key);
    }
}

pub fn write_wrapped_storage_key(
    vfs: &mut SystemStorage,
    state: &PasscodeState,
    uid: &[u8; 12],
    seed: &[u8],
    salt: &[u8; 16],
    storage_nonce: &[u8; 16],
    iterations: u32,
    progress: Option<ProgressHook>,
) -> Result<(), PasscodeError> {
    let mut sto_key = PackedStorageKey {
        magic: *MAGIC_STORAGE_KEY,
        storage_nonce: *storage_nonce,
        salt: *salt,
        encrypted_mk: *state.master_key.as_slice(),
    };
    super::super::keychain::xor_derived_key(
        uid,
        seed,
        salt,
        iterations,
        &mut sto_key.encrypted_mk,
        progress,
    );

    vfs.write_struct(PATH_STORAGE_KEY, &sto_key)
        .map_err(|_| PasscodeError::StorageError)?;
    Ok(())
}
