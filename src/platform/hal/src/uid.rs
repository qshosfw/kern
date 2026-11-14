//! Hardware Electronic Signature & Factory Calibration Introspection for PY32F071.
//!
//! - `UID_BASE`: `0x1FFF_3000` (256-byte system electronic signature)
//! - `FLASHSIZE_BASE`: `0x1FFF_31FC` (16-bit Flash size in KB)
//! - `TSENSOR_CAL1`: `0x1FFF_3228` (30°C calibration point @ 3.3V)
//! - `TSENSOR_CAL2`: `0x1FFF_3230` (105°C calibration point @ 3.3V)

pub const UID_BASE_ADDR: usize = 0x1FFF_3000;
pub const FLASH_SIZE_REG: usize = 0x1FFF_31FC;
pub const VREFINT_CAL_ADDR: usize = 0x1FFF_0F04;
pub const TSENSOR_CAL1_ADDR: usize = 0x1FFF_3228;
pub const TSENSOR_CAL2_ADDR: usize = 0x1FFF_3230;
pub const TSENSOR_CAL_ADDR: usize = TSENSOR_CAL1_ADDR;
pub const VREFINT_MV_TYPICAL: u16 = 1200;

pub struct SystemInfo;

impl SystemInfo {
    /// Reads the internal Flash capacity of the MCU in kilobytes (e.g. 128 for PY32F071xB).
    #[inline(always)]
    pub fn flash_size_kb() -> u16 {
        let ptr = FLASH_SIZE_REG as *const u16;
        let sz = unsafe { ptr.read_volatile() };
        if sz == 0 || sz == 0xFFFF { 128 } else { sz }
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
