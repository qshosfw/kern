//! Calibration Subsystem & Silicon Hardware Binding
//!
//! Uses a 64-byte (`ActiveCalib`) resident cache in SRAM (`-448 B` vs full 512 B block).
//! The full 512-byte `/sys/calib.dat` is read into `KERNEL_IO_SCRATCH` (`staging_buf()`)
//! only on boot, band-boundary crossing, or calibration commit/reformat.

use calib::{ActiveCalib, CalibrationBlock};
use kernfs::{ATTR_READ_ONLY, ATTR_SYSTEM, BlockDevice, FsError, KernFS};

const DEFAULT_CALIB_RAW: &[u8; 512] = include_bytes!("calib_default.dat");

const fn packbits_len_calib() -> usize {
    let bytes = DEFAULT_CALIB_RAW;
    let mut r = 0usize;
    let mut w = 0usize;
    while r < bytes.len() {
        let b = bytes[r];
        let mut run = 1usize;
        while r + run < bytes.len() && run < 128 && bytes[r + run] == b {
            run += 1;
        }
        if run >= 2 {
            w += 2;
            r += run;
        } else {
            let start = r;
            while r < bytes.len()
                && r - start < 128
                && !(r + 2 < bytes.len() && bytes[r] == bytes[r + 1] && bytes[r] == bytes[r + 2])
            {
                r += 1;
            }
            w += 1 + (r - start);
        }
    }
    w
}

const DEFAULT_CALIB_PB_LEN: usize = packbits_len_calib();

const fn encode_default_calib_packbits() -> [u8; DEFAULT_CALIB_PB_LEN] {
    let bytes = DEFAULT_CALIB_RAW;
    let mut out = [0u8; DEFAULT_CALIB_PB_LEN];
    let mut r = 0usize;
    let mut w = 0usize;
    while r < bytes.len() {
        let b = bytes[r];
        let mut run = 1usize;
        while r + run < bytes.len() && run < 128 && bytes[r + run] == b {
            run += 1;
        }
        if run >= 2 {
            out[w] = (257 - run) as u8;
            out[w + 1] = b;
            w += 2;
            r += run;
        } else {
            let start = r;
            while r < bytes.len()
                && r - start < 128
                && !(r + 2 < bytes.len() && bytes[r] == bytes[r + 1] && bytes[r] == bytes[r + 2])
            {
                r += 1;
            }
            let len = r - start;
            out[w] = (len - 1) as u8;
            w += 1;
            let mut k = 0;
            while k < len {
                out[w + k] = bytes[start + k];
                k += 1;
            }
            w += len;
        }
    }
    out
}

/// PackBits-compressed factory calibration fallback (expands to 512 B).
static DEFAULT_CALIB_PB: [u8; DEFAULT_CALIB_PB_LEN] = encode_default_calib_packbits();

/// Expand the PackBits-compressed default calibration image into a 512-byte buffer.
#[inline(always)]
pub fn fill_default_calibration(dst: &mut [u8; 512]) {
    utils::codec::packbits_decompress_clamped(&DEFAULT_CALIB_PB, dst);
}

pub const CALIB_FILE_PATH: &str = "/sys/calib.dat";
pub const CALIB_BAK_PATH: &str = "/sys/calib.bak";
pub const CALIB_SIG_PATH: &str = "/sys/calib.sig";

pub const CALIB_SIG_IDENT: &[u8; 4] = b"CLBS";

/// 32-byte Hardware Silicon Binding Signature for calibration data.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CalibSignature {
    pub ident: [u8; 4],       // b"CLBS"
    pub version: u8,          // 1
    pub _reserved: [u8; 3],   // Alignment padding
    pub hardware_serial: u64, // MurmurHash3 64-bit hardware serial
    pub auth_tag: [u8; 16],   // Ascon-128 MAC over the 512-byte calibration data
}

impl CalibSignature {
    pub const fn empty() -> Self {
        Self {
            ident: [0; 4],
            version: 0,
            _reserved: [0; 3],
            hardware_serial: 0,
            auth_tag: [0; 16],
        }
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        const _: () = assert!(core::mem::size_of::<CalibSignature>() == 32);
        unsafe { &*(self as *const Self as *const [u8; 32]) }
    }

    pub fn to_bytes(&self) -> [u8; 32] {
        *self.as_bytes()
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 32 || &bytes[..4] != CALIB_SIG_IDENT {
            return None;
        }
        unsafe {
            let mut sig = core::mem::MaybeUninit::<Self>::uninit();
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), sig.as_mut_ptr() as *mut u8, 32);
            Some(sig.assume_init())
        }
    }
}

/// Compute a 32-byte silicon binding signature linking calibration data to the MCU UID.
pub fn compute_device_signature(calib: &[u8; 512], mcu_uid: &[u8; 12]) -> CalibSignature {
    static NONCE: [u8; 16] = [0x5Au8; 16];
    let hw_serial = bsp::uid::get_hardware_serial64();
    #[cfg(feature = "secure_fs")]
    let auth_tag =
        crate::security::keychain::compute_hw_mac(mcu_uid, &NONCE, CALIB_SIG_IDENT, calib);
    #[cfg(not(feature = "secure_fs"))]
    let auth_tag = {
        let mut key16 = [0u8; 16];
        key16[..4].copy_from_slice(CALIB_SIG_IDENT);
        key16[4..16].copy_from_slice(mcu_uid);
        crypto::ascon::Ascon128::new(key16).compute_mac(&NONCE, CALIB_SIG_IDENT, calib)
    };

    CalibSignature {
        ident: *CALIB_SIG_IDENT,
        version: 1,
        _reserved: [0; 3],
        hardware_serial: hw_serial,
        auth_tag,
    }
}

/// Constant-time byte-slice equality to prevent timing side-channels on MAC tags.
fn ct_eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Strict semantic validation of calibration memory block.
pub fn is_valid_calibration(calib: &[u8]) -> bool {
    if calib.len() < 512 {
        return false;
    }

    let first = calib[0];
    if first == 0x00 || first == 0xFF {
        let mut same = true;
        let mut i = 1;
        while i < 64 {
            if calib[i] != first {
                same = false;
                break;
            }
            i += 1;
        }
        if same {
            return false;
        }
    }

    let read_u16 = |off: usize| unsafe {
        u16::from_le_bytes([*calib.get_unchecked(off), *calib.get_unchecked(off + 1)])
    };

    if read_u16(0x146).wrapping_sub(1800) > 800 {
        return false;
    }

    for i in 0..5 {
        if read_u16(0x142 + i * 2) <= read_u16(0x140 + i * 2) {
            return false;
        }
    }

    (read_u16(0x188) as i16).unsigned_abs() <= 1000
        && calib[0x18E] <= 63
        && calib[0x18F] <= 15
        && ((read_u16(0x18A) | read_u16(0x18C)) & !0x01FF) == 0
}

pub(crate) fn read_valid_calib<D: BlockDevice>(
    vfs: &mut KernFS<'_, D>,
    path: &str,
    buf: &mut [u8; 512],
) -> bool {
    matches!(vfs.read_file(path, buf), Ok(512) if is_valid_calibration(buf))
}

fn write_protected_sys_file<D: BlockDevice>(
    vfs: &mut KernFS<'_, D>,
    path: &str,
    data: &[u8],
) -> Result<(), FsError> {
    let _ = vfs.set_permission(path, 0);
    let res = vfs.write_file(path, data);
    let _ = vfs.set_permission(path, ATTR_SYSTEM | ATTR_READ_ONLY);
    res
}

fn sign_and_save_sig<D: BlockDevice>(
    vfs: &mut KernFS<'_, D>,
    data: &[u8; 512],
    mcu_uid: &[u8; 12],
) {
    let sig = compute_device_signature(data, mcu_uid);
    let _ = write_protected_sys_file(vfs, CALIB_SIG_PATH, sig.as_bytes());
}

#[inline(never)]
pub(crate) fn persist_calib_bundle<D: BlockDevice>(vfs: &mut KernFS<'_, D>, raw_calib: &[u8; 512]) {
    let _ = vfs.create_dir("/sys");
    let _ = write_protected_sys_file(vfs, CALIB_FILE_PATH, raw_calib);
    let _ = write_protected_sys_file(vfs, CALIB_BAK_PATH, raw_calib);
    let mcu_uid = bsp::uid::read_mcu_uid();
    sign_and_save_sig(vfs, raw_calib, &mcu_uid);
}

#[cfg(feature = "filesystem")]
pub fn reformat_storage<D: BlockDevice>(
    vfs: &mut KernFS<'_, D>,
    keep_calib: bool,
) -> Result<(), ()> {
    let staging = crate::services::staging_buf();
    if keep_calib {
        let loaded = read_valid_calib(vfs, CALIB_FILE_PATH, staging)
            || read_valid_calib(vfs, CALIB_BAK_PATH, staging);
        if !loaded {
            fill_default_calibration(staging);
        }
    }
    if vfs.format(None).is_err() {
        return Err(());
    }
    if keep_calib {
        persist_calib_bundle(vfs, staging);
    }
    Ok(())
}

/// Populate `ActiveCalib` with compiled factory defaults using `staging_buf()`.
pub fn init_active_defaults(active: &mut ActiveCalib) {
    let staging = crate::services::staging_buf();
    fill_default_calibration(staging);
    active.load_from_block(
        CalibrationBlock::from_bytes(staging),
        145_500_000,
        3,
        0,
        false,
    );
}

/// Load `/sys/calib.dat` via `KERNEL_IO_SCRATCH`, verify silicon binding, and populate `ActiveCalib`.
pub fn load_active_calib<D: BlockDevice>(
    active: &mut ActiveCalib,
    vfs: &mut KernFS<'_, D>,
    mcu_uid: &[u8; 12],
) {
    let staging = crate::services::staging_buf();
    let mut loaded = read_valid_calib(vfs, CALIB_FILE_PATH, staging);
    if !loaded {
        if read_valid_calib(vfs, CALIB_BAK_PATH, staging) {
            loaded = true;
        } else {
            fill_default_calibration(staging);
        }
        let _ = write_protected_sys_file(vfs, CALIB_FILE_PATH, staging);
        sign_and_save_sig(vfs, staging, mcu_uid);
    }

    const _: () = {
        assert!(core::mem::offset_of!(CalibSignature, hardware_serial) == 8);
        assert!(core::mem::offset_of!(CalibSignature, auth_tag) == 16);
    };
    let mut sig_buf = [0u8; 32];
    let mut sig_present = false;
    let mut bound = !loaded;
    if let Ok(n) = vfs.read_file(CALIB_SIG_PATH, &mut sig_buf) {
        sig_present = true;
        if n >= 32 && &sig_buf[..4] == CALIB_SIG_IDENT {
            let computed = compute_device_signature(staging, mcu_uid);
            bound = ct_eq_bytes(&sig_buf[8..32], &computed.as_bytes()[8..32]);
        }
    }
    if !sig_present && loaded {
        sign_and_save_sig(vfs, staging, mcu_uid);
        bound = true;
    }

    active.load_from_block(
        CalibrationBlock::from_bytes(staging),
        145_500_000,
        3,
        0,
        bound,
    );
}

/// Reload `ActiveCalib` when crossing into a new calibration frequency band or squelch level.
pub fn refresh_active_band<D: BlockDevice>(
    active: &mut ActiveCalib,
    vfs: &mut KernFS<'_, D>,
    freq_hz: u32,
    squelch_lvl: u8,
) {
    let band = calib::get_band_index(freq_hz) as u8;
    if active.is_valid() && active.band_idx == band && active.squelch_lvl == squelch_lvl.min(9) {
        return;
    }
    let staging = crate::services::staging_buf();
    if !read_valid_calib(vfs, CALIB_FILE_PATH, staging) {
        fill_default_calibration(staging);
    }
    let bound = active.is_device_bound();
    let vox_lvl = active.vox_lvl;
    active.load_from_block(
        CalibrationBlock::from_bytes(staging),
        freq_hz,
        squelch_lvl,
        vox_lvl,
        bound,
    );
}

/// Atomically commit a validated 512-byte calibration slice, update backup & signature, and refresh `ActiveCalib`.
pub fn commit_calib_slice<D: BlockDevice>(
    active: &mut ActiveCalib,
    vfs: &mut KernFS<'_, D>,
    data: &[u8],
) -> Result<(), ()> {
    if data.len() != 512 || !is_valid_calibration(data) {
        return Err(());
    }
    let new_calib: &[u8; 512] = unsafe { &*(data.as_ptr() as *const [u8; 512]) };
    let mcu_uid = bsp::uid::read_mcu_uid();
    let mut verify = [0u8; 64];

    // Backup existing valid calib if present.
    let mut prev = [0u8; 512];
    let backed_up = read_valid_calib(vfs, CALIB_FILE_PATH, &mut prev)
        && write_protected_sys_file(vfs, CALIB_BAK_PATH, &prev).is_ok();

    let write_ok = write_protected_sys_file(vfs, CALIB_FILE_PATH, new_calib).is_ok();
    let mut read_ok = write_ok;
    if read_ok {
        let mut off = 0usize;
        while off < 512 {
            if vfs.read_file_at(CALIB_FILE_PATH, off, &mut verify) != Ok(64)
                || verify != new_calib[off..off + 64]
            {
                read_ok = false;
                break;
            }
            off += 64;
        }
    }

    if !read_ok {
        if backed_up {
            let _ = write_protected_sys_file(vfs, CALIB_FILE_PATH, &prev);
        }
        return Err(());
    }

    sign_and_save_sig(vfs, new_calib, &mcu_uid);
    let freq_hz = calib::CALIB_BAND_LIMITS[(active.band_idx as usize).min(6)].1;
    let sql = active.squelch_lvl;
    let vox = active.vox_lvl;
    active.load_from_block(
        CalibrationBlock::from_bytes(new_calib),
        freq_hz,
        sql,
        vox,
        true,
    );
    unsafe {
        (*core::ptr::addr_of_mut!(crate::POWER_SERVICE))
            .battery_driver
            .set_calib_const(active.battery_calib_const());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calib_signature_byte_roundtrip() {
        let mut sig = CalibSignature::empty();
        sig.ident = *CALIB_SIG_IDENT;
        sig.version = 1;
        sig._reserved = [0xAA, 0xBB, 0xCC];
        sig.hardware_serial = 0x0123_4567_89AB_CDEF;
        sig.auth_tag = [0x42; 16];

        let encoded = sig.to_bytes();
        let decoded = CalibSignature::from_bytes(&encoded).expect("valid CalibSignature");
        assert_eq!(decoded.ident, *CALIB_SIG_IDENT);
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded._reserved, [0xAA, 0xBB, 0xCC]);
        assert_eq!(decoded.hardware_serial, 0x0123_4567_89AB_CDEF);
        assert_eq!(decoded.auth_tag, [0x42; 16]);

        let mut bad = encoded;
        bad[0] = b'X';
        assert!(CalibSignature::from_bytes(&bad).is_none());
        assert!(CalibSignature::from_bytes(&encoded[..31]).is_none());
    }

    #[test]
    fn default_calibration_packbits_matches_reference_image() {
        let mut decoded = [0u8; 512];
        fill_default_calibration(&mut decoded);
        assert_eq!(&decoded, DEFAULT_CALIB_RAW);
    }
}
