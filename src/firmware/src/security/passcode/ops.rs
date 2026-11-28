//! Passcode Low-Level Keychain File Operations, Hashes & Metadata Readers.

use super::super::constants::*;
use super::super::keychain::PackedPasscodeKey;
use super::state::*;
use crate::services::SystemStorage;

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

/// Updates passcode security preferences (`opt_flags` holds `PASSCODE_FLAG_{EXPOSE_LEN|STEALTH|DIGITS_ONLY}`).
pub fn set_passcode_options(
    filesystem: &mut SystemStorage,
    opt_flags: u8,
    max_tries: u8,
) -> Result<(), PasscodeError> {
    let mut key = PackedPasscodeKey::empty();
    read_passcode_key(filesystem, &mut key, 8)?;

    const OPT_MASK: u8 =
        PASSCODE_FLAG_EXPOSE_LEN | PASSCODE_FLAG_STEALTH | PASSCODE_FLAG_DIGITS_ONLY;
    key.flags = (key.flags & !OPT_MASK) | (opt_flags & OPT_MASK);
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

/// Decrements failed attempts to (max_tries - 1) after lockout timeout expires.
pub fn reset_lockout_penalty(filesystem: &mut SystemStorage, max_tries: u8) {
    let mut key = PackedPasscodeKey::empty();
    if read_passcode_key(filesystem, &mut key, 8).is_ok() {
        key.failed_attempts = max_tries.saturating_sub(1);
        let _ = filesystem.write_struct(PATH_PASSCODE_KEY, &key);
    }
}
