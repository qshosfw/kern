// Dynamic Hardware Silicon Key Pipeline & Security Keychain Containers

use crypto::ascon::Ascon128;
use crypto::buffer::SecretBuf;
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
    let mut key16 = [0u8; 16];
    key16[..4].copy_from_slice(ident);
    key16[4..16].copy_from_slice(uid);
    Ascon128::new(key16).compute_mac(nonce, ident, data)
}

fn key_valid(
    filesystem: &mut kernfs::KernFS<'_, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    path: &str,
    ident: &[u8; 4],
    expected_len: usize,
) -> bool {
    let mut header = [0u8; 4];
    filesystem.file_size(path) == Ok(expected_len)
        && filesystem.read_file(path, &mut header).is_ok()
        && &header == ident
}

fn fill_rand(trng: &mut HardwareTrng, buf: &mut [u8]) {
    trng.fill_bytes(buf, bsp::entropy::sample_hardware_noise());
}

fn init_device_key(
    filesystem: &mut kernfs::KernFS<'_, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    trng: &mut HardwareTrng,
    uid: &[u8; 12],
) {
    let mut dev_key = PackedDeviceKey::empty();
    dev_key.ident = *FILE_ID_DEVICE_KEY;
    fill_rand(trng, &mut dev_key.hardware_nonce);
    dev_key.device_signature =
        compute_hw_mac(uid, &dev_key.hardware_nonce, FILE_ID_DEVICE_KEY, uid);
    let _ = filesystem.write_struct(KEY_PATHS[0], &dev_key);
}

pub fn write_passcode_key(
    filesystem: &mut kernfs::KernFS<'_, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    trng: &mut HardwareTrng,
    uid: &[u8; 12],
    new_passcode: &[u8],
    flags: u8,
    max_tries: u8,
    progress: Option<super::kdf::ProgressHook>,
) -> Result<(), ()> {
    let is_enable = !new_passcode.is_empty();
    let seed = if is_enable {
        new_passcode
    } else {
        DEFAULT_PASSCODE_SEED
    };
    let mut passcode_key = PackedPasscodeKey {
        ident: *FILE_ID_PASSCODE_KEY,
        flags: if is_enable { flags } else { 0 },
        failed_attempts: 0,
        passcode_len: new_passcode.len() as u8,
        max_tries: if is_enable {
            max_tries.clamp(3, 50)
        } else {
            DEFAULT_MAX_TRIES
        },
        iterations: DEFAULT_PBKDF_ITERATIONS,
        salt: [0; 16],
        passcode_verifier: [0; 32],
        mac: [0; 16],
    };
    fill_rand(trng, &mut passcode_key.salt);

    let mut passcode_verifier = SecretBuf::<32>::new();
    derive_passcode_kek(
        uid,
        seed,
        &passcode_key.salt,
        DEFAULT_PBKDF_ITERATIONS,
        &mut passcode_verifier,
        progress,
    );
    passcode_key.passcode_verifier = *passcode_verifier.as_slice();
    passcode_key.mac = compute_hw_mac(
        uid,
        &passcode_key.salt,
        FILE_ID_PASSCODE_KEY,
        &passcode_key.passcode_verifier,
    );

    filesystem
        .write_struct(KEY_PATHS[1], &passcode_key)
        .map_err(|_| ())
}

pub fn write_storage_key(
    filesystem: &mut kernfs::KernFS<'_, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    trng: &mut HardwareTrng,
    uid: &[u8; 12],
    seed: &[u8],
    master_key: &mut [u8; 32],
    progress: Option<super::kdf::ProgressHook>,
) -> Result<(), ()> {
    if master_key.iter().all(|&b| b == 0) {
        fill_rand(trng, master_key);
    }
    let mut storage_key = PackedStorageKey {
        ident: *FILE_ID_STORAGE_KEY,
        storage_nonce: [0; 16],
        salt: [0; 16],
        encrypted_mk: *master_key,
    };
    let nonce_and_salt = unsafe {
        core::slice::from_raw_parts_mut(&mut storage_key.storage_nonce as *mut _ as *mut u8, 32)
    };
    fill_rand(trng, nonce_and_salt);
    xor_derived_key(
        uid,
        seed,
        &storage_key.salt,
        DEFAULT_PBKDF_ITERATIONS,
        &mut storage_key.encrypted_mk,
        progress,
    );

    filesystem
        .write_struct(KEY_PATHS[2], &storage_key)
        .map_err(|_| ())
}

pub fn ensure_keychain(
    filesystem: &mut kernfs::KernFS<'_, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
) {
    let uid = bsp::uid::read_mcu_uid();
    let mut trng = HardwareTrng::new();
    trng.init_with_uid(&uid, bsp::entropy::sample_hardware_noise());

    let mut created = false;
    if !key_valid(filesystem, KEY_PATHS[0], FILE_ID_DEVICE_KEY, 36) {
        init_device_key(filesystem, &mut trng, &uid);
        created = true;
    }
    if !key_valid(filesystem, KEY_PATHS[1], FILE_ID_PASSCODE_KEY, 76) {
        let _ = write_passcode_key(filesystem, &mut trng, &uid, &[], 0, DEFAULT_MAX_TRIES, None);
        created = true;
    }
    if !key_valid(filesystem, KEY_PATHS[2], FILE_ID_STORAGE_KEY, 68) {
        let mut mk = [0u8; 32];
        let _ = write_storage_key(
            filesystem,
            &mut trng,
            &uid,
            DEFAULT_PASSCODE_SEED,
            &mut mk,
            None,
        );
        mk.fill(0);
        created = true;
    }

    if created {
        let perm = kernfs::ATTR_SYSTEM | kernfs::ATTR_HIDDEN;
        for &k in &KEY_PATHS {
            let _ = filesystem.set_permission(k, perm);
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
    let mut kek = SecretBuf::<32>::new();
    derive_passcode_kek(uid, seed, salt, iterations, &mut kek, progress);
    for i in 0..32 {
        buf[i] ^= kek.as_slice()[i];
    }
}

#[inline(always)]
pub fn derive_passcode_kek(
    mcu_uid: &[u8; 12],
    user_passcode: &[u8],
    salt: &[u8; 16],
    iterations: u32,
    out_kek: &mut SecretBuf<32>,
    progress: Option<super::kdf::ProgressHook>,
) {
    super::kdf::derive_master_key(mcu_uid, user_passcode, salt, iterations, out_kek, progress);
}

/// Derives a domain-separated, UID-bound 128-bit `FileSystemDriver` key from the 256-bit master key.
pub fn derive_fs_key(master_key: &[u8; 32], mcu_uid: &[u8; 12]) -> [u8; 16] {
    let mut perm = crypto::ascon::AsconPermutation::new();
    // Domain tag: "KRN_FS01"
    perm.x[0] = 0x4B52_4E5F_4653_3031;
    #[cfg(target_endian = "little")]
    unsafe {
        let p = (&raw mut perm.x[1]).cast::<u8>();
        core::ptr::copy_nonoverlapping(master_key.as_ptr(), p, 32);
        for (i, &b) in mcu_uid.iter().enumerate() {
            *p.add(16 + i) ^= b;
        }
    }
    #[cfg(not(target_endian = "little"))]
    {
        for i in 0..4 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&master_key[i * 8..(i + 1) * 8]);
            perm.x[i + 1] = u64::from_le_bytes(b);
        }
        for (i, &b) in mcu_uid.iter().enumerate() {
            perm.x[3 + (i >> 3)] ^= (b as u64) << ((i & 7) * 8);
        }
    }
    perm.permute(12);
    let mut fs_key = [0u8; 16];
    #[cfg(target_endian = "little")]
    unsafe {
        core::ptr::copy_nonoverlapping(perm.x.as_ptr().cast::<u8>(), fs_key.as_mut_ptr(), 16);
    }
    #[cfg(not(target_endian = "little"))]
    {
        fs_key[0..8].copy_from_slice(&perm.x[0].to_le_bytes());
        fs_key[8..16].copy_from_slice(&perm.x[1].to_le_bytes());
    }
    perm.x.fill(0);
    fs_key
}

/// Unwraps the UID-bound device storage master key using `DEFAULT_PASSCODE_SEED`.
/// Used when `secure_fs` is active without a user passcode.
#[cfg(not(feature = "passcode"))]
pub fn unwrap_device_master_key(
    filesystem: &mut kernfs::KernFS<'_, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
    uid: &[u8; 12],
) -> Option<SecretBuf<32>> {
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

    let mut mk = SecretBuf::<32>::new();
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
#[cfg(not(feature = "passcode"))]
pub fn unlock_device_storage(
    filesystem: &mut kernfs::KernFS<'_, drivers::py25q16::Py25q16FlashDriver<bsp::FlashBus>>,
) -> bool {
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
