//! Hardware Electronic Signature & Factory Calibration Introspection for PY32F071.
//!
//! Sourced from official CMSIS device descriptors:
//! - UID_BASE: 0x1FFF_3000 (256 bytes)
//! - FLASHSIZE_BASE: 0x1FFF_31FC (16-bit Flash size in KB)
//! - VREFINT_CAL: 0x1FFF_0F04 / 0x1FFF_0F06 (factory reference calibration)
//! - TSENSOR_CAL: 0x1FFF_0F14 / 0x1FFF_0F16 (factory temperature sensor calibration)

pub const UID_BASE_ADDR: usize = 0x1FFF_3000;
pub const FLASH_SIZE_REG: usize = 0x1FFF_31FC;
pub const VREFINT_CAL_ADDR: usize = 0x1FFF_0F04;
pub const TSENSOR_CAL_ADDR: usize = 0x1FFF_0F14;

pub struct SystemInfo;

impl SystemInfo {
    /// Reads the internal Flash capacity of the MCU in kilobytes (e.g. 128 for PY32F071xB).
    #[inline(always)]
    pub fn flash_size_kb() -> u16 {
        let ptr = FLASH_SIZE_REG as *const u16;
        let sz = unsafe { ptr.read_volatile() };
        if sz == 0 || sz == 0xFFFF { 128 } else { sz }
    }

    /// Reads the factory internal reference voltage calibration word.
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
        let mut uid = [0u8; 12];
        let ptr = UID_BASE_ADDR as *const u8;
        for i in 0..12 {
            uid[i] = unsafe { ptr.add(i).read_volatile() };
        }
        uid
    }

    /// Reads the 16-byte CPU ID block used by DeltaFW.
    pub fn read_uid_16() -> [u8; 16] {
        let mut id = [0u8; 16];
        let ptr = UID_BASE_ADDR as *const u8;
        for i in 0..16 {
            id[i] = unsafe { ptr.add(i).read_volatile() };
        }
        id
    }

    /// Reads the full 256-byte factory system UID space.
    pub fn read_uid_256() -> [u8; 256] {
        let mut full = [0u8; 256];
        let ptr = UID_BASE_ADDR as *const u8;
        for i in 0..256 {
            full[i] = unsafe { ptr.add(i).read_volatile() };
        }
        full
    }
}
