use crate::services::SystemStorage;
// Dynamic Hardware Silicon Key Pipeline & Security Keychain Containers

use crypto::ascon::Ascon128;
use crypto::buffer::SecretBuffer;
use crypto::trng::HardwareTrng;

use super::constants::*;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PackedDeviceKey {
    pub ident: [u8; 4],
    pub hardware_nonce: [u8; 16],
    pub device_signature: [u8; 16],
}

impl PackedDeviceKey {
    pub const fn empty() -> Self {
        Self {
            ident: [0; 4],
            hardware_nonce: [0; 16],
            device_signature: [0; 16],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PackedPasscodeKey {
    pub ident: [u8; 4],
    pub flags: u8,
    pub failed_attempts: u8,
    pub passcode_len: u8,
    pub max_tries: u8,
    pub iterations: u32,
    pub salt: [u8; 16],
    pub passcode_verifier: [u8; 32],
    pub mac: [u8; 16],
}

impl PackedPasscodeKey {
    pub const fn empty() -> Self {
        Self {
            ident: [0; 4],
            flags: 0,
            failed_attempts: 0,
            passcode_len: 0,
            max_tries: 0,
            iterations: 0,
            salt: [0; 16],
            passcode_verifier: [0; 32],
            mac: [0; 16],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PackedStorageKey {
    pub ident: [u8; 4],
    pub storage_nonce: [u8; 16],
    pub salt: [u8; 16],
    pub encrypted_mk: [u8; 32],
}

impl PackedStorageKey {
    pub const fn empty() -> Self {
        Self {
            ident: [0; 4],
            storage_nonce: [0; 16],
            salt: [0; 16],
            encrypted_mk: [0; 32],
        }
    }
}

pub(crate) fn compute_hw_mac(
    uid: &[u8; 12],
    nonce: &[u8; 16],
    ident: &[u8; 4],
    data: &[u8],
) -> [u8; 16] {
    let mut hardware_kek = SecretBuffer::new();
    derive_passcode_kek(uid, b"HW_KEK", nonce, 100, &mut hardware_kek, None);
    let mut key16 = [0u8; 16];
    key16.copy_from_slice(&hardware_kek.as_slice()[0..16]);
    let cipher = Ascon128::new(key16);
    cipher.compute_mac(nonce, ident, data)
}

fn key_valid(
    filesystem: &mut SystemStorage,
    path: &str,
    ident: &[u8; 4],
    expected_len: usize,
) -> bool {
    let mut header = [0u8; 4];
    filesystem.file_size(path) == Ok(expected_len)
        && filesystem.read_file(path, &mut header).is_ok()
        && &header == ident
}

fn init_device_key(filesystem: &mut SystemStorage, trng: &mut HardwareTrng, uid: &[u8; 12]) {
    let mut dev_key = PackedDeviceKey::empty();
    dev_key.ident = *FILE_ID_DEVICE_KEY;
    trng.fill_bytes(
        &mut dev_key.hardware_nonce,
        bsp::entropy::sample_hardware_noise(),
    );
    dev_key.device_signature =
        compute_hw_mac(uid, &dev_key.hardware_nonce, FILE_ID_DEVICE_KEY, uid);
    let _ = filesystem.write_struct(KEY_PATHS[0], &dev_key);
}

fn init_passcode_key(filesystem: &mut SystemStorage, trng: &mut HardwareTrng, uid: &[u8; 12]) {
    let mut passcode_key = PackedPasscodeKey::empty();
    passcode_key.ident = *FILE_ID_PASSCODE_KEY;
    passcode_key.max_tries = DEFAULT_MAX_TRIES;
    passcode_key.iterations = DEFAULT_PBKDF_ITERATIONS;
    trng.fill_bytes(
        &mut passcode_key.salt,
        bsp::entropy::sample_hardware_noise(),
    );

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
        FILE_ID_PASSCODE_KEY,
        &passcode_key.passcode_verifier,
    );

    let _ = filesystem.write_struct(KEY_PATHS[1], &passcode_key);
}

fn init_storage_key(filesystem: &mut SystemStorage, trng: &mut HardwareTrng, uid: &[u8; 12]) {
    let mut storage_key = PackedStorageKey::empty();
    storage_key.ident = *FILE_ID_STORAGE_KEY;
    let s_bytes = unsafe {
        core::slice::from_raw_parts_mut(&mut storage_key.storage_nonce as *mut _ as *mut u8, 64)
    };
    trng.fill_bytes(s_bytes, bsp::entropy::sample_hardware_noise());
    xor_derived_key(
        uid,
        DEFAULT_PASSCODE_SEED,
        &storage_key.salt,
        DEFAULT_PBKDF_ITERATIONS,
        &mut storage_key.encrypted_mk,
        None,
    );

    let _ = filesystem.write_struct(KEY_PATHS[2], &storage_key);
}

#[inline(never)]
pub fn ensure_keychain(filesystem: &mut SystemStorage) {
    let uid = bsp::uid::read_mcu_uid();
    let hw_noise = bsp::entropy::sample_hardware_noise();
    let mut trng = HardwareTrng::new();
    trng.init_with_uid(&uid, hw_noise);

    // 1. `/sys/device.key`
    let dev_valid = key_valid(
        filesystem,
        KEY_PATHS[0],
        FILE_ID_DEVICE_KEY,
        core::mem::size_of::<PackedDeviceKey>(),
    );
    if !dev_valid {
        init_device_key(filesystem, &mut trng, &uid);
    }

    // 2. `/sys/passcode.key`
    let pass_valid = key_valid(
        filesystem,
        KEY_PATHS[1],
        FILE_ID_PASSCODE_KEY,
        core::mem::size_of::<PackedPasscodeKey>(),
    );
    if !pass_valid {
        init_passcode_key(filesystem, &mut trng, &uid);
    }

    // 3. `/sys/storage.key`
    let sto_valid = key_valid(
        filesystem,
        KEY_PATHS[2],
        FILE_ID_STORAGE_KEY,
        core::mem::size_of::<PackedStorageKey>(),
    );
    if !sto_valid {
        init_storage_key(filesystem, &mut trng, &uid);
    }

    // Apply strict permissions to keys if newly created
    if !dev_valid || !pass_valid || !sto_valid {
        let sys_attr = crate::services::storage::FileAttributes {
            permission: crate::services::storage::ATTR_SYSTEM
                | crate::services::storage::ATTR_HIDDEN,
        };
        for &k in &KEY_PATHS {
            let _ = filesystem.set_attributes(k, sys_attr);
        }
    }
}

pub fn xor_derived_key(
    uid: &[u8; 12],
    seed: &[u8],
    salt: &[u8; 16],
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
    salt: &[u8; 16],
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

/// Derives a domain-separated, UID-bound 128-bit `FileSystemDriver` key from the 256-bit master key.
pub fn derive_fs_key(master_key: &[u8; 32], mcu_uid: &[u8; 12]) -> [u8; 16] {
    let mut perm = crypto::ascon::AsconPermutation::new();
    // Domain tag: "KRN_FS01"
    perm.x[0] = 0x4B52_4E5F_4653_3031;
    perm.x[1] = u64::from_le_bytes(master_key[0..8].try_into().unwrap());
    perm.x[2] = u64::from_le_bytes(master_key[8..16].try_into().unwrap());
    perm.x[3] = u64::from_le_bytes(master_key[16..24].try_into().unwrap())
        ^ u64::from_le_bytes(mcu_uid[0..8].try_into().unwrap());
    let uid_hi = u32::from_le_bytes(mcu_uid[8..12].try_into().unwrap()) as u64;
    perm.x[4] = u64::from_le_bytes(master_key[24..32].try_into().unwrap()) ^ uid_hi;
    perm.permute(12);
    let mut fs_key = [0u8; 16];
    fs_key[0..8].copy_from_slice(&perm.x[0].to_le_bytes());
    fs_key[8..16].copy_from_slice(&perm.x[1].to_le_bytes());
    perm.x.fill(0);
    fs_key
}

/// Unwraps the UID-bound device storage master key using `DEFAULT_PASSCODE_SEED`.
/// Used when `secure_fs` is active without a user passcode.
pub fn unwrap_device_master_key(
    filesystem: &mut SystemStorage,
    uid: &[u8; 12],
) -> Option<SecretBuffer> {
    let mut sto_key = PackedStorageKey::empty();
    let sn = filesystem
        .read_struct(PATH_STORAGE_KEY, &mut sto_key)
        .ok()?;
    if sn < core::mem::size_of::<PackedStorageKey>() || sto_key.ident != *FILE_ID_STORAGE_KEY {
        return None;
    }

    let mut pass_key = PackedPasscodeKey::empty();
    let iterations = match filesystem.read_struct(PATH_PASSCODE_KEY, &mut pass_key) {
        Ok(pn)
            if pn >= core::mem::size_of::<PackedPasscodeKey>()
                && pass_key.ident == *FILE_ID_PASSCODE_KEY
                && pass_key.iterations != 0
                && pass_key.iterations != 0xFFFF_FFFF =>
        {
            pass_key.iterations
        }
        _ => DEFAULT_PBKDF_ITERATIONS,
    };

    let mut mk = SecretBuffer::new();
    mk.as_mut_slice().copy_from_slice(&sto_key.encrypted_mk);
    xor_derived_key(
        uid,
        DEFAULT_PASSCODE_SEED,
        &sto_key.salt,
        iterations,
        mk.as_mut_slice(),
        None,
    );
    Some(mk)
}

/// Ensures keychain exists and installs the UID-bound device FS key into `FileSystemDriver`.
pub fn unlock_device_storage(filesystem: &mut SystemStorage) -> bool {
    ensure_keychain(filesystem);
    let uid = bsp::uid::read_mcu_uid();
    if let Some(mk) = unwrap_device_master_key(filesystem, &uid) {
        let mut fs_key = derive_fs_key(mk.as_slice(), &uid);
        filesystem.set_encryption_key(fs_key);
        fs_key.fill(0);
        true
    } else {
        filesystem.lock_session();
        false
    }
}
