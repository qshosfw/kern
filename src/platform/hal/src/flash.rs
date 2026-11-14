//! On-Chip Flash Memory Programming & Page Erase Controller for PY32F071.
//!
//! Page Size: 256 bytes (64 words).
//! Sector Size: 512 bytes (2 pages).
//! Base Address: 0x0800_0000 (up to 128 KB).
//! Bootloader Protected Region: 0x0800_0000..0x0800_2800 (10 KB).
//!
//! Erase and program sequences execute from `.data.ramfunc` (SRAM) with IRQs
//! masked so AHB instruction fetches never stall or HardFault the active bank.

use pac::flash::*;

pub const FLASH_PAGE_SIZE: usize = 256;
pub const FLASH_BASE_ADDR: u32 = 0x0800_0000;
pub const FIRMWARE_MIN_ADDR: u32 = 0x0800_2800;
pub const FLASH_END_ADDR: u32 = 0x0802_0000;
const FLASH_WAIT_TIMEOUT: u32 = 250_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlashError {
    ProtectedRange,
    WriteProtected,
    ProgramError,
    Timeout,
}

pub struct InternalFlash;

impl InternalFlash {
    /// Validate that `addr` targets user flash above the 10 KB bootloader and within 128 KB.
    #[inline(always)]
    fn validate_addr(addr: u32) -> Result<u32, FlashError> {
        let aligned = addr & !0xFF;
        if aligned < FIRMWARE_MIN_ADDR || aligned >= FLASH_END_ADDR {
            return Err(FlashError::ProtectedRange);
        }
        Ok(aligned)
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".data.ramfunc"))]
    #[inline(never)]
    unsafe fn wait_flash_done_ram() -> Result<(), FlashError> {
        unsafe {
            let mut t = FLASH_WAIT_TIMEOUT;
            while (FLASH_SR.read_volatile() & SR_BSY) != 0 {
                t -= 1;
                if t == 0 {
                    return Err(FlashError::Timeout);
                }
            }
            let sr = FLASH_SR.read_volatile();
            if (sr & SR_WRPERR) != 0 {
                FLASH_SR.write_volatile(SR_WRPERR);
                return Err(FlashError::WriteProtected);
            }
            if (sr & SR_PGERR) != 0 {
                FLASH_SR.write_volatile(SR_PGERR);
                return Err(FlashError::ProgramError);
            }
            let mut eop_t = FLASH_WAIT_TIMEOUT;
            while (FLASH_SR.read_volatile() & SR_EOP) == 0 {
                eop_t -= 1;
                if eop_t == 0 {
                    break;
                }
            }
            FLASH_SR.write_volatile(SR_EOP);
            Ok(())
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".data.ramfunc"))]
    #[inline(never)]
    unsafe fn erase_page_ram(aligned: u32) -> Result<(), FlashError> {
        unsafe {
            let mut t = FLASH_WAIT_TIMEOUT;
            while (FLASH_SR.read_volatile() & SR_BSY) != 0 {
                t -= 1;
                if t == 0 {
                    return Err(FlashError::Timeout);
                }
            }
            if (FLASH_CR.read_volatile() & CR_LOCK) != 0 {
                FLASH_KEYR.write_volatile(KEY1);
                FLASH_KEYR.write_volatile(KEY2);
            }
            FLASH_SR.write_volatile(SR_EOP | SR_WRPERR | SR_PGERR);
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_PER | CR_EOPIE);
            (aligned as *mut u32).write_volatile(0xFFFF_FFFF);
            let res = Self::wait_flash_done_ram();
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() & !(CR_PER | CR_EOPIE));
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_LOCK);
            res
        }
    }

    #[cfg_attr(target_os = "none", unsafe(link_section = ".data.ramfunc"))]
    #[inline(never)]
    unsafe fn program_page_ram(
        aligned: u32,
        data: &[u8; FLASH_PAGE_SIZE],
    ) -> Result<(), FlashError> {
        unsafe {
            let mut t = FLASH_WAIT_TIMEOUT;
            while (FLASH_SR.read_volatile() & SR_BSY) != 0 {
                t -= 1;
                if t == 0 {
                    return Err(FlashError::Timeout);
                }
            }
            if (FLASH_CR.read_volatile() & CR_LOCK) != 0 {
                FLASH_KEYR.write_volatile(KEY1);
                FLASH_KEYR.write_volatile(KEY2);
            }
            FLASH_SR.write_volatile(SR_EOP | SR_WRPERR | SR_PGERR);
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_PG | CR_EOPIE);

            let dest = aligned as *mut u32;
            let mut i = 0usize;
            while i < 63 {
                let off = i * 4;
                let w =
                    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
                dest.add(i).write_volatile(w);
                i += 1;
            }
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_PGSTRT);
            let last_w = u32::from_le_bytes([data[252], data[253], data[254], data[255]]);
            dest.add(63).write_volatile(last_w);

            let res = Self::wait_flash_done_ram();
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() & !(CR_PG | CR_EOPIE));
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_LOCK);
            res
        }
    }

    /// Unlocks flash control registers for write/erase operations.
    pub fn unlock() {
        unsafe {
            if (FLASH_CR.read_volatile() & CR_LOCK) != 0 {
                FLASH_KEYR.write_volatile(KEY1);
                FLASH_KEYR.write_volatile(KEY2);
            }
        }
    }

    /// Locks flash control registers against accidental mutation.
    #[inline(always)]
    pub fn lock() {
        unsafe {
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_LOCK);
        }
    }

    /// Checks if a 256-byte page at `addr` contains any non-0xFF bytes requiring an erase.
    pub fn page_needs_erase(addr: u32) -> bool {
        let ptr = (addr & !0xFF) as *const u32;
        for i in 0..64 {
            if unsafe { ptr.add(i).read_volatile() } != 0xFFFF_FFFF {
                return true;
            }
        }
        false
    }

    /// Erases a 256-byte page of flash memory at `addr` (executing from SRAM inside IRQ-free critical section).
    pub fn erase_page(addr: u32) -> Result<(), FlashError> {
        let aligned = Self::validate_addr(addr)?;
        cortex_m::interrupt::free(|_| unsafe { Self::erase_page_ram(aligned) })
    }

    /// Programs a complete 256-byte page of flash memory at `addr`, erasing first if needed.
    pub fn program_page(addr: u32, data: &[u8; FLASH_PAGE_SIZE]) -> Result<(), FlashError> {
        let aligned = Self::validate_addr(addr)?;
        cortex_m::interrupt::free(|_| unsafe {
            if Self::page_needs_erase(aligned) {
                Self::erase_page_ram(aligned)?;
            }
            Self::program_page_ram(aligned, data)
        })
    }

    /// Reads raw 32-bit `FLASH_OPTR` register from Flash controller.
    #[inline(always)]
    pub fn raw_optr() -> u32 {
        unsafe { pac::flash::FLASH_OPTR.read_volatile() }
    }

    /// Reads raw 32-bit `FLASH_WRPR` write protection register.
    #[inline(always)]
    pub fn raw_wrpr() -> u32 {
        unsafe { pac::flash::FLASH_WRPR.read_volatile() }
    }

    /// Reads raw 32-bit `FLASH_BORCR` brown-out reset configuration register.
    #[inline(always)]
    pub fn raw_borcr() -> u32 {
        unsafe { pac::flash::FLASH_BORCR.read_volatile() }
    }

    /// Reads the raw 8-bit Flash Read Protection (RDP) byte from `FLASH_OPTR` (0xAA = Level 0 / Open, 0x55 = Level 1 / Locked).
    #[inline(always)]
    pub fn rdp_level() -> u8 {
        (Self::raw_optr() & pac::flash::FLASH_OPTR_RDP_MASK) as u8
    }

    /// Decodes the active Flash Read Protection (RDP) status into a strongly-typed [`RdpLevel`].
    #[inline(always)]
    pub fn rdp_status() -> RdpLevel {
        match Self::rdp_level() {
            pac::flash::FLASH_OPTR_RDP_LEVEL_0 => RdpLevel::Level0,
            pac::flash::FLASH_OPTR_RDP_LEVEL_1 => RdpLevel::Level1,
            other => RdpLevel::Custom(other),
        }
    }

    /// Returns `true` if Flash Readout Protection (RDP) is currently active.
    /// In PY32F071 hardware, any value other than `0xAA` enforces Level 1 protection (SWD debugger locked out of Flash).
    #[inline(always)]
    pub fn is_rdp_protected() -> bool {
        Self::rdp_status().is_protected()
    }

    /// Snapshot the MCU Option Bytes and Flash security configurations.
    pub fn read_option_bytes() -> FlashOptionBytes {
        let optr = Self::raw_optr();
        let wrpr = Self::raw_wrpr();
        let borcr = Self::raw_borcr();
        let raw_rdp = (optr & pac::flash::FLASH_OPTR_RDP_MASK) as u8;

        FlashOptionBytes {
            rdp: match raw_rdp {
                pac::flash::FLASH_OPTR_RDP_LEVEL_0 => RdpLevel::Level0,
                pac::flash::FLASH_OPTR_RDP_LEVEL_1 => RdpLevel::Level1,
                other => RdpLevel::Custom(other),
            },
            rdp_raw: raw_rdp,
            iwdg_sw: (optr & pac::flash::FLASH_OPTR_IWDG_SW) != 0,
            wwdg_sw: (optr & pac::flash::FLASH_OPTR_WWDG_SW) != 0,
            nrst_as_gpio: (optr & pac::flash::FLASH_OPTR_NRST_MODE) != 0,
            nboot1: (optr & pac::flash::FLASH_OPTR_NBOOT1) != 0,
            iwdg_stop_active: (optr & pac::flash::FLASH_OPTR_IWDG_STOP) != 0,
            wrp_mask: (wrpr & 0xFFFF) as u16,
            bor_raw: (borcr & 0xFF) as u8,
        }
    }
}

/// Strongly-typed Flash Readout Protection (RDP) status for PY32F071.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdpLevel {
    /// Level 0: No Protection (0xAA). Full SWD/JTAG debug access to Flash and SRAM.
    Level0,
    /// Level 1: Readout Protection Active (0x55). Debugger is locked out of Flash.
    Level1,
    /// Custom non-standard byte (treated as Level 1 protection by hardware).
    Custom(u8),
}

impl RdpLevel {
    /// Returns `true` if Readout Protection is active and debugger is locked out of Flash.
    #[inline(always)]
    pub const fn is_protected(&self) -> bool {
        match self {
            RdpLevel::Level0 => false,
            RdpLevel::Level1 | RdpLevel::Custom(_) => true,
        }
    }

    /// User-friendly descriptive label.
    pub const fn as_str(&self) -> &'static str {
        match self {
            RdpLevel::Level0 => "Level 0 (Open)",
            RdpLevel::Level1 => "Level 1 (Locked)",
            RdpLevel::Custom(_) => "Custom (Locked)",
        }
    }

    /// Short compact string for UI badges and menus.
    pub const fn short_str(&self) -> &'static str {
        match self {
            RdpLevel::Level0 => "Open",
            RdpLevel::Level1 => "Locked",
            RdpLevel::Custom(_) => "Custom",
        }
    }
}

/// Complete Option Bytes snapshot from MCU hardware registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlashOptionBytes {
    pub rdp: RdpLevel,
    pub rdp_raw: u8,
    pub iwdg_sw: bool,
    pub wwdg_sw: bool,
    pub nrst_as_gpio: bool,
    pub nboot1: bool,
    pub iwdg_stop_active: bool,
    pub wrp_mask: u16,
    pub bor_raw: u8,
}
