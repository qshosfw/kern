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

/// Base address of the 104-byte (`0x68`) factory provisioning certificate.
/// - UV-K1 / UV-K5v3 (10 KB bootloader): `0x0800_2700..0x0800_2767`
/// - UV-K10 Pro (8 KB bootloader): `0x0800_1F00..0x0800_1F67`
pub const FACTORY_CERT_ADDR: usize = if cfg!(feature = "target_k10pro") {
    0x0800_1F00
} else {
    0x0800_2700
};
pub const FACTORY_CERT_BYTES: usize = 104;

/// Bootloader metadata scan window in internal Flash (avoids reading application space).
pub const BOOTLOADER_ROM_SCAN_BASE: usize = if cfg!(feature = "target_k10pro") {
    0x0800_1800
} else {
    0x0800_1F00
};
pub const BOOTLOADER_ROM_SCAN_LEN: usize = if cfg!(feature = "target_k10pro") {
    0x0700
} else {
    0x0800
};

/// USB identification metadata extracted from the factory bootloader ROM (`0x0800_1F00..0x0800_2700`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BootloaderUsbInfo {
    pub vid: u16,
    pub pid: u16,
    pub product: [u8; 16],
    pub product_len: u8,
}

impl BootloaderUsbInfo {
    const fn default_model(digit: u8) -> Self {
        let mut product = [0u8; 16];
        product[0] = b'K';
        product[1] = digit;
        product[2] = b' ';
        product[3] = b'C';
        product[4] = b'P';
        product[5] = b'S';
        Self {
            vid: 0x36B7,
            pid: 0x0001,
            product,
            product_len: 6,
        }
    }

    pub const fn default_k1() -> Self {
        Self::default_model(b'1')
    }

    pub const fn default_k8() -> Self {
        Self::default_model(b'8')
    }

    pub const fn default_for_target() -> Self {
        if cfg!(feature = "target_k10pro") {
            Self::default_k8()
        } else {
            Self::default_k1()
        }
    }

    #[inline(always)]
    pub fn product_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.product[..self.product_len as usize]) }
    }
}

/// Scans a bootloader ROM slice (`0x0800_1F00..0x0800_2700`) for the USB 2.0 Device
/// Descriptor (`12 01 00 02 ...`) and its UTF-16LE `iProduct` string descriptor.
pub fn parse_bootloader_usb_info(rom: &[u8]) -> BootloaderUsbInfo {
    let mut info = BootloaderUsbInfo::default_for_target();
    let mut dev_end = None;
    let mut i_product = 2u8;

    let mut i = 0usize;
    while i + 18 <= rom.len() {
        if rom[i] == 0x12
            && rom[i + 1] == 0x01
            && rom[i + 2] == 0x00
            && rom[i + 3] == 0x02
            && rom[i + 7] == 0x40
        {
            info.vid = u16::from_le_bytes([rom[i + 8], rom[i + 9]]);
            info.pid = u16::from_le_bytes([rom[i + 10], rom[i + 11]]);
            i_product = rom[i + 15];
            dev_end = Some(i + 18);
            break;
        }
        i += 1;
    }

    if let Some(start) = dev_end {
        // Locate String Descriptor 0 (`04 03 09 04`, US-English LangID) then walk to `i_product`.
        let mut j = start;
        while j + 4 <= rom.len() {
            if rom[j] == 0x04 && rom[j + 1] == 0x03 && rom[j + 2] == 0x09 && rom[j + 3] == 0x04 {
                let mut pos = j + 4;
                let mut idx = 1u8;
                while pos + 2 <= rom.len() {
                    let len = rom[pos] as usize;
                    if len < 2 || (len & 1) != 0 || rom[pos + 1] != 0x03 || pos + len > rom.len() {
                        break;
                    }
                    if idx == i_product {
                        let mut n = 0usize;
                        let mut k = pos + 2;
                        while k + 1 < pos + len && n < info.product.len() {
                            if rom[k + 1] == 0 && (rom[k].is_ascii_graphic() || rom[k] == b' ') {
                                info.product[n] = rom[k];
                                n += 1;
                            }
                            k += 2;
                        }
                        if n > 0 {
                            info.product_len = n as u8;
                        }
                        break;
                    }
                    pos += len;
                    idx = idx.saturating_add(1);
                }
                break;
            }
            j += 1;
        }
    }

    info
}

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
    /// Flash (`BOOTLOADER_ROM_SCAN_BASE..+BOOTLOADER_ROM_SCAN_LEN`), falling back to `"7.02.02"` on host targets.
    #[inline(never)]
    pub fn bootloader_version_str() -> &'static str {
        #[cfg(all(not(test), target_arch = "arm"))]
        unsafe {
            let rom = core::slice::from_raw_parts(
                BOOTLOADER_ROM_SCAN_BASE as *const u8,
                BOOTLOADER_ROM_SCAN_LEN,
            );
            let mut i = 0usize;
            while i + 8 <= rom.len() {
                let w = &rom[i..i + 8];
                if w[7] == 0 {
                    let mut ok = true;
                    let mut j = 0usize;
                    while j < 7 {
                        let b = w[j];
                        let want_dot = j == 1 || j == 4;
                        if if want_dot {
                            b != b'.'
                        } else {
                            !b.is_ascii_digit()
                        } {
                            ok = false;
                            break;
                        }
                        j += 1;
                    }
                    if ok {
                        return core::str::from_utf8_unchecked(&w[..7]);
                    }
                }
                i += 1;
            }
        }
        "7.02.02"
    }

    /// Extracts USB VID, PID, and product descriptor name directly from the bootloader ROM
    /// (`BOOTLOADER_ROM_SCAN_BASE..+BOOTLOADER_ROM_SCAN_LEN`), falling back to `36B7:0001` / `"K1 CPS"` on host targets.
    pub fn bootloader_usb_info() -> BootloaderUsbInfo {
        #[cfg(all(not(test), target_arch = "arm"))]
        unsafe {
            let rom = core::slice::from_raw_parts(
                BOOTLOADER_ROM_SCAN_BASE as *const u8,
                BOOTLOADER_ROM_SCAN_LEN,
            );
            return parse_bootloader_usb_info(rom);
        }
        #[cfg(not(all(not(test), target_arch = "arm")))]
        BootloaderUsbInfo::default_for_target()
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
    #[inline(never)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bootloader_usb_info_reads_vid_pid_and_product() {
        let fixture: [u8; 0x9A] = [
            0x12, 0x01, 0x00, 0x02, 0xEF, 0x02, 0x01, 0x40, 0xB7, 0x36, 0x01, 0x00, 0x00, 0x01,
            0x01, 0x02, 0x03, 0x01, 0x09, 0x02, 0x4B, 0x00, 0x02, 0x01, 0x00, 0x80, 0x32, 0x08,
            0x0B, 0x00, 0x02, 0x02, 0x02, 0x01, 0x00, 0x09, 0x04, 0x00, 0x00, 0x01, 0x02, 0x02,
            0x01, 0x02, 0x05, 0x24, 0x00, 0x10, 0x01, 0x05, 0x24, 0x01, 0x00, 0x01, 0x04, 0x24,
            0x02, 0x02, 0x05, 0x24, 0x06, 0x00, 0x01, 0x07, 0x05, 0x83, 0x03, 0x08, 0x00, 0x0A,
            0x09, 0x04, 0x01, 0x00, 0x02, 0x0A, 0x00, 0x00, 0x00, 0x07, 0x05, 0x02, 0x02, 0x40,
            0x00, 0x00, 0x07, 0x05, 0x81, 0x02, 0x40, 0x00, 0x00, 0x04, 0x03, 0x09, 0x04, 0x14,
            0x03, 0x51, 0x00, 0x55, 0x00, 0x41, 0x00, 0x4E, 0x00, 0x53, 0x00, 0x48, 0x00, 0x45,
            0x00, 0x4E, 0x00, 0x47, 0x00, 0x0E, 0x03, 0x4B, 0x00, 0x31, 0x00, 0x20, 0x00, 0x43,
            0x00, 0x50, 0x00, 0x53, 0x00, 0x16, 0x03, 0x32, 0x00, 0x30, 0x00, 0x32, 0x00, 0x35,
            0x00, 0x30, 0x00, 0x38, 0x00, 0x30, 0x00, 0x38, 0x00, 0x30, 0x00, 0x31, 0x00, 0x00,
        ];
        let info = parse_bootloader_usb_info(&fixture);
        assert_eq!(info.vid, 0x36B7);
        assert_eq!(info.pid, 0x0001);
        assert_eq!(info.product_str(), "K1 CPS");
    }
}
