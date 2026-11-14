use crate::services::SystemStorage;
// Calibration Subsystem & Silicon Hardware Binding
//
// Provides structured access to factory radio calibration parameters,
// compile-time fallback for the target dev device, semantic validation,
// and hardware binding signatures against the 96-bit MCU UID.

use crate::services::storage::{ATTR_READ_ONLY, ATTR_SYSTEM, FileAttributes};
use crypto::ascon::Ascon128;

/// Factory radio calibration fallback (512 bytes).
pub const DEV_TARGET_FACTORY_CALIB: &[u8; 512] = include_bytes!("calib_default.dat");

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

    pub fn to_bytes(&self) -> [u8; 32] {
        unsafe { core::mem::transmute(*self) }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 32 || &bytes[..4] != CALIB_SIG_IDENT {
            return None;
        }
        let sig: Self = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) };
        Some(sig)
    }
}

/// Compute a 32-byte silicon binding signature linking calibration data to the MCU UID.
pub fn compute_device_signature(calib: &[u8; 512], mcu_uid: &[u8; 12]) -> CalibSignature {
    let hw_serial = bsp::uid::get_hardware_serial64();
    let nonce = [0x5Au8; 16];
    let mut key16 = [0u8; 16];
    key16[..4].copy_from_slice(CALIB_SIG_IDENT);
    key16[4..16].copy_from_slice(mcu_uid);
    let cipher = Ascon128::new(key16);
    let auth_tag = cipher.compute_mac(&nonce, CALIB_SIG_IDENT, calib);

    CalibSignature {
        ident: *CALIB_SIG_IDENT,
        version: 1,
        _reserved: [0; 3],
        hardware_serial: hw_serial,
        auth_tag,
    }
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
    sig.hardware_serial == computed.hardware_serial && sig.auth_tag == computed.auth_tag
}

/// Strict semantic validation of calibration memory block.
/// Rejects erased flash (all 0xFF), zeroed memory (all 0x00), or physically invalid values.
pub fn is_valid_calibration(calib: &[u8]) -> bool {
    if calib.len() < 512 {
        return false;
    }

    // 1. Check for blank / erased flash
    let mut all_ff = true;
    let mut all_00 = true;
    for &b in &calib[..64] {
        if b != 0xFF {
            all_ff = false;
        }
        if b != 0x00 {
            all_00 = false;
        }
    }
    if all_ff || all_00 {
        return false;
    }

    // 2. Validate Battery Calibration words at offset 0x140..0x14C (6 x u16)
    let mut words = [0u16; 6];
    for i in 0..6 {
        words[i] = u16::from_le_bytes([calib[0x140 + i * 2], calib[0x140 + i * 2 + 1]]);
    }

    // Word 3 (7.60V calibration baseline) must be in valid ADC range (1800..2600, authentic = 2227)
    let w3 = words[3];
    if w3 < 1800 || w3 > 2600 {
        return false;
    }

    // Monotonic sanity check: W0 < W1 < W2 < W3 < W4 < W5
    for i in 1..6 {
        if words[i] <= words[i - 1] {
            return false;
        }
    }

    // 3. Validate Crystal Trim at offset 0x188 (i16)
    let xtal = i16::from_le_bytes([calib[0x188], calib[0x189]]);
    if xtal < -1000 || xtal > 1000 {
        return false;
    }

    // 4. Validate Volume Gain at offset 0x18E (u8 <= 63)
    if calib[0x18E] > 63 {
        return false;
    }

    // 5. Validate DAC Gain at offset 0x18F (u8 <= 15)
    if calib[0x18F] > 15 {
        return false;
    }

    // 6. Validate LNA & Mix registers (9-bit register masks <= 0x01FF)
    let lna = u16::from_le_bytes([calib[0x18A], calib[0x18B]]);
    let mix = u16::from_le_bytes([calib[0x18C], calib[0x18D]]);
    if (lna & !0x01FF) != 0 || (mix & !0x01FF) != 0 {
        return false;
    }

    true
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

fn sign_and_save_sig(vfs: &mut SystemStorage, data: &[u8; 512], mcu_uid: &[u8; 12]) {
    let sig = compute_device_signature(data, mcu_uid);
    let sig_bytes = sig.to_bytes();
    let _ = vfs.write_file(CALIB_SIG_PATH, &sig_bytes);
    let attr = FileAttributes {
        permission: ATTR_SYSTEM | ATTR_READ_ONLY,
    };
    let _ = vfs.set_attributes(CALIB_SIG_PATH, attr);
}

impl SystemCalibration {
    pub const fn empty() -> Self {
        Self {
            raw: [0u8; 512],
            valid: false,
            bound: false,
        }
    }

    pub const fn new() -> Self {
        Self {
            raw: *DEV_TARGET_FACTORY_CALIB,
            valid: true,
            bound: false,
        }
    }

    #[inline(always)]
    pub fn init_defaults(&mut self) {
        self.raw.copy_from_slice(DEV_TARGET_FACTORY_CALIB);
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
        let data = crate::services::staging_buf();
        let mut loaded = false;

        // 1. Try primary calibration path
        if let Ok(n) = vfs.read_file(CALIB_FILE_PATH, data) {
            if n == 512 && is_valid_calibration(data) {
                loaded = true;
            }
        }

        // 2. Rollback to backup file if primary is missing or corrupted
        if !loaded {
            if let Ok(n) = vfs.read_file(CALIB_BAK_PATH, data) {
                if n == 512 && is_valid_calibration(data) {
                    let _ = vfs.write_file(CALIB_FILE_PATH, data);
                    let attr = FileAttributes {
                        permission: ATTR_SYSTEM | ATTR_READ_ONLY,
                    };
                    let _ = vfs.set_attributes(CALIB_FILE_PATH, attr);
                    sign_and_save_sig(vfs, data, mcu_uid);
                    loaded = true;
                }
            }
        }

        // 3. Last-resort fallback to embedded factory default
        if !loaded {
            data.copy_from_slice(DEV_TARGET_FACTORY_CALIB);
            let _ = vfs.write_file(CALIB_FILE_PATH, data);
            let attr = FileAttributes {
                permission: ATTR_SYSTEM | ATTR_READ_ONLY,
            };
            let _ = vfs.set_attributes(CALIB_FILE_PATH, attr);
            sign_and_save_sig(vfs, data, mcu_uid);
        }

        // Verify device binding signature
        let mut sig_buf = [0u8; 32];
        let mut bound = false;
        if let Ok(n) = vfs.read_file(CALIB_SIG_PATH, &mut sig_buf) {
            if n >= 32 {
                if let Some(sig) = CalibSignature::from_bytes(&sig_buf) {
                    bound = verify_device_signature(data, &sig, mcu_uid);
                }
            }
        }

        if !bound && loaded {
            // Re-sign if signature was missing but data was valid
            sign_and_save_sig(vfs, data, mcu_uid);
            bound = true;
        }

        self.raw.copy_from_slice(data);
        self.valid = true;
        self.bound = bound;
    }

    /// Atomically commit validated calibration with automatic backup, readback verification, and rollback.
    pub fn commit_with_backup(
        &mut self,
        vfs: &mut SystemStorage,
        new_calib: &[u8; 512],
        mcu_uid: &[u8; 12],
    ) -> Result<(), ()> {
        if !is_valid_calibration(new_calib) {
            return Err(());
        }

        let staging = unsafe { &mut *(crate::runtime::arena_base_ptr() as *mut [u8; 512]) };
        if let Ok(512) = vfs.read_file(CALIB_FILE_PATH, staging) {
            if is_valid_calibration(staging) {
                let _ = vfs.write_file(CALIB_BAK_PATH, staging);
            }
        }

        if vfs.write_file(CALIB_FILE_PATH, new_calib).is_err() {
            return Err(());
        }

        let read_ok = match vfs.read_file(CALIB_FILE_PATH, staging) {
            Ok(512) => staging == new_calib,
            _ => false,
        };

        if !read_ok {
            return Err(());
        }

        self.raw.copy_from_slice(new_calib);
        self.valid = true;

        let attr = FileAttributes {
            permission: ATTR_SYSTEM | ATTR_READ_ONLY,
        };
        let _ = vfs.set_attributes(CALIB_FILE_PATH, attr);
        sign_and_save_sig(vfs, new_calib, mcu_uid);
        self.bound = true;

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
