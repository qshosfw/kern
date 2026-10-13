use crate::services::SystemStorage;
// Dynamic Hardware Silicon Key Pipeline & Security Keychain Containers

use crypto::buffer::SecretBuffer;
use crypto::chacha20poly1305::ChaCha20Poly1305;
use crypto::trng::HardwareTrng;

use super::constants::*;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PackedDeviceKey {
    pub magic: [u8; 4],
    pub hardware_nonce: [u8; 12],
    pub device_signature: [u8; 16],
}

impl PackedDeviceKey {
    pub const fn empty() -> Self {
        Self {
            magic: [0; 4],
            hardware_nonce: [0; 12],
            device_signature: [0; 16],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PackedPasscodeKey {
    pub magic: [u8; 4],
    pub flags: u8,
    pub failed_attempts: u8,
    pub passcode_len: u8,
    pub max_tries: u8,
    pub iterations: u32,
    pub salt: [u8; 12],
    pub passcode_verifier: [u8; 32],
    pub mac: [u8; 16],
}

impl PackedPasscodeKey {
    pub const fn empty() -> Self {
        Self {
            magic: [0; 4],
            flags: 0,
            failed_attempts: 0,
            passcode_len: 0,
            max_tries: 0,
            iterations: 0,
            salt: [0; 12],
            passcode_verifier: [0; 32],
            mac: [0; 16],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PackedStorageKey {
    pub magic: [u8; 4],
    pub storage_nonce: [u8; 12],
    pub salt: [u8; 12],
    pub encrypted_mk: [u8; 32],
}

impl PackedStorageKey {
    pub const fn empty() -> Self {
        Self {
            magic: [0; 4],
            storage_nonce: [0; 12],
            salt: [0; 12],
            encrypted_mk: [0; 32],
        }
    }
}

pub(crate) fn compute_hw_mac(
    uid: &[u8; 12],
    nonce: &[u8; 12],
    magic: &[u8; 4],
    data: &[u8],
) -> [u8; 16] {
    let mut hardware_kek = SecretBuffer::new();
    derive_passcode_kek(uid, b"HW_KEK", nonce, 100, &mut hardware_kek, None);
    let cipher = ChaCha20Poly1305::new(*hardware_kek.as_slice());
    cipher.compute_mac(nonce, magic, data)
}

fn key_valid(vfs: &mut SystemStorage, path: &str, magic: &[u8; 4], expected_len: usize) -> bool {
    let mut header = [0u8; 4];
    vfs.file_size(path) == Ok(expected_len)
        && vfs.read_file(path, &mut header).is_ok()
        && &header == magic
}

fn init_device_key(vfs: &mut SystemStorage, trng: &mut HardwareTrng, uid: &[u8; 12], hw_us: u32) {
    let mut dev_key = PackedDeviceKey::empty();
    dev_key.magic = *MAGIC_DEVICE_KEY;
    trng.fill_bytes(&mut dev_key.hardware_nonce, hw_us.wrapping_add(1));
    dev_key.device_signature = compute_hw_mac(uid, &dev_key.hardware_nonce, MAGIC_DEVICE_KEY, uid);
    let _ = vfs.write_struct(KEY_PATHS[0], &dev_key);
}

fn init_passcode_key(vfs: &mut SystemStorage, trng: &mut HardwareTrng, uid: &[u8; 12], hw_us: u32) {
    let mut passcode_key = PackedPasscodeKey::empty();
    passcode_key.magic = *MAGIC_PASSCODE_KEY;
    passcode_key.max_tries = DEFAULT_MAX_TRIES;
    passcode_key.iterations = DEFAULT_PBKDF_ITERATIONS;
    trng.fill_bytes(&mut passcode_key.salt, hw_us.wrapping_add(2));

    let mut passcode_verifier = SecretBuffer::new();
    derive_passcode_kek(
        uid,
        DEFAULT_PASSCODE_SEED,
        &passcode_key.salt,
        DEFAULT_PBKDF_ITERATIONS,
        &mut passcode_verifier,
        None,
    );
    passcode_key.passcode_verifier = *passcode_verifier.as_slice();
    passcode_key.mac = compute_hw_mac(
        uid,
        &passcode_key.salt,
        MAGIC_PASSCODE_KEY,
        &passcode_key.passcode_verifier,
    );

    let _ = vfs.write_struct(KEY_PATHS[1], &passcode_key);
}

fn init_storage_key(vfs: &mut SystemStorage, trng: &mut HardwareTrng, uid: &[u8; 12], hw_us: u32) {
    let mut storage_key = PackedStorageKey::empty();
    storage_key.magic = *MAGIC_STORAGE_KEY;
    let s_bytes = unsafe {
        core::slice::from_raw_parts_mut(&mut storage_key.storage_nonce as *mut _ as *mut u8, 56)
    };
    trng.fill_bytes(s_bytes, hw_us.wrapping_add(3));
    xor_derived_key(
        uid,
        DEFAULT_PASSCODE_SEED,
        &storage_key.salt,
        DEFAULT_PBKDF_ITERATIONS,
        &mut storage_key.encrypted_mk,
        None,
    );

    let _ = vfs.write_struct(KEY_PATHS[2], &storage_key);
}

pub fn ensure_keychain(vfs: &mut SystemStorage) {
    let uid = bsp::uid::read_mcu_uid();
    let hw_us = py32_hal::time::now_us();
    let mut trng = HardwareTrng::new();
    trng.init_with_uid(&uid, hw_us);

    // 1. `/sys/device.key`
    let dev_valid = key_valid(
        vfs,
        KEY_PATHS[0],
        MAGIC_DEVICE_KEY,
        core::mem::size_of::<PackedDeviceKey>(),
    );
    if !dev_valid {
        init_device_key(vfs, &mut trng, &uid, hw_us);
    }

    // 2. `/sys/passcode.key`
    let pass_valid = key_valid(
        vfs,
        KEY_PATHS[1],
        MAGIC_PASSCODE_KEY,
        core::mem::size_of::<PackedPasscodeKey>(),
    );
    if !pass_valid {
        init_passcode_key(vfs, &mut trng, &uid, hw_us);
    }

    // 3. `/sys/storage.key`
    let sto_valid = key_valid(
        vfs,
        KEY_PATHS[2],
        MAGIC_STORAGE_KEY,
        core::mem::size_of::<PackedStorageKey>(),
    );
    if !sto_valid {
        init_storage_key(vfs, &mut trng, &uid, hw_us);
    }

    // Apply strict permissions to keys if newly created
    if !dev_valid || !pass_valid || !sto_valid {
        let sys_attr = crate::services::storage::FileAttributes {
            permission: crate::services::storage::PERM_SYSTEM
                | crate::services::storage::PERM_HIDDEN,
        };
        for &k in &KEY_PATHS {
            let _ = vfs.set_attributes(k, sys_attr);
        }
    }
}

pub fn xor_derived_key(
    uid: &[u8; 12],
    seed: &[u8],
    salt: &[u8; 12],
    iterations: u32,
    buf: &mut [u8; 32],
    progress: Option<super::kdf::ProgressHook>,
) {
    let mut kek = SecretBuffer::new();
    derive_passcode_kek(uid, seed, salt, iterations, &mut kek, progress);
    for i in 0..32 {
        buf[i] ^= kek.as_slice()[i];
    }
}

pub fn derive_passcode_kek(
    mcu_uid: &[u8; 12],
    user_passcode: &[u8],
    salt: &[u8; 12],
    iterations: u32,
    out_kek: &mut SecretBuffer,
    progress: Option<super::kdf::ProgressHook>,
) {
    super::kdf::derive_master_key(
        mcu_uid,
        user_passcode,
        salt,
        iterations,
        out_kek,
        Some(bsp::Board::wdt_feed),
        progress,
    );
}
