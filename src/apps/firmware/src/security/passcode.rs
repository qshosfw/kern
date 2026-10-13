use crate::services::SystemStorage;
// Passcode Lock Engine & Session Lifecycle Management
// Provides envelope encryption, passcode verification, key derivation, and FNV-1a hashing.

use super::constants::*;
use super::keychain::{PackedPasscodeKey, PackedStorageKey, derive_passcode_kek};
use crypto::buffer::SecretBuffer;
use crypto::trng::HardwareTrng;

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
    pub master_key: SecretBuffer,
}

impl PasscodeState {
    pub const fn empty() -> Self {
        Self {
            is_locked: false,
            tries_failed: 0,
            max_tries: 0,
            master_key: SecretBuffer::new(),
        }
    }

    pub const fn new() -> Self {
        Self {
            is_locked: false,
            tries_failed: 0,
            max_tries: DEFAULT_MAX_TRIES,
            master_key: SecretBuffer::new(),
        }
    }

    /// Locks session and zeroizes master key buffer.
    pub fn lock(&mut self) {
        self.master_key.zeroize();
        self.is_locked = true;
        super::port_lock::set_swd_enabled(false);
    }

    /// Unlocks session upon successful passcode authentication.
    pub fn unlock(&mut self) {
        self.is_locked = false;
        self.tries_failed = 0;
        super::port_lock::set_swd_enabled(true);
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

fn read_passcode_key(
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
    hash_file_slice(vfs, PATH_DEVICE_KEY, Some(MAGIC_DEVICE_KEY), 16, 16)
}

/// Returns the 32-bit FNV-1a hash of the Storage Master Key.
/// Matches DeltaFW `Passcode_GetMasterKeyHash()`.
pub fn get_master_key_hash(vfs: &mut SystemStorage, state: &PasscodeState) -> Option<u32> {
    if !state.is_locked && state.master_key.as_slice().iter().any(|&b| b != 0) {
        return Some(fnv1a_32(state.master_key.as_slice()));
    }
    hash_file_slice(vfs, PATH_STORAGE_KEY, Some(MAGIC_STORAGE_KEY), 28, 32)
}

/// Returns the 32-bit FNV-1a hash of the Passcode verifier if enabled.
pub fn get_passcode_hash(vfs: &mut SystemStorage) -> Option<u32> {
    let mut key = PackedPasscodeKey::empty();
    if read_passcode_key(vfs, &mut key, 72).is_ok() && (key.flags & PASSCODE_FLAG_ENABLED) != 0 {
        return Some(fnv1a_32(&key.passcode_verifier));
    }
    None
}

/// Returns the 32-bit FNV-1a hash of the calibration signature in `/sys/calib.sig`.
pub fn get_calib_sig_hash(vfs: &mut SystemStorage) -> Option<u32> {
    hash_file_slice(vfs, "/sys/calib.sig", None, 0, 16)
}

pub struct SecuritySummary {
    pub pcfg: PasscodeConfigInfo,
    pub passcode_hash: Option<u32>,
    pub dev_key_hash: Option<u32>,
    pub master_key_hash: Option<u32>,
    pub swd_open: bool,
    pub rdp: py32_hal::flash::RdpLevel,
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
        swd_open: super::port_lock::is_swd_enabled(),
        rdp: py32_hal::flash::InternalFlash::rdp_status(),
    }
}

pub use super::kdf::ProgressHook;

/// Initializes Passcode state on boot:
pub fn unwrap_master_key(
    vfs: &mut SystemStorage,
    uid: &[u8; 12],
    passcode: &[u8],
    iterations: u32,
    state: &mut PasscodeState,
    progress: Option<ProgressHook>,
) -> bool {
    let mut sto_key = PackedStorageKey::empty();
    if let Ok(sn) = vfs.read_struct(PATH_STORAGE_KEY, &mut sto_key) {
        if sn >= core::mem::size_of::<PackedStorageKey>() && sto_key.magic == *MAGIC_STORAGE_KEY {
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
pub fn init_passcode_state(vfs: &mut SystemStorage, state: &mut PasscodeState) {
    if is_passcode_set(vfs) {
        state.lock();
    } else {
        let uid = bsp::uid::read_mcu_uid();
        let mut key = PackedPasscodeKey::empty();
        let stored_it =
            if read_passcode_key(vfs, &mut key, core::mem::size_of::<PackedPasscodeKey>()).is_ok()
                && key.iterations != 0
                && key.iterations != 0xFFFF_FFFF
            {
                key.iterations
            } else {
                DEFAULT_PBKDF_ITERATIONS
            };

        if unwrap_master_key(vfs, &uid, DEFAULT_PASSCODE_SEED, stored_it, state, None) {
            state.unlock();
            if stored_it != DEFAULT_PBKDF_ITERATIONS {
                let mut sto_key = PackedStorageKey::empty();
                if let Ok(sn) = vfs.read_struct(PATH_STORAGE_KEY, &mut sto_key) {
                    if sn >= core::mem::size_of::<PackedStorageKey>()
                        && sto_key.magic == *MAGIC_STORAGE_KEY
                    {
                        let _ = write_wrapped_storage_key(
                            vfs,
                            state,
                            &uid,
                            DEFAULT_PASSCODE_SEED,
                            &sto_key.salt,
                            &sto_key.storage_nonce,
                            DEFAULT_PBKDF_ITERATIONS,
                            None,
                        );
                    }
                }
                key.iterations = DEFAULT_PBKDF_ITERATIONS;
                let mut default_verifier = SecretBuffer::new();
                derive_passcode_kek(
                    &uid,
                    DEFAULT_PASSCODE_SEED,
                    &key.salt,
                    DEFAULT_PBKDF_ITERATIONS,
                    &mut default_verifier,
                    None,
                );
                key.passcode_verifier = *default_verifier.as_slice();
                key.mac = super::keychain::compute_hw_mac(
                    &uid,
                    &key.salt,
                    MAGIC_PASSCODE_KEY,
                    &key.passcode_verifier,
                );
                let _ = vfs.write_struct(PATH_PASSCODE_KEY, &key);
            }
        } else if stored_it != DEFAULT_PBKDF_ITERATIONS {
            if unwrap_master_key(
                vfs,
                &uid,
                DEFAULT_PASSCODE_SEED,
                DEFAULT_PBKDF_ITERATIONS,
                state,
                None,
            ) {
                state.unlock();
            }
        }
    }
}

/// Decrements failed attempts to (max_tries - 1) after lockout timeout expires.
/// Saves updated record to `/sys/passcode.key` so on reboot the timeout cannot be bypassed.
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

fn progress_phase1(ctx: *mut (), pct: u8) {
    let hook = unsafe { *(ctx as *const ProgressHook) };
    hook.report(pct >> 1);
}

fn progress_phase2(ctx: *mut (), pct: u8) {
    let hook = unsafe { *(ctx as *const ProgressHook) };
    hook.report(50 + (pct >> 1));
}

/// Verifies user passcode against `/sys/passcode.key` with optional progress reporting.
pub fn verify_passcode(
    vfs: &mut SystemStorage,
    passcode: &[u8],
    state: &mut PasscodeState,
    progress: Option<ProgressHook>,
) -> Result<bool, PasscodeError> {
    let mut key = PackedPasscodeKey::empty();
    read_passcode_key(vfs, &mut key, core::mem::size_of::<PackedPasscodeKey>())?;

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
    let mut computed = SecretBuffer::new();

    let hook_copy = progress;
    let (p_cb1, p_cb2) = if let Some(ref h) = hook_copy {
        let ptr = h as *const ProgressHook as *mut ();
        (
            Some(ProgressHook::new(ptr, progress_phase1)),
            Some(ProgressHook::new(ptr, progress_phase2)),
        )
    } else {
        (None, None)
    };

    super::keychain::derive_passcode_kek(
        &uid_current,
        passcode,
        &key.salt,
        it,
        &mut computed,
        p_cb1,
    );

    // Constant-time verifier comparison
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= computed.as_slice()[i] ^ key.passcode_verifier[i];
    }

    let success = diff == 0;
    if success {
        // Unwrap Master Key from storage.key with progress reporting
        unwrap_master_key(vfs, &uid_current, passcode, it, state, p_cb2);
        key.failed_attempts = 0;
        state.unlock();

        // Auto-migration: If key was wrapped with legacy iterations, re-wrap at DEFAULT_PBKDF_ITERATIONS
        if key.iterations != DEFAULT_PBKDF_ITERATIONS {
            let mut new_verifier = SecretBuffer::new();
            super::keychain::derive_passcode_kek(
                &uid_current,
                passcode,
                &key.salt,
                DEFAULT_PBKDF_ITERATIONS,
                &mut new_verifier,
                None,
            );
            key.passcode_verifier = *new_verifier.as_slice();
            key.iterations = DEFAULT_PBKDF_ITERATIONS;
            key.mac = super::keychain::compute_hw_mac(
                &uid_current,
                &key.salt,
                MAGIC_PASSCODE_KEY,
                &key.passcode_verifier,
            );

            let mut sto_key = PackedStorageKey::empty();
            if let Ok(sn) = vfs.read_struct(PATH_STORAGE_KEY, &mut sto_key) {
                if sn >= core::mem::size_of::<PackedStorageKey>()
                    && sto_key.magic == *MAGIC_STORAGE_KEY
                {
                    let _ = write_wrapped_storage_key(
                        vfs,
                        state,
                        &uid_current,
                        passcode,
                        &sto_key.salt,
                        &sto_key.storage_nonce,
                        DEFAULT_PBKDF_ITERATIONS,
                        None,
                    );
                }
            }
        }
    } else {
        let new_fails = key.failed_attempts.saturating_add(1);
        state.tries_failed = new_fails;
        key.failed_attempts = new_fails;
    }
    let _ = vfs.write_struct(PATH_PASSCODE_KEY, &key);
    if let Some(ref hook) = progress {
        hook.report(100);
    }
    Ok(success)
}

/// Sets or updates passcode preferences and re-wraps storage master key with optional progress reporting.
pub fn set_passcode(
    vfs: &mut SystemStorage,
    state: &mut PasscodeState,
    new_passcode: &[u8],
    expose_len: bool,
    stealth: bool,
    digits_only: bool,
    max_tries: u8,
    progress: Option<ProgressHook>,
) -> Result<(), PasscodeError> {
    if new_passcode.is_empty() {
        return disable_passcode(vfs, state);
    }
    if new_passcode.len() < 3 || new_passcode.len() > 16 {
        return Err(PasscodeError::InvalidLength);
    }

    if let Some(ref hook) = progress {
        hook.report(5);
    }

    let uid = bsp::uid::read_mcu_uid();
    let hw_us = py32_hal::time::now_us();
    let mut trng = HardwareTrng::new();
    trng.init_with_uid(&uid, hw_us);

    // If master key is empty (e.g. initial setup), generate a new random master key
    if state.master_key.as_slice().iter().all(|&b| b == 0) {
        trng.fill_bytes(state.master_key.as_mut_slice(), hw_us.wrapping_add(10));
    }

    let mut rand_buf = [0u8; 36];
    trng.fill_bytes(&mut rand_buf, hw_us.wrapping_add(11));
    let passcode_salt: &[u8; 12] = unsafe { &*(rand_buf.as_ptr() as *const [u8; 12]) };
    let storage_salt: &[u8; 12] = unsafe { &*(rand_buf.as_ptr().add(12) as *const [u8; 12]) };
    let storage_nonce: &[u8; 12] = unsafe { &*(rand_buf.as_ptr().add(24) as *const [u8; 12]) };

    if let Some(ref hook) = progress {
        hook.report(15);
    }

    let mut passcode_verifier = SecretBuffer::new();
    super::keychain::derive_passcode_kek(
        &uid,
        new_passcode,
        passcode_salt,
        DEFAULT_PBKDF_ITERATIONS,
        &mut passcode_verifier,
        progress,
    );

    let mac = super::keychain::compute_hw_mac(
        &uid,
        passcode_salt,
        MAGIC_PASSCODE_KEY,
        passcode_verifier.as_slice(),
    );

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

    let passcode_key = PackedPasscodeKey {
        magic: *MAGIC_PASSCODE_KEY,
        flags,
        failed_attempts: 0,
        passcode_len: new_passcode.len() as u8,
        max_tries: max_tries.clamp(3, 50),
        iterations: DEFAULT_PBKDF_ITERATIONS,
        salt: *passcode_salt,
        passcode_verifier: *passcode_verifier.as_slice(),
        mac,
    };
    vfs.write_struct(PATH_PASSCODE_KEY, &passcode_key)
        .map_err(|_| PasscodeError::StorageError)?;

    if let Some(ref hook) = progress {
        hook.report(60);
    }

    write_wrapped_storage_key(
        vfs,
        state,
        &uid,
        new_passcode,
        storage_salt,
        storage_nonce,
        DEFAULT_PBKDF_ITERATIONS,
        progress,
    )?;

    if let Some(ref hook) = progress {
        hook.report(100);
    }

    state.unlock();
    Ok(())
}

fn write_wrapped_storage_key(
    vfs: &mut SystemStorage,
    state: &PasscodeState,
    uid: &[u8; 12],
    seed: &[u8],
    salt: &[u8; 12],
    storage_nonce: &[u8; 12],
    iterations: u32,
    progress: Option<ProgressHook>,
) -> Result<(), PasscodeError> {
    let mut sto_key = PackedStorageKey {
        magic: *MAGIC_STORAGE_KEY,
        storage_nonce: *storage_nonce,
        salt: *salt,
        encrypted_mk: *state.master_key.as_slice(),
    };
    super::keychain::xor_derived_key(
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

/// Disables passcode protection and reverts master key wrapping to default seed.
pub fn disable_passcode(
    vfs: &mut SystemStorage,
    state: &mut PasscodeState,
) -> Result<(), PasscodeError> {
    let uid = bsp::uid::read_mcu_uid();
    let hw_us = py32_hal::time::now_us();
    let mut trng = HardwareTrng::new();
    trng.init_with_uid(&uid, hw_us);

    let mut rand_buf = [0u8; 36];
    trng.fill_bytes(&mut rand_buf, hw_us.wrapping_add(20));
    let storage_salt: &[u8; 12] = unsafe { &*(rand_buf.as_ptr() as *const [u8; 12]) };
    let storage_nonce: &[u8; 12] = unsafe { &*(rand_buf.as_ptr().add(12) as *const [u8; 12]) };
    let passcode_salt: &[u8; 12] = unsafe { &*(rand_buf.as_ptr().add(24) as *const [u8; 12]) };

    write_wrapped_storage_key(
        vfs,
        state,
        &uid,
        DEFAULT_PASSCODE_SEED,
        storage_salt,
        storage_nonce,
        DEFAULT_PBKDF_ITERATIONS,
        None,
    )?;

    // Disable flags in passcode.key
    let mut default_verifier = SecretBuffer::new();
    derive_passcode_kek(
        &uid,
        DEFAULT_PASSCODE_SEED,
        passcode_salt,
        DEFAULT_PBKDF_ITERATIONS,
        &mut default_verifier,
        None,
    );

    let passcode_key = PackedPasscodeKey {
        magic: *MAGIC_PASSCODE_KEY,
        flags: 0,
        failed_attempts: 0,
        passcode_len: 0,
        max_tries: DEFAULT_MAX_TRIES,
        iterations: DEFAULT_PBKDF_ITERATIONS,
        salt: *passcode_salt,
        passcode_verifier: *default_verifier.as_slice(),
        mac: [0; 16],
    };
    vfs.write_struct(PATH_PASSCODE_KEY, &passcode_key)
        .map_err(|_| PasscodeError::StorageError)?;

    state.unlock();
    Ok(())
}
