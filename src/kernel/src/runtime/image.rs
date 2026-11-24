//! KRX (KERN Realtime eXecutable) Binary Container & Header Parser
//!
//! Normative implementation of KRX-ABI 2.5.0 container format and section headers.
//! Provides zero-allocation, explicit little-endian parsing safe for ARM Cortex-M0+.

use ::abi::CapabilitySet;

pub const KRX_IDENT: [u8; 4] = [0xC0, 0xDE, 0x01, 0xFE];
pub const BOOT_IDENT: [u8; 4] = [0xC0, 0xDE, 0x02, 0xFD];
pub const CALIB_IDENT: [u8; 4] = [0xC0, 0xDE, 0x03, 0xFC];
pub const SLAB_IDENT: [u8; 4] = [0xC0, 0xDE, 0x04, 0xFB];
pub const SWAP_IDENT: [u8; 4] = [0xC0, 0xDE, 0x05, 0xFA];
pub const KRX_PCB_IDENT: [u8; 4] = [0xC0, 0xDE, 0x06, 0xF9];
pub const KERNFS_IDENT: [u8; 4] = [0xC0, 0xDE, 0x07, 0xF8];
pub const SIG_IDENT: [u8; 4] = [0xC0, 0xDE, 0x08, 0xF7];

/// Execution class: how the payload executes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KrxClass {
    Native = 0,
    Paged = 1,
}

impl KrxClass {
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Native),
            1 => Some(Self::Paged),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        const NAMES: &[u8] = b"Native\0Paged\0";
        utils::format::nth_cstr(NAMES, *self as u8)
    }
}

/// KRX Execution Profile & Executable Memory Paradigm taxonomy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KrxProfile {
    /// N0: Native-Fixed (trusted native, fixed execution VMA, zero relocation overhead, hot DSP)
    N0NativeFixed = 0,
    /// P0: Structured Paged (trusted native, resident root + leaf overlays with continuation dispatch)
    P0StructuredPaged = 1,
    /// N1: Native-PIC (trusted native, dynamic PIC with GOT base r9)
    N1NativePic = 2,
    /// P1: Transparent Paged (trusted native, demand-paged fixed window with LR-thunking shadow return stack)
    P1TransparentPaged = 3,
}

impl KrxProfile {
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::N0NativeFixed),
            1 => Some(Self::P0StructuredPaged),
            2 => Some(Self::N1NativePic),
            3 => Some(Self::P1TransparentPaged),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        const NAMES: &[u8] =
            b"N0 (Native-Fixed)\0P0 (Structured-Paged)\0N1 (Native-PIC)\0P1 (Transparent-Paged)\0";
        utils::format::nth_cstr(NAMES, *self as u8)
    }

    #[inline(always)]
    pub const fn is_trusted_native(&self) -> bool {
        true
    }

    #[inline(always)]
    pub const fn is_structured_paged(&self) -> bool {
        matches!(self, Self::P0StructuredPaged)
    }

    #[inline(always)]
    pub const fn uses_fixed_vma(&self) -> bool {
        matches!(self, Self::N0NativeFixed | Self::P0StructuredPaged)
    }

    #[inline(always)]
    pub const fn raw_code(&self) -> u8 {
        0x10 + (*self as u8)
    }

    #[inline(always)]
    pub const fn from_raw_code(code: u8) -> Option<Self> {
        Self::from_u8(code.wrapping_sub(0x10))
    }

    /// Automatically infers the normative execution profile from package metadata.
    pub const fn detect(
        preamble: &KrxPreamble,
        exec: &SectionExecution,
        _has_block_map: bool,
        has_leaf_map: bool,
    ) -> Self {
        if let Some(p) = exec.profile() {
            return p;
        }
        match preamble.execution_class() {
            Some(KrxClass::Paged) => {
                if has_leaf_map {
                    Self::P0StructuredPaged
                } else {
                    Self::P1TransparentPaged
                }
            }
            Some(KrxClass::Native) | None => {
                if exec.reloc_count == 0 && preamble.link_base != 0 {
                    Self::N0NativeFixed
                } else {
                    Self::N1NativePic
                }
            }
        }
    }
}

/// Execution Type (KRX-ABI 2.5.0 Section 5.2 & Chapter 11)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum KrxExecType {
    App = 1,
    Service = 2,
    Daemon = 3,
    Driver = 4,
    Module = 5,
}

impl KrxExecType {
    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if val >= 1 && val <= 5 {
            Some(unsafe { core::mem::transmute::<u8, Self>(val) })
        } else {
            None
        }
    }

    pub const fn as_str(&self) -> &'static str {
        const NAMES: &[u8] = b"App\0Service\0Daemon\0Driver\0Module\0";
        utils::format::nth_cstr(NAMES, (*self as u8) - 1)
    }

    pub const fn is_app(&self) -> bool {
        matches!(self, Self::App)
    }

    pub const fn is_service_or_daemon(&self) -> bool {
        matches!(
            self,
            Self::Service | Self::Daemon | Self::Driver | Self::Module
        )
    }

    pub const fn default_priority(&self) -> crate::sched::Priority {
        match self {
            Self::App => crate::sched::Priority::Ui,
            Self::Service | Self::Driver => crate::sched::Priority::Radio,
            Self::Daemon | Self::Module => crate::sched::Priority::Idle,
        }
    }
}

/// Normative KRX Binary Launch and Verification Errors (§12, KRX-ABI 2.5.0 Appendix C).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KrxLaunchError {
    FileNotFound,
    ReadFailed,
    InvalidPreamble,
    CrcMismatch,
    MissingIdentity,
    MissingExecution,
    CapabilityDenied(Option<::abi::Cap>),
    InsufficientRam,
    InsufficientStack,
    RelocationFailed,
    ImportBindingFailed,
    PayloadTooLarge,
    ExecutionFailed,
    InvalidEntryPoint,
    TruncatedPackage,
    UnsupportedProfile,
}

// Human-language explanation for each `KrxLaunchError`, indexed by
// `KrxLaunchError::index`. One table serves the on-device dialog and host
// diagnostics so wording never drifts between surfaces.
utils::static_bytes!(
    static LAUNCH_ERR_HELP = b"\
Not installed.\nSync via Kern Dock.\0\
Storage read failed.\0\
Corrupt package.\nReinstall in Dock.\0\
CRC mismatch.\nReinstall in Dock.\0\
Missing metadata.\0\
Missing code.\0\
Unsupported cap.\0\
Exceeds 6KB RAM.\0\
Low stack RAM.\0\
Relocation failed.\0\
ABI mismatch.\nUpdate app or OS.\0\
Exceeds 6KB limit.\0\
Start failed.\0\
Invalid entry.\0\
Incomplete package.\nReinstall in Dock.\0\
Unsupported profile.\0";
);

impl KrxLaunchError {
    #[inline(never)]
    pub const fn index(&self) -> u8 {
        match self {
            Self::FileNotFound => 0,
            Self::ReadFailed => 1,
            Self::InvalidPreamble => 2,
            Self::CrcMismatch => 3,
            Self::MissingIdentity => 4,
            Self::MissingExecution => 5,
            Self::CapabilityDenied(_) => 6,
            Self::InsufficientRam => 7,
            Self::InsufficientStack => 8,
            Self::RelocationFailed => 9,
            Self::ImportBindingFailed => 10,
            Self::PayloadTooLarge => 11,
            Self::ExecutionFailed => 12,
            Self::InvalidEntryPoint => 13,
            Self::TruncatedPackage => 14,
            Self::UnsupportedProfile => 15,
        }
    }

    /// Plain-language explanation suitable for a dialog body.
    pub const fn describe(&self) -> &'static str {
        utils::format::nth_cstr(&LAUNCH_ERR_HELP, self.index())
    }
}

/// KRX Preamble (24 bytes, little-endian, non-ASCII format identifier)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct KrxPreamble {
    pub ident: [u8; 4],
    pub app_type: u8,
    pub format_version: u8,
    pub section_count: u16,
    pub header_len: u16,
    pub flags: u16,
    pub payload_crc32: u32,
    pub header_crc32: u32,
    pub link_base: u32,
}

const _: () = assert!(core::mem::size_of::<KrxPreamble>() == 24);

impl KrxPreamble {
    pub const SIZE: usize = 24;
    pub const MAX_HEADER_SIZE: usize = 768;

    /// Parse preamble directly into `out` from a little-endian byte slice.
    pub fn parse_into(buf: &[u8], out: &mut Self) -> Result<(), ()> {
        if buf.len() < Self::SIZE {
            return Err(());
        }
        #[cfg(target_endian = "little")]
        unsafe {
            core::ptr::copy_nonoverlapping(
                buf.as_ptr(),
                (out as *mut Self).cast::<u8>(),
                Self::SIZE,
            );
        }
        #[cfg(not(target_endian = "little"))]
        {
            *out = Self {
                ident: [buf[0], buf[1], buf[2], buf[3]],
                app_type: buf[4],
                format_version: buf[5],
                section_count: u16::from_le_bytes([buf[6], buf[7]]),
                header_len: u16::from_le_bytes([buf[8], buf[9]]),
                flags: u16::from_le_bytes([buf[10], buf[11]]),
                payload_crc32: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
                header_crc32: u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]),
                link_base: u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]]),
            };
        }
        if u32::from_ne_bytes(out.ident) != u32::from_ne_bytes(KRX_IDENT)
            || out.format_version.wrapping_sub(1) > 4
        {
            return Err(());
        }
        Ok(())
    }

    /// Parse preamble from little-endian byte slice
    pub fn parse(buf: &[u8]) -> Result<Self, ()> {
        let mut p = core::mem::MaybeUninit::<Self>::uninit();
        Self::parse_into(buf, unsafe { &mut *p.as_mut_ptr() })?;
        Ok(unsafe { p.assume_init() })
    }

    #[inline(always)]
    pub const fn execution_class(&self) -> Option<KrxClass> {
        KrxClass::from_u8(self.app_type)
    }
}

const _: () = assert!(core::mem::size_of::<KrxPreamble>() == KrxPreamble::SIZE);

/// Standard KRX Section IDs
pub const SECTION_IDENTITY: u16 = 0x0001;
pub const SECTION_EXECUTION: u16 = 0x0002;
pub const SECTION_CAP_IMPORTS: u16 = 0x0003;
pub const SECTION_ASSETS: u16 = 0x0004;

/// Self-contained package icon asset kinds inside `SECTION_ASSETS` (`0x0004`).
pub const ASSET_ICON_8: u8 = 1;
pub const ASSET_ICON_16: u8 = 2;
pub const ASSET_ICON_TRAY: u8 = 3;
pub const SECTION_BLOCK_MAP: u16 = 0x0005;
pub const SECTION_SERVICE_EXPORTS: u16 = 0x0006;
pub const SECTION_CONFIG: u16 = 0x0007;
pub const SECTION_SEAL: u16 = 0x0008;
pub const SECTION_TYPE_LIFECYCLE: u16 = 0x0009;
pub const SECTION_LEAF_MAP: u16 = 0x000A;
pub const SECTION_ACTIONS: u16 = 0x000B;

/// Standard 4-byte Section Header prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct SectionHeader {
    pub id: u16,
    pub length: u16, // Covers header + body, 4-aligned
}

impl SectionHeader {
    pub const SIZE: usize = 4;

    #[inline(never)]
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let id = u16::from_le_bytes([buf[0], buf[1]]);
        let length = u16::from_le_bytes([buf[2], buf[3]]);
        Some(Self { id, length })
    }
}

/// Zero-allocation iterator over sections in a KRX header
pub struct KrxSectionIterator<'a> {
    slice: &'a [u8],
    offset: usize,
    end: usize,
}

impl<'a> KrxSectionIterator<'a> {
    pub fn new(buf: &'a [u8], header_len: usize) -> Self {
        let end = header_len.min(buf.len());
        Self {
            slice: buf,
            offset: KrxPreamble::SIZE,
            end,
        }
    }
}

impl<'a> Iterator for KrxSectionIterator<'a> {
    type Item = (SectionHeader, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset + SectionHeader::SIZE > self.end {
            return None;
        }
        let hdr = SectionHeader::parse(&self.slice[self.offset..])?;
        let len = hdr.length as usize;
        if len < SectionHeader::SIZE || self.offset + len > self.end {
            return None;
        }
        let body = &self.slice[self.offset + SectionHeader::SIZE..self.offset + len];
        self.offset += (len + 3) & !3; // 4-byte align
        Some((hdr, body))
    }
}

/// Locate a section body within a KRX header buffer
pub fn find_section<'a>(buf: &'a [u8], header_len: usize, target_id: u16) -> Option<&'a [u8]> {
    for (hdr, body) in KrxSectionIterator::new(buf, header_len) {
        if hdr.id == target_id {
            return Some(body);
        }
    }
    None
}

/// Identity Section (0x0001, 40 bytes normative, naturally aligned in RAM)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SectionIdentity {
    pub build_timestamp: u32,
    pub name_str: u16,
    pub min_kernel_abi: u16,
    pub icon_asset: u16,
    pub flags: u16,
    pub version: [u8; 3],
    pub author: [u8; 12],
    pub reserved: [u8; 13],
}

const _: () = {
    assert!(core::mem::size_of::<SectionIdentity>() == 40);
    assert!(core::mem::align_of::<SectionIdentity>() == 4);
};

impl SectionIdentity {
    pub const SIZE: usize = 40;

    #[inline(never)]
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let name_str = u16::from_le_bytes([buf[0], buf[1]]);
        let version = [buf[2], buf[3], buf[4]];
        let min_kernel_abi = u16::from_le_bytes([buf[5], buf[6]]);
        let mut author = [0u8; 12];
        author.copy_from_slice(&buf[7..19]);
        let build_timestamp = u32::from_le_bytes([buf[19], buf[20], buf[21], buf[22]]);
        let icon_asset = u16::from_le_bytes([buf[23], buf[24]]);
        let flags = u16::from_le_bytes([buf[25], buf[26]]);
        let mut reserved = [0u8; 13];
        reserved.copy_from_slice(&buf[27..40]);

        Some(Self {
            name_str,
            version,
            min_kernel_abi,
            author,
            build_timestamp,
            icon_asset,
            flags,
            reserved,
        })
    }

    /// Extract string-pool display name if offset is valid within the header buffer
    pub fn name<'a>(&self, header_buf: &'a [u8]) -> Option<&'a str> {
        let offset = self.name_str as usize;
        if offset < KrxPreamble::SIZE || offset >= header_buf.len() {
            return None;
        }
        let mut end = offset;
        while end < header_buf.len() && header_buf[end] != 0 && (end - offset) < 32 {
            end += 1;
        }
        if end > offset {
            utils::format::ascii_str(&header_buf[offset..end])
        } else {
            None
        }
    }
}

/// Execution Manifest Section (0x0002, 28 bytes normative)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SectionExecution {
    pub min_stack_bytes: u16,
    pub min_heap_bytes: u16,
    pub page_size: u16,
    pub cache_blocks: u8,
    pub dma_burst_size: u8,
    pub entry_point: u32,
    pub payload_size: u32,
    pub reloc_offset: u16,
    pub reloc_count: u16,
    pub got_offset: u16,
    pub rt_pin_mask: u16,
    pub max_fault_latency_us: u16,
    pub reserved: [u8; 2],
}

const _: () = assert!(core::mem::size_of::<SectionExecution>() == 28);

impl SectionExecution {
    pub const SIZE: usize = 28;

    pub fn parse(buf: &[u8]) -> Option<Self> {
        let mut out = None;
        Self::parse_into(buf, &mut out);
        out
    }

    #[inline(always)]
    pub fn parse_into(buf: &[u8], dst: &mut Option<Self>) {
        if buf.len() < Self::SIZE {
            *dst = None;
            return;
        }
        let mut val = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), val.as_mut_ptr().cast::<u8>(), Self::SIZE);
            *dst = Some(val.assume_init());
        }
    }

    /// Check whether this section can safely execute given available system RAM and required stack safety margin.
    pub fn can_execute(&self, available_ram: usize, stack_margin: usize) -> bool {
        let total_required = (self.min_stack_bytes as usize)
            .saturating_add(self.min_heap_bytes as usize)
            .saturating_add(stack_margin);
        available_ram >= total_required
    }

    /// Explicitly requested execution profile if specified in reserved header bytes (0 = unspecified / autodetect).
    #[inline(always)]
    pub const fn profile(&self) -> Option<KrxProfile> {
        KrxProfile::from_raw_code(self.reserved[0])
    }
}

/// CapBinding Record (6 or 8 bytes on flash, naturally aligned in RAM)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct CapBinding {
    pub interface_id: u16,
    pub min_abi: u16,
    pub flags: u16,
    pub slot_idx: u16,
}

const _: () = {
    assert!(core::mem::size_of::<CapBinding>() == 8);
    assert!(core::mem::align_of::<CapBinding>() == 2);
};

impl CapBinding {
    pub const SIZE: usize = 6;
    pub const SIZE_WITH_SLOT: usize = 8;
    pub const FLAG_MANDATORY: u16 = 1 << 0;
    pub const FLAG_OPTIONAL: u16 = 1 << 1;
    pub const FLAG_EXPORT: u16 = 1 << 2;
    pub const FLAG_PIN_PROVIDER: u16 = 1 << 3;

    /// Parse a `CapBinding` record from little-endian bytes (6-byte or 8-byte record).
    #[inline(never)]
    pub fn parse(buf: &[u8], default_slot_idx: u16) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let interface_id = u16::from_le_bytes([buf[0], buf[1]]);
        let min_abi = u16::from_le_bytes([buf[2], buf[3]]);
        let flags = u16::from_le_bytes([buf[4], buf[5]]);
        let slot_idx = if buf.len() >= Self::SIZE_WITH_SLOT {
            u16::from_le_bytes([buf[6], buf[7]])
        } else {
            default_slot_idx
        };
        Some(Self {
            interface_id,
            min_abi,
            flags,
            slot_idx,
        })
    }
}

/// Hardware Claim Descriptor for Drivers (4 bytes, Section 5.3 normative)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct HwClaim {
    pub kind: u8, // 0: PIN, 1: IRQ, 2: DMA_CH, 3: BUS
    pub id: u8,
    pub flags: u8,
    pub reserved: u8,
}

/// Type & Lifecycle Section (0x0009, 12 bytes base header)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SectionType {
    pub exec_type: u8, // 1: APP, 2: SERVICE, 3: DAEMON, 4: DRIVER, 5: MOD
    pub flags: u8,     // b0 STARTUP, b1 PERSISTENT, b2 RESTART_ON_FAIL, b3 SINGLETON
    pub stack_cost: u8,
    pub restart_budget: u8,
    pub depends_count: u8,
    pub provides_count: u8,
    pub hw_claim_count: u8,
    pub restart_policy: u8,
    pub cap_mask: u32,
}

const _: () = assert!(core::mem::size_of::<SectionType>() == 12);

impl SectionType {
    pub const SIZE: usize = 12;
    pub const EXEC_APP: u8 = 0x01;
    pub const EXEC_SERVICE: u8 = 0x02;
    pub const EXEC_DAEMON: u8 = 0x03;
    pub const EXEC_DRIVER: u8 = 0x04;
    pub const EXEC_MODULE: u8 = 0x05;

    pub const FLAG_STARTUP: u8 = 1 << 0;
    pub const FLAG_PERSISTENT: u8 = 1 << 1;
    pub const FLAG_RESTART_ON_FAIL: u8 = 1 << 2;
    pub const FLAG_SINGLETON: u8 = 1 << 3;
    pub const FLAG_AUTO_UPDATE_OK: u8 = 1 << 4;
    pub const FLAG_RTC: u8 = 1 << 5;

    pub fn exec_type(&self) -> Option<KrxExecType> {
        KrxExecType::from_u8(self.exec_type)
    }

    #[inline(never)]
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            exec_type: buf[0],
            flags: buf[1],
            stack_cost: buf[2],
            restart_budget: buf[3],
            depends_count: buf[4],
            provides_count: buf[5],
            hw_claim_count: buf[6],
            restart_policy: buf[7],
            cap_mask: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
        })
    }

    #[inline(always)]
    pub fn capabilities(&self) -> ::abi::CapabilitySet {
        ::abi::CapabilitySet::from_raw(self.cap_mask)
    }

    #[inline(always)]
    pub fn set_capabilities(&mut self, caps: ::abi::CapabilitySet) {
        self.cap_mask = caps.raw();
    }
}

/// Compact, zero-allocation metadata summary extracted from a KRX package header.
#[derive(Debug, Clone, Copy, Default)]
pub struct KrxAuxSections<'a> {
    pub block_map: Option<&'a [u8]>,
    pub leaf_map: Option<&'a [u8]>,
    pub service_exports: Option<&'a [u8]>,
    pub config: Option<&'a [u8]>,
    pub actions: Option<&'a [u8]>,
}

/// Declarative App Config Header (`SECTION_CONFIG = 0x0007`, 16 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxConfigHeader {
    pub version: u16,
    pub item_count: u8,
    pub cfg_bytes: u8,
    pub schema_crc: u32,
    pub reserved: [u8; 8],
}

impl KrxConfigHeader {
    pub const SIZE: usize = 16;

    pub fn parse(body: &[u8]) -> Option<Self> {
        if body.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            version: u16::from_le_bytes([body[0], body[1]]),
            item_count: body[2],
            cfg_bytes: body[3],
            schema_crc: u32::from_le_bytes([body[4], body[5], body[6], body[7]]),
            reserved: [0; 8],
        })
    }
}

const _: () = assert!(core::mem::size_of::<KrxConfigHeader>() == 16);

pub const CFG_KIND_TOGGLE: u8 = 0;
pub const CFG_KIND_CHOICE: u8 = 1;
pub const CFG_KIND_RANGE: u8 = 2;

/// Declarative App Config Field Descriptor (`SECTION_CONFIG = 0x0007`, 16 bytes per field)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxConfigItem {
    pub offset: u8,
    pub kind: u8,
    pub flags: u8,
    pub step: u8,
    pub default_v: i16,
    pub min_v: i16,
    pub max_v: i16,
    pub label_off: u16,
    pub aux_off: u16,
    pub reserved: u16,
}

impl KrxConfigItem {
    pub const SIZE: usize = 16;

    pub fn parse(body: &[u8], idx: usize) -> Option<Self> {
        let off = (idx + 1) << 4;
        if off + Self::SIZE > body.len() {
            return None;
        }
        let s = &body[off..off + Self::SIZE];
        Some(Self {
            offset: s[0],
            kind: s[1],
            flags: s[2],
            step: s[3],
            default_v: i16::from_le_bytes([s[4], s[5]]),
            min_v: i16::from_le_bytes([s[6], s[7]]),
            max_v: i16::from_le_bytes([s[8], s[9]]),
            label_off: u16::from_le_bytes([s[10], s[11]]),
            aux_off: u16::from_le_bytes([s[12], s[13]]),
            reserved: 0,
        })
    }
}

const _: () = assert!(core::mem::size_of::<KrxConfigItem>() == 16);

/// Declared App Action Entry (`SECTION_ACTIONS = 0x000B`, 16 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxActionDecl {
    /// App-local action ID passed to `entry(event)` via launch args (0 = default launch)
    pub action_id: u16,
    /// Scope mask of `abi::scope` bits: where the OS surfaces this action
    pub scope_mask: u8,
    pub flags: u8,
    /// ASCII NUL-terminated label
    pub label: [u8; 12],
}

impl KrxActionDecl {
    pub const SIZE: usize = 16;

    #[inline(always)]
    pub const fn count(body: &[u8]) -> usize {
        body.len() >> 4
    }

    pub fn parse(body: &[u8], idx: usize) -> Option<Self> {
        let off = idx << 4;
        if off + Self::SIZE > body.len() {
            return None;
        }
        let s = &body[off..off + Self::SIZE];
        let mut label = [0u8; 12];
        label.copy_from_slice(&s[4..16]);
        Some(Self {
            action_id: u16::from_le_bytes([s[0], s[1]]),
            scope_mask: s[2],
            flags: s[3],
            label,
        })
    }

    pub fn label_str(&self) -> &str {
        let len = self.label.iter().position(|&b| b == 0).unwrap_or(12);
        utils::format::ascii_str(&self.label[..len]).unwrap_or("")
    }
}

const _: () = assert!(core::mem::size_of::<KrxActionDecl>() == 16);

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct KrxSummary<'a> {
    // Hot scalars first (offsets 0..12) for 1-insn Cortex-M0+ Thumb-1 ldrb/ldrh.
    pub exec_type: KrxExecType,
    pub type_flags: u8,
    pub has_exec: bool,
    pub icon_id: u16,
    /// ABI version the package was built against (identity `min_kernel_abi`).
    pub abi: u16,
    /// Package semantic version `[major, minor, patch]` from `SECTION_IDENTITY`.
    pub version: [u8; 3],
    pub caps: ::abi::CapabilitySet,
    pub exec_raw: SectionExecution,
    pub preamble: KrxPreamble,
    pub title: Option<&'a str>,
    pub aux: KrxAuxSections<'a>,
}

impl<'a> KrxSummary<'a> {
    #[inline(always)]
    pub fn title(&self) -> &'a str {
        self.title.unwrap_or("")
    }

    #[inline(always)]
    pub fn exec(&self) -> Option<&SectionExecution> {
        self.has_exec.then_some(&self.exec_raw)
    }

    #[inline(always)]
    pub fn is_startup(&self) -> bool {
        (self.type_flags & SectionType::FLAG_STARTUP) != 0
    }

    #[inline(always)]
    pub fn is_persistent(&self) -> bool {
        (self.type_flags & SectionType::FLAG_PERSISTENT) != 0
    }

    #[inline(always)]
    pub fn is_singleton(&self) -> bool {
        (self.type_flags & SectionType::FLAG_SINGLETON) != 0
    }

    #[inline(always)]
    pub fn profile(&self, exec: &SectionExecution) -> KrxProfile {
        KrxProfile::detect(
            &self.preamble,
            exec,
            self.aux.block_map.is_some(),
            self.aux.leaf_map.is_some(),
        )
    }
}

impl<'a> Default for KrxSummary<'a> {
    fn default() -> Self {
        Self {
            preamble: KrxPreamble {
                ident: [0u8; 4],
                app_type: 0,
                format_version: 0,
                section_count: 0,
                header_len: 0,
                flags: 0,
                payload_crc32: 0,
                header_crc32: 0,
                link_base: 0,
            },
            title: None,
            icon_id: 0,
            abi: 0,
            version: [0; 3],
            exec_type: KrxExecType::App,
            type_flags: 0,
            has_exec: false,
            caps: ::abi::CapabilitySet::NONE,
            exec_raw: SectionExecution {
                min_stack_bytes: 0,
                min_heap_bytes: 0,
                page_size: 0,
                cache_blocks: 0,
                dma_burst_size: 0,
                entry_point: 0,
                payload_size: 0,
                reloc_offset: 0,
                reloc_count: 0,
                got_offset: 0,
                rt_pin_mask: 0,
                max_fault_latency_us: 0,
                reserved: [0; 2],
            },
            aux: KrxAuxSections {
                block_map: None,
                leaf_map: None,
                service_exports: None,
                config: None,
                actions: None,
            },
        }
    }
}

impl KrxPreamble {
    /// Read a NUL-terminated ASCII string from a parsed KRX header buffer at `off`.
    #[inline(always)]
    pub fn read_hdr_str(hdr: &[u8], off: u16) -> Option<&str> {
        Self::scan_hdr_str(hdr, off, false)
    }

    /// Read a `\0\0`-terminated packed Choice options slice (preserving internal `\0` separators).
    #[inline(always)]
    pub fn read_hdr_packed(hdr: &[u8], off: u16) -> Option<&str> {
        Self::scan_hdr_str(hdr, off, true)
    }

    #[inline(never)]
    fn scan_hdr_str(hdr: &[u8], off: u16, packed: bool) -> Option<&str> {
        let s = off as usize;
        if s < Self::SIZE || s >= hdr.len() {
            return None;
        }
        let max = if packed {
            hdr.len()
        } else {
            hdr.len().min(s + 32)
        };
        let mut i = s;
        let end = loop {
            if i >= max {
                if !packed {
                    break i;
                }
                return None;
            }
            if hdr[i] == 0 {
                if !packed {
                    break i;
                }
                if i + 1 < max && hdr[i + 1] == 0 {
                    break i + 1;
                }
            }
            i += 1;
        };
        if end == s {
            return None;
        }
        utils::format::ascii_str(&hdr[s..end])
    }

    /// Zero-allocation, single-pass inspection of a KRX package header buffer into uninitialized `out`.
    #[inline(never)]
    pub fn summarize_into<'a, 'b>(
        buf: &'a [u8],
        out: &'b mut core::mem::MaybeUninit<KrxSummary<'a>>,
    ) -> Result<&'b mut KrxSummary<'a>, KrxLaunchError> {
        let ptr = out.as_mut_ptr();
        unsafe { core::ptr::write_bytes(ptr, 0, 1) };
        let preamble = unsafe { &mut *core::ptr::addr_of_mut!((*ptr).preamble) };
        Self::parse_into(buf, preamble).map_err(|_| KrxLaunchError::InvalidPreamble)?;
        let h_len = preamble.header_len as usize;
        if h_len.wrapping_sub(Self::SIZE) > Self::MAX_HEADER_SIZE - Self::SIZE || (h_len & 3) != 0 {
            return Err(KrxLaunchError::InvalidPreamble);
        }
        let hdr = buf.get(..h_len).ok_or(KrxLaunchError::TruncatedPackage)?;

        // Hardened CRC32 verification of the header payload before populating `out`
        if preamble.header_crc32 != 0
            && crypto::crc::CrcEngine::crc32(&hdr[Self::SIZE..]) != preamble.header_crc32
        {
            return Err(KrxLaunchError::CrcMismatch);
        }

        let out = unsafe { &mut *ptr };
        out.exec_type = KrxExecType::App;

        let mut rest = &hdr[Self::SIZE..];
        while rest.len() >= SectionHeader::SIZE {
            let sec_id = u16::from_le_bytes([rest[0], rest[1]]);
            let sec_len = u16::from_le_bytes([rest[2], rest[3]]) as usize;
            if sec_len < SectionHeader::SIZE || sec_len > rest.len() {
                break;
            }
            let body = &rest[SectionHeader::SIZE..sec_len];
            match sec_id {
                SECTION_IDENTITY if body.len() >= 25 => {
                    out.title = Self::read_hdr_str(hdr, u16::from_le_bytes([body[0], body[1]]));
                    out.version = [body[2], body[3], body[4]];
                    out.abi = u16::from_le_bytes([body[5], body[6]]);
                    out.icon_id = u16::from_le_bytes([body[23], body[24]]);
                }
                SECTION_TYPE_LIFECYCLE if body.len() >= 12 => {
                    if let Some(et) = KrxExecType::from_u8(body[0]) {
                        out.exec_type = et;
                    }
                    out.type_flags = body[1];
                    out.caps = ::abi::CapabilitySet::from_raw(u32::from_le_bytes([
                        body[8], body[9], body[10], body[11],
                    ]));
                }
                SECTION_EXECUTION if body.len() >= SectionExecution::SIZE => {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            body.as_ptr(),
                            (&raw mut out.exec_raw).cast::<u8>(),
                            SectionExecution::SIZE,
                        );
                    }
                    out.has_exec = true;
                }
                SECTION_BLOCK_MAP => out.aux.block_map = Some(body),
                SECTION_LEAF_MAP => out.aux.leaf_map = Some(body),
                SECTION_SERVICE_EXPORTS => out.aux.service_exports = Some(body),
                SECTION_CONFIG => out.aux.config = Some(body),
                SECTION_ACTIONS => out.aux.actions = Some(body),
                _ => {}
            }
            let step = (sec_len + 3) & !3;
            if step >= rest.len() {
                break;
            }
            rest = &rest[step..];
        }

        Ok(out)
    }

    /// Zero-allocation, single-pass inspection of a KRX package header buffer.
    pub fn summarize<'a>(buf: &'a [u8]) -> Result<KrxSummary<'a>, KrxLaunchError> {
        let mut summary = core::mem::MaybeUninit::uninit();
        let _ = Self::summarize_into(buf, &mut summary)?;
        Ok(unsafe { summary.assume_init() })
    }

    /// Look up a self-contained icon bitmap (`ASSET_ICON_8`, `ASSET_ICON_16`, `ASSET_ICON_TRAY`)
    /// inside a package header's `SECTION_ASSETS (0x0004)` without allocating or expanding `KrxSummary`.
    pub fn find_icon_asset<'a>(buf: &'a [u8], want_kind: u8) -> Option<(u8, u8, &'a [u8])> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let h_len = u16::from_le_bytes([buf[8], buf[9]]) as usize;
        let hdr = buf.get(..h_len)?;
        let mut rest = hdr.get(Self::SIZE..)?;
        while rest.len() >= SectionHeader::SIZE {
            let sec_id = u16::from_le_bytes([rest[0], rest[1]]);
            let sec_len = u16::from_le_bytes([rest[2], rest[3]]) as usize;
            if sec_len < SectionHeader::SIZE || sec_len > rest.len() {
                break;
            }
            if sec_id == SECTION_ASSETS {
                let mut assets = &rest[SectionHeader::SIZE..sec_len];
                while assets.len() >= 4 {
                    let blen = assets[3] as usize;
                    if 4 + blen > assets.len() {
                        break;
                    }
                    if assets[0] == want_kind {
                        return Some((assets[1], assets[2], &assets[4..4 + blen]));
                    }
                    let step = (4 + blen + 3) & !3;
                    if step >= assets.len() {
                        break;
                    }
                    assets = &assets[step..];
                }
            }
            let step = (sec_len + 3) & !3;
            if step >= rest.len() {
                break;
            }
            rest = &rest[step..];
        }
        None
    }
}

/// Paged Block Directory Entry (0x0005, 8 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct BlockEntry {
    pub block_id: u8,
    pub flags: u8,
    pub size: u16,
    pub flash_offset: u32,
}

/// Service Export Descriptor (0x0006, 16-byte power-of-two layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ServiceExport {
    pub interface_id: u16,
    pub abi_version: u16,
    pub vtable_offset: u32,
    pub flags: u16,
    pub reserved: [u8; 6],
}

const _: () = assert!(core::mem::size_of::<ServiceExport>() == 16);

impl ServiceExport {
    pub const SIZE: usize = 16;
    pub const FLAG_PINNED: u16 = 1 << 0;

    pub const fn empty() -> Self {
        Self {
            interface_id: 0,
            abi_version: 0,
            vtable_offset: 0,
            flags: 0,
            reserved: [0u8; 6],
        }
    }

    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            interface_id: u16::from_le_bytes([buf[0], buf[1]]),
            abi_version: u16::from_le_bytes([buf[2], buf[3]]),
            vtable_offset: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            flags: u16::from_le_bytes([buf[8], buf[9]]),
            reserved: [buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]],
        })
    }

    #[inline(always)]
    pub const fn is_pinned(&self) -> bool {
        (self.flags & Self::FLAG_PINNED) != 0
    }
}

// ── P0 Structured Paged Leaf Overlays & Continuations (§5.1 / KRX 3.2) ──────

pub const FLAG_LEAF_PINNED: u8 = 1 << 0;
pub const FLAG_LEAF_RT: u8 = 1 << 1;
pub const FLAG_LEAF_PREFETCH: u8 = 1 << 2;

/// Paged Leaf Overlay Descriptor (0x000A, 16-byte power-of-two layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LeafEntry {
    pub leaf_id: u8,
    pub flags: u8,
    pub size: u16,
    pub flash_offset: u32,
    pub entry_offset: u16,
    pub ram_scratch_bytes: u16,
    pub reserved: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<LeafEntry>() == 16);

impl LeafEntry {
    pub const SIZE: usize = 16;

    pub const fn empty() -> Self {
        Self {
            leaf_id: 0,
            flags: 0,
            size: 0,
            flash_offset: 0,
            entry_offset: 0,
            ram_scratch_bytes: 0,
            reserved: [0u8; 4],
        }
    }

    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            leaf_id: buf[0],
            flags: buf[1],
            size: u16::from_le_bytes([buf[2], buf[3]]),
            flash_offset: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            entry_offset: u16::from_le_bytes([buf[8], buf[9]]),
            ram_scratch_bytes: u16::from_le_bytes([buf[10], buf[11]]),
            reserved: [buf[12], buf[13], buf[14], buf[15]],
        })
    }

    #[inline(always)]
    pub const fn is_pinned(&self) -> bool {
        (self.flags & FLAG_LEAF_PINNED) != 0
    }

    #[inline(always)]
    pub const fn is_realtime(&self) -> bool {
        (self.flags & FLAG_LEAF_RT) != 0
    }

    #[inline(always)]
    pub const fn is_prefetch_hint(&self) -> bool {
        (self.flags & FLAG_LEAF_PREFETCH) != 0
    }
}

/// Typed continuation returned by a P0 leaf overlay to the resident root upon completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LeafContinuation {
    pub kind: u8,
    pub target_or_code: u8,
    pub reserved: u16,
    pub payload: u32,
}

impl LeafContinuation {
    pub const KIND_DONE: u8 = 0;
    pub const KIND_CALL_LEAF: u8 = 1;
    pub const KIND_YIELD: u8 = 2;
    pub const KIND_ERROR: u8 = 3;

    #[inline(always)]
    pub const fn done(result: u32) -> Self {
        Self {
            kind: Self::KIND_DONE,
            target_or_code: 0,
            reserved: 0,
            payload: result,
        }
    }

    #[inline(always)]
    pub const fn call_leaf(target_leaf_id: u8, arg: u32) -> Self {
        Self {
            kind: Self::KIND_CALL_LEAF,
            target_or_code: target_leaf_id,
            reserved: 0,
            payload: arg,
        }
    }

    #[inline(always)]
    pub const fn yield_token(token: u32) -> Self {
        Self {
            kind: Self::KIND_YIELD,
            target_or_code: 0,
            reserved: 0,
            payload: token,
        }
    }

    #[inline(always)]
    pub const fn error(err_code: u8) -> Self {
        Self {
            kind: Self::KIND_ERROR,
            target_or_code: err_code,
            reserved: 0,
            payload: 0,
        }
    }

    #[inline(always)]
    pub const fn is_done(&self) -> bool {
        self.kind == Self::KIND_DONE
    }

    #[inline(always)]
    pub const fn is_call_leaf(&self) -> bool {
        self.kind == Self::KIND_CALL_LEAF
    }

    #[inline(always)]
    pub const fn is_yield(&self) -> bool {
        self.kind == Self::KIND_YIELD
    }

    #[inline(always)]
    pub const fn is_error(&self) -> bool {
        self.kind == Self::KIND_ERROR
    }

    #[inline(always)]
    pub const fn as_u64(&self) -> u64 {
        let low = (self.kind as u32)
            | ((self.target_or_code as u32) << 8)
            | ((self.reserved as u32) << 16);
        (low as u64) | ((self.payload as u64) << 32)
    }

    #[inline(always)]
    pub const fn from_u64(val: u64) -> Self {
        let low = (val & 0xFFFF_FFFF) as u32;
        let high = (val >> 32) as u32;
        Self {
            kind: (low & 0xFF) as u8,
            target_or_code: ((low >> 8) & 0xFF) as u8,
            reserved: ((low >> 16) & 0xFFFF) as u16,
            payload: high,
        }
    }
}

/// Invocation context passed into a P0 Structured Leaf Overlay (16 bytes, power-of-two geometry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LeafContext {
    pub leaf_id: u8,
    pub flags: u8,
    pub reserved: u16,
    pub arg: u32,
    pub continuation: LeafContinuation,
}

const _: () = assert!(core::mem::size_of::<LeafContext>() == 16);

impl LeafContext {
    pub const SIZE: usize = 16;

    pub const fn new(leaf_id: u8, arg: u32) -> Self {
        Self {
            leaf_id,
            flags: 0,
            reserved: 0,
            arg,
            continuation: LeafContinuation::done(0),
        }
    }
}

/// Asset Descriptor (0x0004, 20 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct AssetDescriptor {
    pub kind: u8,
    pub id: u8,
    pub flags: u16,
    pub offset: u32,
    pub length: u32,
    pub meta: u32,
    pub meta2: u32,
}

/// Fixed-size 32-byte Index Entry for two-pass zero-alloc launcher scanner
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct KrxIndexEntry {
    pub name: [u8; 16],
    pub class: u8,
    pub exec_type: u8,
    pub icon_id: u16,
    pub caps: u32,
    pub file_size: u32,
    pub crc16: u16,
    pub flags: u16,
}

const _: () = assert!(core::mem::size_of::<KrxIndexEntry>() == 32);

impl KrxIndexEntry {
    pub const fn empty() -> Self {
        Self {
            name: [0; 16],
            class: 0,
            exec_type: 1, // App
            icon_id: 0,
            caps: 0,
            file_size: 0,
            crc16: 0,
            flags: 0,
        }
    }

    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(16);
        unsafe { core::str::from_utf8_unchecked(&self.name[..len]) }
    }

    pub fn capability_set(&self) -> CapabilitySet {
        CapabilitySet::from_raw(self.caps)
    }
}
