use crate::services::SystemStorage;
// Calibration Subsystem & Silicon Hardware Binding
//
// Provides structured access to factory radio calibration parameters,
// compile-time fallback for the target dev device, semantic validation,
// and hardware binding signatures against the 96-bit MCU UID.

use crate::services::storage::{ATTR_READ_ONLY, ATTR_SYSTEM, FileAttributes};

/// RLE-compressed factory radio calibration fallback (expands to 512 B; `0xFF <n>` skips `n` `0xFF` bytes).
const DEFAULT_CALIB_RLE: [u8; 310] = [
    0x0A, 0x53, 0x55, 0x57, 0x59, 0x5B, 0x5D, 0x5F, 0x62, 0x65, 0xFF, 0x06, 0x05, 0x50, 0x52, 0x54,
    0x56, 0x58, 0x5A, 0x5C, 0x5F, 0x62, 0xFF, 0x06, 0x5A, 0x2D, 0x29, 0x26, 0x23, 0x20, 0x1D, 0x1A,
    0x17, 0x14, 0xFF, 0x06, 0x64, 0x30, 0x2D, 0x29, 0x26, 0x23, 0x20, 0x1D, 0x1A, 0x17, 0xFF, 0x06,
    0x5A, 0x14, 0x11, 0x0E, 0x0B, 0x08, 0x03, 0x02, 0x02, 0x02, 0xFF, 0x06, 0x64, 0x11, 0x0E, 0x0B,
    0x08, 0x05, 0x05, 0x04, 0x04, 0x04, 0xFF, 0x06, 0x32, 0x58, 0x5B, 0x5E, 0x60, 0x63, 0x65, 0x67,
    0x69, 0x6B, 0xFF, 0x06, 0x28, 0x55, 0x58, 0x5B, 0x5D, 0x60, 0x62, 0x64, 0x66, 0x68, 0xFF, 0x06,
    0x41, 0x32, 0x2D, 0x28, 0x24, 0x21, 0x1E, 0x1A, 0x17, 0x16, 0xFF, 0x06, 0x46, 0x37, 0x32, 0x2D,
    0x28, 0x25, 0x22, 0x1E, 0x1B, 0x19, 0xFF, 0x06, 0x5A, 0x19, 0x0F, 0x0A, 0x09, 0x08, 0x07, 0x06,
    0x05, 0x04, 0xFF, 0x06, 0x64, 0x1E, 0x14, 0x0F, 0x0D, 0x0C, 0x0B, 0x0A, 0x09, 0x08, 0xFF, 0x06,
    0x6E, 0x00, 0x78, 0x00, 0x82, 0x00, 0x8C, 0x00, 0xB4, 0x00, 0xBE, 0x00, 0xC8, 0x00, 0xD2, 0x00,
    0x32, 0x32, 0x32, 0x64, 0x64, 0x64, 0x8C, 0x8C, 0x8C, 0xFF, 0x07, 0x32, 0x32, 0x32, 0x64, 0x64,
    0x64, 0x8C, 0x8C, 0x8C, 0xFF, 0x07, 0x4B, 0x4B, 0x4B, 0x78, 0x78, 0x78, 0xA0, 0xA0, 0xA0, 0xFF,
    0x07, 0x32, 0x32, 0x32, 0x64, 0x64, 0x64, 0x8C, 0x8C, 0x8C, 0xFF, 0x07, 0x5A, 0x5A, 0x5A, 0x64,
    0x64, 0x64, 0xA0, 0xA0, 0xA0, 0xFF, 0x07, 0x4B, 0x4B, 0x4B, 0x78, 0x78, 0x78, 0xA0, 0xA0, 0xA0,
    0xFF, 0x07, 0x32, 0x32, 0x32, 0x64, 0x64, 0x64, 0x94, 0x8C, 0x8C, 0xFF, 0x07, 0x9E, 0x05, 0x02,
    0x08, 0x5A, 0x08, 0xB3, 0x08, 0x0A, 0x09, 0x28, 0x0A, 0xFF, 0x04, 0x1E, 0x00, 0x32, 0x00, 0x46,
    0x00, 0x5A, 0x00, 0x6E, 0x00, 0x82, 0x00, 0x96, 0x00, 0xAA, 0x00, 0xC8, 0x00, 0xE6, 0x00, 0xFF,
    0x04, 0x14, 0x00, 0x28, 0x00, 0x3C, 0x00, 0x50, 0x00, 0x64, 0x00, 0x78, 0x00, 0x8C, 0x00, 0xA0,
    0x00, 0xBE, 0x00, 0xDC, 0x00, 0xFF, 0x04, 0x08, 0x0E, 0x13, 0x18, 0x1E, 0xFF, 0x03, 0x04, 0x00,
    0x46, 0x00, 0x50, 0x00, 0x37, 0x0E,
];

/// Expand the RLE-compressed default calibration image into a 512-byte buffer.
#[inline(always)]
pub fn fill_default_calibration(dst: &mut [u8; 512]) {
    utils::format::decode_rle_escape(&DEFAULT_CALIB_RLE, 0xFF, 0, dst);
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

    #[inline(always)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 32] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 32]) }
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
#[inline(never)]
pub fn compute_device_signature(calib: &[u8; 512], mcu_uid: &[u8; 12]) -> CalibSignature {
    static NONCE: [u8; 16] = [0x5Au8; 16];
    let hw_serial = bsp::uid::get_hardware_serial64();
    #[cfg(feature = "secure_fs")]
    let auth_tag = crate::security::keychain::compute_hw_mac(mcu_uid, &NONCE, CALIB_SIG_IDENT, calib);
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
#[inline(never)]
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

/// Verify whether a signature matches both the MCU UID and the calibration contents.
pub fn verify_device_signature(
    calib: &[u8; 512],
    sig: &CalibSignature,
    mcu_uid: &[u8; 12],
) -> bool {
    if &sig.ident != CALIB_SIG_IDENT {
        return false;
    }
    let computed = compute_device_signature(calib, mcu_uid);
    sig.hardware_serial == computed.hardware_serial
        && ct_eq_bytes(&sig.auth_tag, &computed.auth_tag)
}

/// Strict semantic validation of calibration memory block.
/// Rejects erased flash (all 0xFF), zeroed memory (all 0x00), or physically invalid values.
#[inline(never)]
pub fn is_valid_calibration(calib: &[u8]) -> bool {
    if calib.len() < 512 {
        return false;
    }

    // 1. Check for blank / erased flash
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

    // 2. Word 3 (7.60V calibration baseline) in valid ADC range (1800..=2600, authentic = 2227)
    if read_u16(0x146).wrapping_sub(1800) > 800 {
        return false;
    }

    // Monotonic sanity check: W0 < W1 < W2 < W3 < W4 < W5
    for i in 0..5 {
        if read_u16(0x142 + i * 2) <= read_u16(0x140 + i * 2) {
            return false;
        }
    }

    // 3..6. Crystal trim (-1000..=1000), volume gain (<=63), DAC gain (<=15), LNA/Mix masks (<=0x01FF)
    (read_u16(0x188) as i16).unsigned_abs() <= 1000
        && calib[0x18E] <= 63
        && calib[0x18F] <= 15
        && ((read_u16(0x18A) | read_u16(0x18C)) & !0x01FF) == 0
}

/// Declarative service interface for system calibration access.
pub trait CalibrationService {
    fn is_valid(&self) -> bool;
    fn is_device_bound(&self) -> bool;
    fn battery_calib_const(&self) -> u16;
    fn battery_curve(&self) -> [u16; 6];
    fn battery_mv_from_adc(&self, raw_adc: u16) -> u16;
    fn battery_percent_from_adc(&self, raw_adc: u16) -> u8;
    fn xtal_freq_low(&self) -> i16;
    fn lna_calib(&self) -> u16;
    fn mix_calib(&self) -> u16;
    fn volume_gain(&self) -> u8;
    fn dac_gain(&self) -> u8;
    fn squelch_thresholds(
        &self,
        freq_hz: u32,
        level: u8,
    ) -> kernel::service::radio::squelch::SquelchThresholds;
    fn tx_power_bias(&self, freq_hz: u32, power_level: u8) -> u8;
    fn rssi_thresholds(&self, freq_hz: u32) -> [u16; 4];
    fn s_meter_level(&self, freq_hz: u32, raw_rssi: u16) -> u8;
    fn vox_thresholds(&self, level: u8) -> (u16, u16);
    fn mic_gain_db(&self, level: u8) -> u8;
    fn raw_bytes(&self) -> &[u8; 512];
}

/// Concrete in-memory system calibration holder (zero-copy 512-byte CalibrationBlock view).
#[repr(C, align(4))]
pub struct SystemCalibration {
    raw: [u8; 512],
    valid: bool,
    bound: bool,
}

const _: () = {
    assert!(core::mem::offset_of!(SystemCalibration, raw) == 0);
    assert!(core::mem::align_of::<SystemCalibration>() >= 4);
};

#[inline(never)]
pub(crate) fn read_valid_calib(vfs: &mut SystemStorage, path: &str, buf: &mut [u8; 512]) -> bool {
    matches!(vfs.read_file(path, buf), Ok(512) if is_valid_calibration(buf))
}

#[inline(never)]
fn write_protected_sys_file(
    vfs: &mut SystemStorage,
    path: &str,
    data: &[u8],
) -> Result<(), crate::services::storage::StorageError> {
    let _ = vfs.set_attributes(path, FileAttributes { permission: 0 });
    let res = vfs.write_file(path, data);
    let _ = vfs.set_attributes(
        path,
        FileAttributes {
            permission: ATTR_SYSTEM | ATTR_READ_ONLY,
        },
    );
    res
}

#[inline(never)]
fn sign_and_save_sig(vfs: &mut SystemStorage, data: &[u8; 512], mcu_uid: &[u8; 12]) {
    let sig = compute_device_signature(data, mcu_uid);
    let _ = write_protected_sys_file(vfs, CALIB_SIG_PATH, sig.as_bytes());
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn persist_calib_bundle(vfs: &mut SystemStorage, data: &[u8; 512]) {
    let _ = vfs.create_dir("/sys");
    let _ = write_protected_sys_file(vfs, CALIB_FILE_PATH, data);
    let _ = write_protected_sys_file(vfs, CALIB_BAK_PATH, data);
    let mcu_uid = bsp::uid::read_mcu_uid();
    sign_and_save_sig(vfs, data, &mcu_uid);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn reformat_storage(
    vfs: &mut SystemStorage,
    fallback_calib: &[u8; 512],
    keep_calib: bool,
) -> Result<(), ()> {
    let staging = crate::services::staging_buf();
    if keep_calib {
        let loaded = read_valid_calib(vfs, CALIB_FILE_PATH, staging)
            || read_valid_calib(vfs, CALIB_BAK_PATH, staging);
        if !loaded {
            staging.copy_from_slice(fallback_calib);
        }
    }
    if vfs.fs.format(None).is_err() {
        return Err(());
    }
    if keep_calib {
        persist_calib_bundle(vfs, staging);
    }
    Ok(())
}

impl SystemCalibration {
    pub const fn empty() -> Self {
        Self {
            raw: [0u8; 512],
            valid: false,
            bound: false,
        }
    }

    #[inline(always)]
    pub fn init_defaults(&mut self) {
        fill_default_calibration(&mut self.raw);
        self.valid = true;
        self.bound = false;
    }

    #[inline(always)]
    pub fn as_block(&self) -> &calib::CalibrationBlock {
        calib::CalibrationBlock::from_bytes(&self.raw)
    }

    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    #[inline(always)]
    pub fn lna_calib(&self) -> u16 {
        self.as_block().lna_calib
    }

    #[inline(always)]
    pub fn mix_calib(&self) -> u16 {
        self.as_block().mix_calib
    }

    #[inline(always)]
    pub fn volume_gain(&self) -> u8 {
        self.as_block().volume_gain
    }

    #[inline(always)]
    pub fn squelch_thresholds(
        &self,
        freq_hz: u32,
        level: u8,
    ) -> kernel::service::radio::squelch::SquelchThresholds {
        let th = self.as_block().get_squelch_thresholds(freq_hz, level);
        kernel::service::radio::squelch::SquelchThresholds {
            open_rssi: th.open_rssi,
            close_rssi: th.close_rssi,
            open_noise: th.open_noise,
            close_noise: th.close_noise,
            open_glitch: th.open_glitch,
            close_glitch: th.close_glitch,
        }
    }

    #[inline(always)]
    pub fn tx_power_bias(&self, freq_hz: u32, power_level: u8) -> u8 {
        self.as_block().get_tx_power_bias(freq_hz, power_level)
    }

    #[inline(always)]
    pub fn rssi_thresholds(&self, freq_hz: u32) -> [u16; 4] {
        self.as_block().get_rssi_thresholds(freq_hz)
    }

    #[inline(always)]
    pub fn s_meter_level(&self, freq_hz: u32, raw_rssi: u16) -> u8 {
        self.as_block().get_s_meter_level(freq_hz, raw_rssi)
    }

    #[inline(always)]
    pub fn battery_mv_from_adc(&self, raw_adc: u16) -> u16 {
        self.as_block().battery_mv(raw_adc)
    }

    #[inline(always)]
    pub fn battery_percent_from_adc(&self, raw_adc: u16) -> u8 {
        self.as_block().battery_percentage(raw_adc)
    }

    #[inline(always)]
    pub fn vox_thresholds(&self, level: u8) -> (u16, u16) {
        self.as_block().get_vox_thresholds(level)
    }

    #[inline(always)]
    pub fn mic_gain_db(&self, level: u8) -> u8 {
        self.as_block().get_mic_gain_db(level)
    }

    /// Load calibration into self from VFS with fallback to backup, factory default, and signature verification.
    #[inline(never)]
    pub fn load_into(&mut self, vfs: &mut SystemStorage, mcu_uid: &[u8; 12]) {
        let mut loaded = read_valid_calib(vfs, CALIB_FILE_PATH, &mut self.raw);

        if !loaded {
            if read_valid_calib(vfs, CALIB_BAK_PATH, &mut self.raw) {
                loaded = true;
            } else {
                fill_default_calibration(&mut self.raw);
            }
            let _ = write_protected_sys_file(vfs, CALIB_FILE_PATH, &self.raw);
            sign_and_save_sig(vfs, &self.raw, mcu_uid);
        }

        // Verify device binding signature (re-sign only if signature file is absent)
        const _: () = {
            assert!(core::mem::offset_of!(CalibSignature, hardware_serial) == 8);
            assert!(core::mem::offset_of!(CalibSignature, auth_tag) == 16);
        };
        let sig_buf = &mut crate::services::staging_buf()[..32];
        let mut sig_present = false;
        let mut bound = !loaded;
        if let Ok(n) = vfs.read_file(CALIB_SIG_PATH, sig_buf) {
            sig_present = true;
            if n >= 32 && &sig_buf[..4] == CALIB_SIG_IDENT {
                let computed = compute_device_signature(&self.raw, mcu_uid);
                bound = ct_eq_bytes(&sig_buf[8..32], &computed.as_bytes()[8..32]);
            }
        }

        if !sig_present && loaded {
            sign_and_save_sig(vfs, &self.raw, mcu_uid);
            bound = true;
        }

        self.valid = true;
        self.bound = bound;
    }

    /// Atomically commit validated calibration with automatic backup, readback verification, and rollback.
    #[inline(never)]
    pub fn commit_with_backup(
        &mut self,
        vfs: &mut SystemStorage,
        new_calib: &[u8; 512],
        mcu_uid: &[u8; 12],
    ) -> Result<(), ()> {
        // Use TRANSIENT_ARENA tail staging_buf() ([4096..4608)) so RigWire payloads in [0..4096) never alias.
        let staging = crate::services::staging_buf();
        let backed_up = read_valid_calib(vfs, CALIB_FILE_PATH, staging)
            && write_protected_sys_file(vfs, CALIB_BAK_PATH, staging).is_ok();

        let write_ok = write_protected_sys_file(vfs, CALIB_FILE_PATH, new_calib).is_ok();
        let read_ok = write_ok
            && match vfs.read_file(CALIB_FILE_PATH, staging) {
                Ok(512) => staging == new_calib,
                _ => false,
            };

        if !read_ok {
            if backed_up && vfs.read_file(CALIB_BAK_PATH, staging) == Ok(512) {
                let _ = write_protected_sys_file(vfs, CALIB_FILE_PATH, staging);
            }
            return Err(());
        }

        self.raw.copy_from_slice(new_calib);
        self.valid = true;
        sign_and_save_sig(vfs, new_calib, mcu_uid);
        self.bound = true;

        Ok(())
    }

    #[inline(never)]
    pub fn commit_slice(&mut self, vfs: &mut SystemStorage, data: &[u8]) -> Result<(), ()> {
        if data.len() != 512 || !is_valid_calibration(data) {
            return Err(());
        }
        let arr: &[u8; 512] = unsafe { &*(data.as_ptr() as *const [u8; 512]) };
        let mcu_uid = bsp::uid::read_mcu_uid();
        self.commit_with_backup(vfs, arr, &mcu_uid)?;
        unsafe {
            (*core::ptr::addr_of_mut!(crate::POWER_SERVICE))
                .battery_driver
                .set_calib_const(self.battery_calib_const());
        }
        Ok(())
    }

    #[inline(always)]
    pub fn battery_calib_const(&self) -> u16 {
        self.as_block().battery_calib_const()
    }

    #[inline(always)]
    pub fn battery_curve(&self) -> [u16; 6] {
        self.as_block().battery_curve
    }

    #[inline(always)]
    pub fn xtal_freq_low(&self) -> i16 {
        self.as_block().xtal_freq_low()
    }

    #[inline(always)]
    pub fn raw_bytes(&self) -> &[u8; 512] {
        &self.raw
    }
}

impl CalibrationService for SystemCalibration {
    fn is_valid(&self) -> bool {
        self.valid
    }

    fn is_device_bound(&self) -> bool {
        self.bound
    }

    fn battery_calib_const(&self) -> u16 {
        self.battery_calib_const()
    }

    fn battery_curve(&self) -> [u16; 6] {
        self.battery_curve()
    }

    fn battery_mv_from_adc(&self, raw_adc: u16) -> u16 {
        self.battery_mv_from_adc(raw_adc)
    }

    fn battery_percent_from_adc(&self, raw_adc: u16) -> u8 {
        self.battery_percent_from_adc(raw_adc)
    }

    fn xtal_freq_low(&self) -> i16 {
        self.xtal_freq_low()
    }

    fn lna_calib(&self) -> u16 {
        self.as_block().lna_calib()
    }

    fn mix_calib(&self) -> u16 {
        self.as_block().mix_calib()
    }

    fn volume_gain(&self) -> u8 {
        self.as_block().volume_gain()
    }

    fn dac_gain(&self) -> u8 {
        self.as_block().dac_gain()
    }

    fn squelch_thresholds(
        &self,
        freq_hz: u32,
        level: u8,
    ) -> kernel::service::radio::squelch::SquelchThresholds {
        self.squelch_thresholds(freq_hz, level)
    }

    fn tx_power_bias(&self, freq_hz: u32, power_level: u8) -> u8 {
        self.tx_power_bias(freq_hz, power_level)
    }

    fn rssi_thresholds(&self, freq_hz: u32) -> [u16; 4] {
        self.rssi_thresholds(freq_hz)
    }

    fn s_meter_level(&self, freq_hz: u32, raw_rssi: u16) -> u8 {
        self.s_meter_level(freq_hz, raw_rssi)
    }

    fn vox_thresholds(&self, level: u8) -> (u16, u16) {
        self.vox_thresholds(level)
    }

    fn mic_gain_db(&self, level: u8) -> u8 {
        self.mic_gain_db(level)
    }

    fn raw_bytes(&self) -> &[u8; 512] {
        &self.raw
    }
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
    fn default_calibration_rle_matches_reference_image() {
        const REF: &[u8; 512] = include_bytes!("calib_default.dat");
        let mut decoded = [0u8; 512];
        fill_default_calibration(&mut decoded);
        assert_eq!(&decoded, REF);
    }
}
