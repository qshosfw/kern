//! On-Chip Flash Memory Programming & Page Erase Controller for PY32F071.
//!
//! Page Size: 256 bytes (64 words).
//! Sector Size: 512 bytes (2 pages).
//! Base Address: 0x0800_0000 (up to 128 KB).
//!
//! Sourced from PY32F071 reference manual & inspo/ichi/src/internal_flash.c.

pub const FLASH_PAGE_SIZE: usize = 256;
pub const FLASH_BASE_ADDR: u32 = 0x0800_0000;

const FLASH_REG_BASE: usize = 0x4002_2000;
#[allow(dead_code)]
const FLASH_ACR: *mut u32 = FLASH_REG_BASE as *mut u32;
const FLASH_KEYR: *mut u32 = (FLASH_REG_BASE + 0x08) as *mut u32;
#[allow(dead_code)]
const FLASH_OPTKEYR: *mut u32 = (FLASH_REG_BASE + 0x0C) as *mut u32;
const FLASH_SR: *mut u32 = (FLASH_REG_BASE + 0x10) as *mut u32;
const FLASH_CR: *mut u32 = (FLASH_REG_BASE + 0x14) as *mut u32;

const KEY1: u32 = 0x4567_0123;
const KEY2: u32 = 0xCDEF_89AB;

const SR_BSY: u32 = 1 << 0;
const SR_EOP: u32 = 1 << 1;
#[allow(dead_code)]
const SR_WRPERR: u32 = 1 << 4;
#[allow(dead_code)]
const SR_PGERR: u32 = 1 << 5;

const CR_PG: u32 = 1 << 0;
const CR_PER: u32 = 1 << 1;
#[allow(dead_code)]
const CR_MER: u32 = 1 << 2;
const CR_PGSTRT: u32 = 1 << 16;
const CR_EOPIE: u32 = 1 << 24;
const CR_LOCK: u32 = 1 << 31;

pub struct InternalFlash;

impl InternalFlash {
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

    #[inline(always)]
    fn wait_busy() {
        unsafe { while (FLASH_SR.read_volatile() & SR_BSY) != 0 {} }
    }

    #[inline(always)]
    fn wait_eop() {
        unsafe {
            while (FLASH_SR.read_volatile() & SR_EOP) == 0 {}
            FLASH_SR.write_volatile(SR_EOP);
        }
    }

    /// Checks if a 256-byte page at  contains any non-0xFF bytes requiring an erase.
    pub fn page_needs_erase(addr: u32) -> bool {
        let ptr = addr as *const u32;
        for i in 0..64 {
            if unsafe { ptr.add(i).read_volatile() } != 0xFFFF_FFFF {
                return true;
            }
        }
        false
    }

    /// Erases a 256-byte page of flash memory at .
    ///  must be page-aligned (multiple of 256).
    pub fn erase_page(addr: u32) {
        Self::wait_busy();
        Self::unlock();

        unsafe {
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_PER | CR_EOPIE);
            let target_ptr = (addr & !0xFF) as *mut u32;
            target_ptr.write_volatile(0xFFFF_FFFF);
        }

        Self::wait_busy();
        Self::wait_eop();

        unsafe {
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() & !(CR_PER | CR_EOPIE));
        }
        Self::lock();
    }

    /// Programs a complete 256-byte page of flash memory at .
    /// Automatically performs a page erase first if needed.
    pub fn program_page(addr: u32, data: &[u8; FLASH_PAGE_SIZE]) {
        if Self::page_needs_erase(addr) {
            Self::erase_page(addr);
        }

        Self::wait_busy();
        Self::unlock();

        unsafe {
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_PG | CR_EOPIE);

            let dest = addr as *mut u32;
            let mut offset = 0;

            // Write first 63 words
            for i in 0..63 {
                let w = u32::from_le_bytes([
                    data[offset],
                    data[offset + 1],
                    data[offset + 2],
                    data[offset + 3],
                ]);
                dest.add(i).write_volatile(w);
                offset += 4;
            }

            // Trigger Page Program Start
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() | CR_PGSTRT);

            // Write final 64th word
            let last_w = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            dest.add(63).write_volatile(last_w);
        }

        Self::wait_busy();
        Self::wait_eop();

        unsafe {
            FLASH_CR.write_volatile(FLASH_CR.read_volatile() & !(CR_PG | CR_EOPIE));
        }
        Self::lock();
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
