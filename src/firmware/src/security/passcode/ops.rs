//! Passcode Low-Level Keychain File Operations, Hashes & Metadata Readers.

use super::super::constants::*;
use super::super::keychain::PackedPasscodeKey;
use super::state::*;
use crate::services::SystemStorage;

#[inline(never)]
pub fn read_passcode_key(
    filesystem: &mut SystemStorage,
    key: &mut PackedPasscodeKey,
    min_len: usize,
) -> Result<(), PasscodeError> {
    *key = PackedPasscodeKey::empty();
    let n = filesystem
        .read_struct(PATH_PASSCODE_KEY, key)
        .map_err(|_| PasscodeError::StorageError)?;
    if n >= min_len && key.ident == *FILE_ID_PASSCODE_KEY {
        Ok(())
    } else {
        Err(PasscodeError::StorageError)
    }
}

/// Retrieves full passcode configuration metadata from `/sys/passcode.key`
#[inline(never)]
pub fn get_passcode_config(filesystem: &mut SystemStorage) -> PasscodeConfigInfo {
    let mut info = PasscodeConfigInfo {
        is_set: false,
        passcode_len: 0,
        expose_len: false,
        stealth: false,
        digits_only: true,
        max_tries: DEFAULT_MAX_TRIES,
        failed_attempts: 0,
    };
    let mut key = PackedPasscodeKey::empty();
    if read_passcode_key(filesystem, &mut key, 8).is_ok() {
        info.is_set = (key.flags & PASSCODE_FLAG_ENABLED) != 0;
        info.passcode_len = key.passcode_len;
        info.expose_len = (key.flags & PASSCODE_FLAG_EXPOSE_LEN) != 0;
        info.stealth = (key.flags & PASSCODE_FLAG_STEALTH) != 0;
        info.digits_only = (key.flags & PASSCODE_FLAG_DIGITS_ONLY) != 0;
        if key.max_tries != 0 {
            info.max_tries = key.max_tries;
        }
        info.failed_attempts = key.failed_attempts;
    }
    info
}

pub fn format_wrong_passcode_msg<'a>(
    failed_attempts: u8,
    max_tries: u8,
    buf: &'a mut [u8; 32],
) -> &'a str {
    let tries_left = max_tries.saturating_sub(failed_attempts);
    gfx::text::format_num_msg(buf, "Wrong PIN\n", tries_left as u32, " left")
}

/// Updates passcode security preferences (length exposure, stealth mode, digits only, and max tries threshold)
#[inline(never)]
pub fn set_passcode_options(
    filesystem: &mut SystemStorage,
    expose_len: bool,
    stealth: bool,
    digits_only: bool,
    max_tries: u8,
) -> Result<(), PasscodeError> {
    let mut key = PackedPasscodeKey::empty();
    read_passcode_key(filesystem, &mut key, 8)?;

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
    filesystem
        .write_struct(PATH_PASSCODE_KEY, &key)
        .map_err(|_| PasscodeError::StorageError)?;
    Ok(())
}

/// Checks if a user passcode is currently active in the keychain.
#[inline(always)]
pub fn is_passcode_set(filesystem: &mut SystemStorage) -> bool {
    get_passcode_config(filesystem).is_set
}

fn hash_file_slice(
    filesystem: &mut SystemStorage,
    path: &str,
    ident: Option<&[u8; 4]>,
    offset: usize,
    len: usize,
) -> Option<u32> {
    let mut buf = [0u8; 72];
    if let Ok(n) = filesystem.read_file(path, &mut buf) {
        if n >= offset + len && ident.map_or(true, |id| n >= 4 && &buf[..4] == id) {
            return Some(fnv1a_32(&buf[offset..offset + len]));
        }
    }
    None
}

/// Returns the 32-bit FNV-1a hash of the Device Key signature in `/sys/device.key`.
pub fn get_device_key_hash(filesystem: &mut SystemStorage) -> Option<u32> {
    hash_file_slice(
        filesystem,
        PATH_DEVICE_KEY,
        Some(FILE_ID_DEVICE_KEY),
        20,
        16,
    )
}

/// Returns the 32-bit FNV-1a hash of the Storage Master Key.
pub fn get_master_key_hash(filesystem: &mut SystemStorage, state: &PasscodeState) -> Option<u32> {
    if !state.is_locked && state.master_key.as_slice().iter().any(|&b| b != 0) {
        return Some(fnv1a_32(state.master_key.as_slice()));
    }
    hash_file_slice(
        filesystem,
        PATH_STORAGE_KEY,
        Some(FILE_ID_STORAGE_KEY),
        36,
        32,
    )
}

/// Returns the 32-bit FNV-1a hash of the Passcode verifier if enabled.
pub fn get_passcode_hash(filesystem: &mut SystemStorage) -> Option<u32> {
    let mut key = PackedPasscodeKey::empty();
    if read_passcode_key(filesystem, &mut key, 76).is_ok()
        && (key.flags & PASSCODE_FLAG_ENABLED) != 0
    {
        return Some(fnv1a_32(&key.passcode_verifier));
    }
    None
}

/// Returns the 32-bit FNV-1a hash of the calibration signature in `/sys/calib.sig`.
pub fn get_calib_sig_hash(filesystem: &mut SystemStorage) -> Option<u32> {
    hash_file_slice(filesystem, "/sys/calib.sig", None, 0, 16)
}

pub fn get_security_summary(
    filesystem: &mut SystemStorage,
    state: &PasscodeState,
) -> SecuritySummary {
    let pcfg = get_passcode_config(filesystem);
    let passcode_hash = if pcfg.is_set {
        get_passcode_hash(filesystem)
    } else {
        None
    };
    SecuritySummary {
        pcfg,
        passcode_hash,
        dev_key_hash: get_device_key_hash(filesystem),
        master_key_hash: get_master_key_hash(filesystem, state),
        swd_open: super::super::port_lock::is_swd_enabled(),
        rdp: py32_hal::flash::InternalFlash::rdp_status(),
    }
}

/// Decrements failed attempts to (max_tries - 1) after lockout timeout expires.
pub fn reset_lockout_penalty(filesystem: &mut SystemStorage) {
    let mut key = PackedPasscodeKey::empty();
    if read_passcode_key(filesystem, &mut key, 8).is_ok() {
        let max_t = if key.max_tries == 0 {
            DEFAULT_MAX_TRIES
        } else {
            key.max_tries
        };
        key.failed_attempts = max_t.saturating_sub(1);
        let _ = filesystem.write_struct(PATH_PASSCODE_KEY, &key);
    }
}
