//! Hardware Electronic Signature, Bootloader ROM & Factory Provisioning Introspection for PY32F071.
//!
//! - `UID_BASE`: `0x1FFF_3000` (256-byte system electronic signature)
//! - `FLASHSIZE_BASE`: `0x1FFF_31FC` (16-bit Flash size in KB)
//! - `TSENSOR_CAL1`: `0x1FFF_3228` (30°C calibration point @ 3.3V)
//! - `TSENSOR_CAL2`: `0x1FFF_3230` (105°C calibration point @ 3.3V)
//! - `FACTORY_CERT_ADDR`: `0x0800_2700` (104-byte Quansheng factory provisioning certificate &
//!   UID-bound AES-128-CBC license block in the protected bootloader tail page `0x0800_2700..0x0800_2767`)

pub const UID_BASE_ADDR: usize = 0x1FFF_3000;
pub const FLASH_SIZE_REG: usize = 0x1FFF_31FC;
pub const VREFINT_CAL_ADDR: usize = 0x1FFF_0F04;
pub const TSENSOR_CAL1_ADDR: usize = 0x1FFF_3228;
pub const TSENSOR_CAL2_ADDR: usize = 0x1FFF_3230;
pub const TSENSOR_CAL_ADDR: usize = TSENSOR_CAL1_ADDR;
pub const VREFINT_MV_TYPICAL: u16 = 1200;

/// Base address of the 104-byte (`0x68`) factory provisioning certificate written via bootloader
/// command `0x0516` into the protected bootloader tail page (`0x0800_2700..0x0800_2767`).
/// Contains 80 bytes of interleaved challenge/AES-128-CBC signature words (`0x0800_2700..0x0800_274F`)
/// bound to the MCU 128-bit UID (`0x1FFF_3000`) followed by a 24-byte factory stamp (`0x0800_2750..0x0800_2767`).
pub const FACTORY_CERT_ADDR: usize = 0x0800_2700;
pub const FACTORY_CERT_BYTES: usize = 104;

pub struct SystemInfo;

impl SystemInfo {
    /// Reads the internal Flash capacity of the MCU in kilobytes (e.g. 128 for PY32F071xB).
    #[inline(always)]
    pub fn flash_size_kb() -> u16 {
        #[cfg(all(not(test), target_arch = "arm"))]
        {
            let ptr = FLASH_SIZE_REG as *const u16;
            let sz = unsafe { ptr.read_volatile() };
            if sz == 0 || sz == 0xFFFF { 128 } else { sz }
        }
        #[cfg(not(all(not(test), target_arch = "arm")))]
        {
            128
        }
    }

    /// Reads the factory bootloader version string (e.g. `"7.02.02"`) directly from internal MCU
    /// Flash (`0x0800_1F00..0x0800_2700`), falling back to `"7.02.02"` on host unit-test targets.
    pub fn bootloader_version_str() -> &'static str {
        #[cfg(all(not(test), target_arch = "arm"))]
        unsafe {
            let rom = core::slice::from_raw_parts(0x0800_1F00 as *const u8, 0x0800);
            let mut i = 0usize;
            while i + 8 <= rom.len() {
                let w = &rom[i..i + 8];
                if w[1] == b'.'
                    && w[4] == b'.'
                    && w[7] == 0
                    && w[0].is_ascii_digit()
                    && w[2].is_ascii_digit()
                    && w[3].is_ascii_digit()
                    && w[5].is_ascii_digit()
                    && w[6].is_ascii_digit()
                {
                    return core::str::from_utf8_unchecked(&w[..7]);
                }
                i += 1;
            }
        }
        "7.02.02"
    }

    /// Returns a slice over the 104-byte factory provisioning certificate at `0x0800_2700..0x0800_2768`
    /// if the sector is factory-provisioned (first word is neither erased `0xFFFF_FFFF` nor `0x0000_0000`).
    #[inline(always)]
    pub fn factory_cert_slice() -> Option<&'static [u8; FACTORY_CERT_BYTES]> {
        #[cfg(all(not(test), target_arch = "arm"))]
        unsafe {
            let w0 = (FACTORY_CERT_ADDR as *const u32).read_volatile();
            if w0 == 0xFFFF_FFFF || w0 == 0x0000_0000 {
                None
            } else {
                Some(&*(FACTORY_CERT_ADDR as *const [u8; FACTORY_CERT_BYTES]))
            }
        }
        #[cfg(not(all(not(test), target_arch = "arm")))]
        {
            static HOST_FACTORY_CERT: [u8; FACTORY_CERT_BYTES] = [
                0x2C, 0x40, 0xC8, 0x9B, 0x27, 0x88, 0xF2, 0xDE, 0x48, 0x11, 0x51, 0x34, 0x73, 0x26,
                0xFC, 0xEE, 0x8C, 0xC4, 0xF8, 0xB4, 0x44, 0xB9, 0xAD, 0xA1, 0x8B, 0x1D, 0x6D, 0x34,
                0xBD, 0xC0, 0xBB, 0xC6, 0x32, 0x14, 0x3F, 0x19, 0x81, 0x26, 0x17, 0xFC, 0x4A, 0x33,
                0xF1, 0x49, 0x30, 0x1F, 0xEF, 0x7C, 0x6A, 0xB1, 0xEC, 0x90, 0x45, 0x30, 0xEB, 0x0C,
                0xC2, 0xD3, 0x10, 0x05, 0xF4, 0x56, 0x15, 0x73, 0x02, 0xC7, 0xC6, 0x9B, 0x5F, 0xBC,
                0xFD, 0xAB, 0x40, 0x76, 0x87, 0x9F, 0xFE, 0x24, 0x78, 0x0F, 0x31, 0x2D, 0x73, 0x71,
                0x43, 0x31, 0x31, 0x31, 0x35, 0x32, 0x30, 0x32, 0x32, 0x32, 0x39, 0x30, 0x39, 0x31,
                0x36, 0x31, 0x40, 0xA4, 0x38, 0x34,
            ];
            Some(&HOST_FACTORY_CERT)
        }
    }

    /// Typical internal reference voltage in millivolts (1200 mV on PY32F071).
    #[inline(always)]
    pub const fn vrefint_mv_typical() -> u16 {
        VREFINT_MV_TYPICAL
    }

    /// Reads the factory internal reference calibration slot if queried by legacy callers.
    #[inline(always)]
    pub fn vrefint_calibration() -> u16 {
        let ptr = VREFINT_CAL_ADDR as *const u16;
        unsafe { ptr.read_volatile() }
    }

    /// Reads the factory temperature sensor calibration word (at 30°C / 85°C).
    #[inline(always)]
    pub fn tsensor_calibration() -> u16 {
        let ptr = TSENSOR_CAL_ADDR as *const u16;
        unsafe { ptr.read_volatile() }
    }

    /// Reads the first 12 bytes (96-bit) of the unique device identifier.
    pub fn read_uid_12() -> [u8; 12] {
        #[allow(unused_mut)]
        let mut uid = [0u8; 12];
        #[cfg(all(not(test), target_arch = "arm"))]
        {
            let ptr = UID_BASE_ADDR as *const u8;
            for i in 0..12 {
                uid[i] = unsafe { ptr.add(i).read_volatile() };
            }
        }
        uid
    }

    /// Reads the 16-byte CPU ID block used by DeltaFW.
    pub fn read_uid_16() -> [u8; 16] {
        #[allow(unused_mut)]
        let mut id = [0u8; 16];
        #[cfg(all(not(test), target_arch = "arm"))]
        {
            let ptr = UID_BASE_ADDR as *const u8;
            for i in 0..16 {
                id[i] = unsafe { ptr.add(i).read_volatile() };
            }
        }
        id
    }

    /// Reads the full 256-byte factory system UID space.
    pub fn read_uid_256() -> [u8; 256] {
        #[allow(unused_mut)]
        let mut full = [0u8; 256];
        #[cfg(all(not(test), target_arch = "arm"))]
        {
            let ptr = UID_BASE_ADDR as *const u8;
            for i in 0..256 {
                full[i] = unsafe { ptr.add(i).read_volatile() };
            }
        }
        full
    }
}
