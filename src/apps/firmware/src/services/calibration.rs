use crate::services::SystemStorage;
// Calibration Subsystem & Silicon Hardware Binding
//
// Provides structured access to factory radio calibration parameters,
// compile-time fallback for the target dev device, semantic validation,
// and hardware binding signatures against the 96-bit MCU UID.

use crate::security::kdf::derive_master_key;
use crate::services::storage::{FileAttributes, PERM_READ_ONLY, PERM_SYSTEM};
use crypto::buffer::SecretBuffer;
use crypto::chacha20poly1305::ChaCha20Poly1305;

/// Factory radio calibration fallback (512 bytes).
pub const DEV_TARGET_FACTORY_CALIB: &[u8; 512] = include_bytes!("calib_default.dat");

pub const CALIB_FILE_PATH: &str = "/sys/calib.dat";
pub const CALIB_SIG_PATH: &str = "/sys/calib.sig";

pub const CALIB_SIG_MAGIC: &[u8; 4] = b"CLBS";

/// 32-byte Hardware Silicon Binding Signature for calibration data.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CalibSignature {
    pub magic: [u8; 4],       // b"CLBS"
    pub version: u8,          // 1
    pub _reserved: [u8; 3],   // Alignment padding
    pub hardware_serial: u64, // MurmurHash3 64-bit hardware serial
    pub auth_tag: [u8; 16],   // Poly1305 MAC over the 512-byte calibration data
}

impl CalibSignature {
    pub const fn empty() -> Self {
        Self {
            magic: [0; 4],
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
        if bytes.len() < 32 || &bytes[..4] != CALIB_SIG_MAGIC {
            return None;
        }
        let sig: Self = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) };
        Some(sig)
    }
}

/// Compute a 32-byte silicon binding signature linking calibration data to the MCU UID.
pub fn compute_device_signature(calib: &[u8; 512], mcu_uid: &[u8; 12]) -> CalibSignature {
    let hw_serial = bsp::uid::get_hardware_serial64();

    // Derive a unique hardware KEK for calibration binding
    let mut calib_kek = SecretBuffer::new();
    let nonce = [0x5Au8; 12];
    derive_master_key(
        mcu_uid,
        b"CALIB_KEK",
        &nonce,
        50,
        &mut calib_kek,
        Some(bsp::Board::wdt_feed),
        None,
    );

    let cipher = ChaCha20Poly1305::new(*calib_kek.as_slice());
    let auth_tag = cipher.compute_mac(&nonce, CALIB_SIG_MAGIC, calib);

    CalibSignature {
        magic: *CALIB_SIG_MAGIC,
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
    if &sig.magic != CALIB_SIG_MAGIC {
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

    true
}

/// Declarative service interface for system calibration access.
pub trait CalibrationService {
    fn is_valid(&self) -> bool;
    fn is_device_bound(&self) -> bool;
    fn battery_calib_const(&self) -> u16;
    fn battery_curve(&self) -> [u16; 6];
    fn xtal_freq_low(&self) -> i16;
    fn lna_calib(&self) -> u16;
    fn mix_calib(&self) -> u16;
    fn volume_gain(&self) -> u8;
    fn dac_gain(&self) -> u8;
    fn raw_bytes(&self) -> &[u8; 512];
}

/// Concrete in-memory system calibration holder.
pub struct SystemCalibration {
    data: [u8; 512],
    valid: bool,
    bound: bool,
}

fn sign_and_save_sig(vfs: &mut SystemStorage, data: &[u8; 512], mcu_uid: &[u8; 12]) {
    let sig = compute_device_signature(data, mcu_uid);
    let sig_bytes = sig.to_bytes();
    let _ = vfs.write_file(CALIB_SIG_PATH, &sig_bytes);
    let attr = FileAttributes {
        permission: PERM_SYSTEM | PERM_READ_ONLY,
    };
    let _ = vfs.set_attributes(CALIB_SIG_PATH, attr);
}

impl SystemCalibration {
    pub const fn new() -> Self {
        Self {
            data: [0; 512],
            valid: false,
            bound: false,
        }
    }

    /// Load calibration into self from VFS with fallback to embedded dev calibration and signature verification.
    pub fn load_into(&mut self, vfs: &mut SystemStorage, mcu_uid: &[u8; 12]) {
        let mut loaded = false;

        if let Ok(n) = vfs.read_file(CALIB_FILE_PATH, &mut self.data) {
            if n == 512 && is_valid_calibration(&self.data) {
                loaded = true;
            }
        }

        if !loaded {
            // Restore from embedded compile-time fallback for our target dev device
            self.data.copy_from_slice(DEV_TARGET_FACTORY_CALIB);
            let _ = vfs.write_file(CALIB_FILE_PATH, &self.data);
            let attr = FileAttributes {
                permission: PERM_SYSTEM | PERM_READ_ONLY,
            };
            let _ = vfs.set_attributes(CALIB_FILE_PATH, attr);
            sign_and_save_sig(vfs, &self.data, mcu_uid);
        }

        // Verify device binding signature
        let mut sig_buf = [0u8; 32];
        let mut bound = false;
        if let Ok(n) = vfs.read_file(CALIB_SIG_PATH, &mut sig_buf) {
            if n >= 32 {
                if let Some(sig) = CalibSignature::from_bytes(&sig_buf) {
                    bound = verify_device_signature(&self.data, &sig, mcu_uid);
                }
            }
        }

        if !bound && loaded {
            // Re-sign if signature was missing but data was valid
            sign_and_save_sig(vfs, &self.data, mcu_uid);
            bound = true;
        }

        self.valid = true;
        self.bound = bound;
    }

    #[inline(always)]
    pub fn battery_calib_const(&self) -> u16 {
        u16::from_le_bytes([self.data[0x146], self.data[0x147]])
    }

    #[inline(always)]
    pub fn xtal_freq_low(&self) -> i16 {
        i16::from_le_bytes([self.data[0x188], self.data[0x189]])
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
        u16::from_le_bytes([self.data[0x146], self.data[0x147]])
    }

    fn battery_curve(&self) -> [u16; 6] {
        let mut curve = [0u16; 6];
        for i in 0..6 {
            curve[i] = u16::from_le_bytes([self.data[0x140 + i * 2], self.data[0x140 + i * 2 + 1]]);
        }
        curve
    }

    fn xtal_freq_low(&self) -> i16 {
        i16::from_le_bytes([self.data[0x188], self.data[0x189]])
    }

    fn lna_calib(&self) -> u16 {
        u16::from_le_bytes([self.data[0x18A], self.data[0x18B]]) & 0x01FF
    }

    fn mix_calib(&self) -> u16 {
        u16::from_le_bytes([self.data[0x18C], self.data[0x18D]]) & 0x01FF
    }

    fn volume_gain(&self) -> u8 {
        let val = self.data[0x18E];
        if val < 64 { val } else { 58 }
    }

    fn dac_gain(&self) -> u8 {
        self.data[0x18F]
    }

    fn raw_bytes(&self) -> &[u8; 512] {
        &self.data
    }
}
