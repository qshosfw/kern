//! KRX (KERN Realtime eXecutable) Binary Container & Header Parser
//!
//! Normative implementation of KRX-ABI 2.5.0 container format and section headers.
//! Provides zero-allocation, explicit little-endian parsing safe for ARM Cortex-M0+.

use crate::caps::CapabilitySet;

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
    VMachine = 2,
    Declare = 3,
}

impl KrxClass {
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::Native),
            1 => Some(Self::Paged),
            2 => Some(Self::VMachine),
            3 => Some(Self::Declare),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        const NAMES: &[u8] = b"Native\0Paged\0VM\0Declare\0";
        utils::format::nth_cstr(NAMES, *self as u8)
    }
}

/// KRX v3.2 Execution Profile & Executable Memory Paradigm taxonomy.
/// Encodes the exact trust, placement, and memory model of an executable package (§1).
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
    /// V0: Resident KVM-16 (software-sandboxed bytecode, resident image)
    V0ResidentVm = 4,
    /// D0: Declare Graph (software-sandboxed declarative UI/data graph)
    D0DeclareGraph = 5,
    /// F0: Declare Flow / Stream Graph (software-sandboxed DSP/audio dataflow pipeline)
    F0DeclareFlow = 6,
}

impl KrxProfile {
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            0 => Some(Self::N0NativeFixed),
            1 => Some(Self::P0StructuredPaged),
            2 => Some(Self::N1NativePic),
            3 => Some(Self::P1TransparentPaged),
            4 => Some(Self::V0ResidentVm),
            5 => Some(Self::D0DeclareGraph),
            6 => Some(Self::F0DeclareFlow),
            _ => None,
        }
    }

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::N0NativeFixed => "N0 (Native-Fixed)",
            Self::P0StructuredPaged => "P0 (Structured-Paged)",
            Self::N1NativePic => "N1 (Native-PIC)",
            Self::P1TransparentPaged => "P1 (Transparent-Paged)",
            Self::V0ResidentVm => "V0 (Resident-VM)",
            Self::D0DeclareGraph => "D0 (Declare-Graph)",
            Self::F0DeclareFlow => "F0 (Declare-Flow)",
        }
    }

    #[inline(always)]
    pub const fn is_trusted_native(&self) -> bool {
        matches!(
            self,
            Self::N0NativeFixed
                | Self::P0StructuredPaged
                | Self::N1NativePic
                | Self::P1TransparentPaged
        )
    }

    #[inline(always)]
    pub const fn is_sandboxed(&self) -> bool {
        matches!(
            self,
            Self::V0ResidentVm | Self::D0DeclareGraph | Self::F0DeclareFlow
        )
    }

    #[inline(always)]
    pub const fn is_structured_paged(&self) -> bool {
        matches!(self, Self::P0StructuredPaged)
    }

    #[inline(always)]
    pub const fn uses_fixed_vma(&self) -> bool {
        matches!(
            self,
            Self::N0NativeFixed
                | Self::P0StructuredPaged
                | Self::V0ResidentVm
                | Self::D0DeclareGraph
                | Self::F0DeclareFlow
        )
    }

    pub const fn raw_code(&self) -> u8 {
        match self {
            Self::N0NativeFixed => 0x10,
            Self::P0StructuredPaged => 0x11,
            Self::N1NativePic => 0x12,
            Self::P1TransparentPaged => 0x13,
            Self::V0ResidentVm => 0x20,
            Self::D0DeclareGraph => 0x30,
            Self::F0DeclareFlow => 0x31,
        }
    }

    pub const fn from_raw_code(code: u8) -> Option<Self> {
        match code {
            0x10 => Some(Self::N0NativeFixed),
            0x11 => Some(Self::P0StructuredPaged),
            0x12 => Some(Self::N1NativePic),
            0x13 => Some(Self::P1TransparentPaged),
            0x20 => Some(Self::V0ResidentVm),
            0x30 => Some(Self::D0DeclareGraph),
            0x31 => Some(Self::F0DeclareFlow),
            _ => None,
        }
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
            Some(KrxClass::Native) => {
                if exec.reloc_count == 0 && preamble.link_base != 0 {
                    Self::N0NativeFixed
                } else {
                    Self::N1NativePic
                }
            }
            Some(KrxClass::Paged) => {
                if has_leaf_map {
                    Self::P0StructuredPaged
                } else {
                    Self::P1TransparentPaged
                }
            }
            Some(KrxClass::VMachine) => Self::V0ResidentVm,
            Some(KrxClass::Declare) | None => Self::D0DeclareGraph,
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
    pub const fn from_u8(val: u8) -> Option<Self> {
        match val {
            1 => Some(Self::App),
            2 => Some(Self::Service),
            3 => Some(Self::Daemon),
            4 => Some(Self::Driver),
            5 => Some(Self::Module),
            _ => None,
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
    CapabilityDenied(&'static str),
    InsufficientRam,
    InsufficientStack,
    RelocationFailed,
    ImportBindingFailed,
    VmDisabled,
    PayloadTooLarge,
    ExecutionFailed,
    InvalidEntryPoint,
    TruncatedPackage,
    UnsupportedProfile,
}

const LAUNCH_ERR_TEXTS: &[u8] = b"\
File Not Found\0File not found\0\
Storage Error\0Storage read error\0\
Bad Package\0Bad package header\0\
CRC Error\0CRC check failed\0\
Bad Identity\0Missing identity\0\
Corrupted Binary\0Missing manifest\0\
Missing Hardware\0\0\
Out of RAM\0Not enough RAM\0\
Stack Overflow\0Stack headroom low\0\
Reloc Error\0Relocation failed\0\
API Unresolved\0API not available\0\
VM Disabled\0VM disabled\0\
Package Too Large\0Package too large\0\
Launch Failed\0Execution failed\0\
Bad Entry Point\0Invalid entry point\0\
Truncated File\0Truncated payload\0\
Unsupported Profile\0Unsupported profile\0";

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
            Self::VmDisabled => 11,
            Self::PayloadTooLarge => 12,
            Self::ExecutionFailed => 13,
            Self::InvalidEntryPoint => 14,
            Self::TruncatedPackage => 15,
            Self::UnsupportedProfile => 16,
        }
    }

    pub const fn title(&self) -> &'static str {
        utils::format::nth_cstr(LAUNCH_ERR_TEXTS, self.index() * 2)
    }

    pub const fn message(&self) -> &'static str {
        if let Self::CapabilityDenied(cap) = self {
            cap
        } else {
            utils::format::nth_cstr(LAUNCH_ERR_TEXTS, self.index() * 2 + 1)
        }
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
    pub const MAX_HEADER_SIZE: usize = 512;

    /// Parse preamble directly into `out` from a little-endian byte slice.
    #[inline(never)]
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
        if out.ident != KRX_IDENT || out.format_version == 0 || out.format_version > 5 {
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

    #[inline(never)]
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
    /// Module exposes an entry in the OS Settings -> System menu.
    pub const FLAG_SYSTEM_MENU: u8 = 1 << 6;
    /// Module exposes an action/dialog button in the OS Settings -> System maintenance group.
    pub const FLAG_SYSTEM_ACTION: u8 = 1 << 7;

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
    pub fn capabilities(&self) -> crate::caps::CapabilitySet {
        crate::caps::CapabilitySet::from_raw(self.cap_mask)
    }

    #[inline(always)]
    pub fn set_capabilities(&mut self, caps: crate::caps::CapabilitySet) {
        self.cap_mask = caps.raw();
    }
}

/// Compact, zero-allocation metadata summary extracted from a KRX package header.
#[derive(Debug, Clone, Copy, Default)]
pub struct KrxAuxSections<'a> {
    pub cap_imports: Option<&'a [u8]>,
    pub block_map: Option<&'a [u8]>,
    pub leaf_map: Option<&'a [u8]>,
    pub service_exports: Option<&'a [u8]>,
    pub actions: Option<&'a [u8]>,
}

/// Declared App Action Entry (`SECTION_ACTIONS = 0x000B`, 16 bytes)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxActionDecl {
    /// App-local action ID passed to `entry(event)` via launch args (0 = default launch)
    pub action_id: u16,
    /// Scope mask: bit 0 = shortcut binder, bit 1 = `[F]` drawer, bit 2 = system action
    pub scope_mask: u8,
    pub flags: u8,
    /// ASCII NUL-terminated label
    pub label: [u8; 12],
}

impl KrxActionDecl {
    pub const SIZE: usize = 16;
    pub const SCOPE_SHORTCUT: u8 = 1 << 0;
    pub const SCOPE_DRAWER: u8 = 1 << 1;
    pub const SCOPE_SYSTEM: u8 = 1 << 2;

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
pub struct KrxSummary<'a> {
    pub preamble: KrxPreamble,
    pub title: &'a str,
    pub icon_id: u16,
    pub exec_type: KrxExecType,
    pub type_flags: u8,
    pub caps: crate::caps::CapabilitySet,
    pub exec: Option<SectionExecution>,
    pub aux: KrxAuxSections<'a>,
}

impl<'a> KrxSummary<'a> {
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
    pub fn is_system_menu(&self) -> bool {
        (self.type_flags & SectionType::FLAG_SYSTEM_MENU) != 0
    }

    #[inline(always)]
    pub fn is_system_action(&self) -> bool {
        (self.type_flags & SectionType::FLAG_SYSTEM_ACTION) != 0
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
            title: "",
            icon_id: 0,
            exec_type: KrxExecType::App,
            type_flags: 0,
            caps: crate::caps::CapabilitySet::NONE,
            exec: None,
            aux: KrxAuxSections {
                cap_imports: None,
                block_map: None,
                leaf_map: None,
                service_exports: None,
                actions: None,
            },
        }
    }
}

impl KrxPreamble {
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
        if h_len < Self::SIZE || h_len > Self::MAX_HEADER_SIZE || (h_len & 3) != 0 {
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
                SECTION_IDENTITY => {
                    if body.len() >= 25 {
                        let name_off = u16::from_le_bytes([body[0], body[1]]) as usize;
                        if name_off >= Self::SIZE && name_off < h_len {
                            let tail = &hdr[name_off..h_len.min(name_off + 32)];
                            let len = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
                            if len > 0 {
                                if let Some(s) = utils::format::ascii_str(&tail[..len]) {
                                    out.title = s;
                                }
                            }
                        }
                        out.icon_id = u16::from_le_bytes([body[23], body[24]]);
                    }
                }
                SECTION_TYPE_LIFECYCLE => {
                    if body.len() >= 12 {
                        if let Some(et) = KrxExecType::from_u8(body[0]) {
                            out.exec_type = et;
                        }
                        out.type_flags = body[1];
                        out.caps = crate::caps::CapabilitySet::from_raw(u32::from_le_bytes([
                            body[8], body[9], body[10], body[11],
                        ]));
                    }
                }
                SECTION_EXECUTION => SectionExecution::parse_into(body, &mut out.exec),
                SECTION_CAP_IMPORTS => out.aux.cap_imports = Some(body),
                SECTION_BLOCK_MAP => out.aux.block_map = Some(body),
                SECTION_LEAF_MAP => out.aux.leaf_map = Some(body),
                SECTION_SERVICE_EXPORTS => out.aux.service_exports = Some(body),
                SECTION_ACTIONS => out.aux.actions = Some(body),
                _ => {}
            }
            let step = (sec_len + 3) & !3;
            rest = rest.get(step..).unwrap_or(&[]);
        }

        Ok(out)
    }

    /// Zero-allocation, single-pass inspection of a KRX package header buffer.
    pub fn summarize<'a>(buf: &'a [u8]) -> Result<KrxSummary<'a>, KrxLaunchError> {
        let mut summary = core::mem::MaybeUninit::uninit();
        let _ = Self::summarize_into(buf, &mut summary)?;
        Ok(unsafe { summary.assume_init() })
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

/// Service Export Descriptor (0x0006 / ORDS-01 16-byte power-of-two geometry).
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

/// P0 Structured Paged Leaf Overlay Descriptor (0x000A / ORDS-01 16-byte power-of-two geometry).
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

// ── Declare 3.0 Declarative Records (Section 10.2 normative) ────────────────

/// Declare Rule Record (12 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct RuleRecord {
    pub rule_id: u16,
    pub trigger: u8,
    pub flags: u8,
    pub trigger_ref: u16,
    pub cond_expr: u16,
    pub action_list: u16,
    pub action_count: u8,
    pub priority: u8,
    pub reserved: u16,
}

/// Declare Action Record (8 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct ActionRecord {
    pub op: u8,
    pub flags: u8,
    pub a: u16,
    pub b: u16,
    pub c: u16,
}

/// Declare Schedule Record (8 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ScheduleRecord {
    pub schedule_id: u16,
    pub kind: u8,
    pub rule_ref: u16,
    pub param: u32,
}

/// Declare UI Projection Node (8 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DeclareNode {
    pub node_id: u16,
    pub layout: u8,
    pub item_count: u8,
    pub title_offset: u16,
    pub first_item_offset: u16,
}

/// Declare UI Item Record (12 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DeclareItem {
    pub kind: u8,
    pub flags: u8,
    pub label_offset: u16,
    pub target: u16,
    pub value_ref: u16,
    pub aux: u16,
    pub reserved: u16,
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

/// Swap image flag: payload compressed using PackBits RLE byte-run algorithm
pub const SWAP_FLAG_COMPRESSED_RLE: u16 = 0x0001;

/// Fixed 16-byte power-of-two Swap Header for state hibernation (KRX-ABI 2.5.0 / ORDS-01)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxSwapHeader {
    pub ident: [u8; 4],
    pub app_id: u16,
    pub original_len: u16,
    pub compressed_len: u16,
    pub flags: u16,
    pub crc16: u16,
    pub reserved: [u8; 2],
}

const _: () = assert!(core::mem::size_of::<KrxSwapHeader>() == 16);

impl KrxSwapHeader {
    pub const SIZE: usize = 16;

    pub const fn new(
        app_id: u16,
        original_len: u16,
        compressed_len: u16,
        flags: u16,
        crc16: u16,
    ) -> Self {
        Self {
            ident: SWAP_IDENT,
            app_id,
            original_len,
            compressed_len,
            flags,
            crc16,
            reserved: [0; 2],
        }
    }

    #[inline(never)]
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let ident = [buf[0], buf[1], buf[2], buf[3]];
        if ident != SWAP_IDENT {
            return None;
        }
        let app_id = u16::from_le_bytes([buf[4], buf[5]]);
        let original_len = u16::from_le_bytes([buf[6], buf[7]]);
        let compressed_len = u16::from_le_bytes([buf[8], buf[9]]);
        let flags = u16::from_le_bytes([buf[10], buf[11]]);
        let crc16 = u16::from_le_bytes([buf[12], buf[13]]);
        let reserved = [buf[14], buf[15]];

        Some(Self {
            ident,
            app_id,
            original_len,
            compressed_len,
            flags,
            crc16,
            reserved,
        })
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..4].copy_from_slice(&self.ident);
        out[4..6].copy_from_slice(&self.app_id.to_le_bytes());
        out[6..8].copy_from_slice(&self.original_len.to_le_bytes());
        out[8..10].copy_from_slice(&self.compressed_len.to_le_bytes());
        out[10..12].copy_from_slice(&self.flags.to_le_bytes());
        out[12..14].copy_from_slice(&self.crc16.to_le_bytes());
        out[14..16].copy_from_slice(&self.reserved);
        out
    }

    pub fn is_valid(&self) -> bool {
        self.ident == SWAP_IDENT
    }

    pub fn is_rle(&self) -> bool {
        (self.flags & SWAP_FLAG_COMPRESSED_RLE) != 0
    }
}

/// PackBits Run-Length Encoding (RLE) byte compression.
///
/// Encodes runs of 3..128 identical bytes as `[-count + 1, byte]`.
/// Encodes non-repetitive literal sequences as `[count - 1, bytes...]`.
pub fn packbits_compress(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let mut in_idx = 0;
    let mut out_idx = 0;

    while in_idx < src.len() {
        // 1. Detect run of identical consecutive bytes
        let mut run_len = 1;
        while in_idx + run_len < src.len() && run_len < 128 && src[in_idx + run_len] == src[in_idx]
        {
            run_len += 1;
        }

        if run_len >= 3 || (run_len == 2 && in_idx + run_len == src.len()) {
            if out_idx + 2 > dst.len() {
                return Err(());
            }
            dst[out_idx] = (257 - run_len) as u8;
            dst[out_idx + 1] = src[in_idx];
            out_idx += 2;
            in_idx += run_len;
        } else {
            // 2. Collect literal bytes until a 3+ identical byte run starts
            let mut lit_len = 0;
            while in_idx + lit_len < src.len() && lit_len < 128 {
                let mut next_run = 1;
                while in_idx + lit_len + next_run < src.len()
                    && next_run < 3
                    && src[in_idx + lit_len + next_run] == src[in_idx + lit_len]
                {
                    next_run += 1;
                }
                if next_run >= 3 {
                    break;
                }
                lit_len += 1;
            }

            if lit_len > 0 {
                if out_idx + 1 + lit_len > dst.len() {
                    return Err(());
                }
                dst[out_idx] = (lit_len - 1) as u8;
                out_idx += 1;
                dst[out_idx..out_idx + lit_len].copy_from_slice(&src[in_idx..in_idx + lit_len]);
                out_idx += lit_len;
                in_idx += lit_len;
            }
        }
    }

    Ok(out_idx)
}

/// PackBits Run-Length Encoding (RLE) byte decompression.
pub fn packbits_decompress(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let mut in_idx = 0;
    let mut out_idx = 0;

    while in_idx < src.len() {
        let header = src[in_idx];
        in_idx += 1;

        if header <= 127 {
            let count = (header as usize) + 1;
            if in_idx + count > src.len() || out_idx + count > dst.len() {
                return Err(());
            }
            dst[out_idx..out_idx + count].copy_from_slice(&src[in_idx..in_idx + count]);
            in_idx += count;
            out_idx += count;
        } else if header > 128 {
            let count = 257 - (header as usize);
            if in_idx >= src.len() || out_idx + count > dst.len() {
                return Err(());
            }
            let byte = src[in_idx];
            in_idx += 1;
            for i in 0..count {
                dst[out_idx + i] = byte;
            }
            out_idx += count;
        }
        // header == 128 is treated as a no-op
    }

    Ok(out_idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_krx_swap_header_serde() {
        let header = KrxSwapHeader::new(2, 64, 28, SWAP_FLAG_COMPRESSED_RLE, 0xABCD);
        assert!(header.is_valid());
        assert!(header.is_rle());
        assert_eq!(core::mem::size_of::<KrxSwapHeader>(), 16);

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), 16);

        let parsed = KrxSwapHeader::parse(&bytes).expect("Failed to parse swap header");
        let ident = parsed.ident;
        let app_id = parsed.app_id;
        let orig_len = parsed.original_len;
        let comp_len = parsed.compressed_len;
        let flags = parsed.flags;
        let crc = parsed.crc16;

        assert_eq!(ident, SWAP_IDENT);
        assert_eq!(app_id, 2);
        assert_eq!(orig_len, 64);
        assert_eq!(comp_len, 28);
        assert_eq!(flags, SWAP_FLAG_COMPRESSED_RLE);
        assert_eq!(crc, 0xABCD);
    }

    #[test]
    fn test_service_export_serde() {
        assert_eq!(core::mem::size_of::<ServiceExport>(), 16);
        let mut raw = [0u8; 16];
        raw[0..2].copy_from_slice(&0x0104u16.to_le_bytes()); // interface_id
        raw[2..4].copy_from_slice(&2u16.to_le_bytes()); // abi_version
        raw[4..8].copy_from_slice(&0x0000_0140u32.to_le_bytes()); // vtable_offset
        raw[8..10].copy_from_slice(&ServiceExport::FLAG_PINNED.to_le_bytes()); // flags

        let export = ServiceExport::parse(&raw).expect("Failed to parse ServiceExport");
        let iface = export.interface_id;
        let abi = export.abi_version;
        let vtable_off = export.vtable_offset;
        assert_eq!(iface, 0x0104);
        assert_eq!(abi, 2);
        assert_eq!(vtable_off, 0x0140);
        assert!(export.is_pinned());
    }

    #[test]
    fn test_section_execution_can_execute() {
        let mut raw = [0u8; 28];
        // min_stack = 256
        raw[0..2].copy_from_slice(&256u16.to_le_bytes());
        // min_heap = 512
        raw[2..4].copy_from_slice(&512u16.to_le_bytes());

        let sec = SectionExecution::parse(&raw).expect("Failed to parse SectionExecution");
        // Total required = 256 + 512 + 128 = 896
        assert!(sec.can_execute(1024, 128));
        assert!(sec.can_execute(896, 128));
        assert!(!sec.can_execute(895, 128));
        assert!(!sec.can_execute(512, 128));
    }

    #[test]
    fn test_packbits_compression_roundtrip() {
        let original = [
            1, 2, 3, 4, 5, 0xAA, 0xAA, 0xAA, 0xAA, 0xAA, 10, 20, 30, 0xBB, 0xBB, 0xBB, 99,
        ];
        let mut compressed = [0u8; 64];
        let comp_len = packbits_compress(&original, &mut compressed).expect("Compression failed");

        let mut restored = [0u8; 64];
        let decomp_len = packbits_decompress(&compressed[..comp_len], &mut restored)
            .expect("Decompression failed");

        assert_eq!(decomp_len, original.len());
        assert_eq!(&restored[..decomp_len], &original[..]);
    }

    #[test]
    fn test_packbits_repeated_runs() {
        let original = [0u8; 100];
        // 100 zeros
        let mut compressed = [0u8; 64];
        let comp_len = packbits_compress(&original, &mut compressed).expect("Compression failed");
        // A single run of 100 identical bytes should take exactly 2 bytes!
        assert_eq!(comp_len, 2);

        let mut restored = [0xFFu8; 100];
        let decomp_len = packbits_decompress(&compressed[..comp_len], &mut restored)
            .expect("Decompression failed");
        assert_eq!(decomp_len, 100);
        assert!(restored.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_krx_launch_error_diagnostics() {
        use crate::caps::Cap;

        let err_cap = KrxLaunchError::CapabilityDenied(Cap::BroadBandRx.required_message());
        assert_eq!(err_cap.title(), "Missing Hardware");
        assert_eq!(err_cap.message(), "Broadband HF RX");

        let err_ram = KrxLaunchError::InsufficientRam;
        assert_eq!(err_ram.title(), "Out of RAM");
        assert_eq!(err_ram.message(), "Not enough RAM");

        let err_stack = KrxLaunchError::InsufficientStack;
        assert_eq!(err_stack.title(), "Stack Overflow");
        assert_eq!(err_stack.message(), "Stack headroom low");

        let err_vm = KrxLaunchError::VmDisabled;
        assert_eq!(err_vm.title(), "VM Disabled");
        assert_eq!(err_vm.message(), "VM disabled");

        let err_fnf = KrxLaunchError::FileNotFound;
        assert_eq!(err_fnf.title(), "File Not Found");
        assert_eq!(err_fnf.message(), "File not found");

        let err_reloc = KrxLaunchError::RelocationFailed;
        assert_eq!(err_reloc.title(), "Reloc Error");
        assert_eq!(err_reloc.message(), "Relocation failed");

        let err_api = KrxLaunchError::ImportBindingFailed;
        assert_eq!(err_api.title(), "API Unresolved");
        assert_eq!(err_api.message(), "API not available");

        let err_corrupt = KrxLaunchError::MissingExecution;
        assert_eq!(err_corrupt.title(), "Corrupted Binary");
        assert_eq!(err_corrupt.message(), "Missing manifest");
    }

    #[test]
    fn test_leaf_continuation_and_context() {
        assert_eq!(core::mem::size_of::<LeafContext>(), 16);
        assert_eq!(core::mem::size_of::<LeafContinuation>(), 8);

        let cont = LeafContinuation::call_leaf(5, 0x1234_5678);
        assert!(cont.is_call_leaf());
        let raw = cont.as_u64();
        let restored = LeafContinuation::from_u64(raw);
        assert_eq!(cont, restored);
        assert_eq!(restored.target_or_code, 5);
        assert_eq!(restored.payload, 0x1234_5678);

        let ctx = LeafContext::new(3, 42);
        assert_eq!(ctx.leaf_id, 3);
        let arg = ctx.arg;
        assert_eq!(arg, 42);
        let cont = ctx.continuation;
        assert!(cont.is_done());
    }

    #[test]
    fn test_krx_profile_detection_and_inference() {
        let mut preamble_bytes = [0u8; 24];
        preamble_bytes[0..4].copy_from_slice(&KRX_IDENT);
        preamble_bytes[4] = KrxClass::Native as u8;
        preamble_bytes[5] = 2; // format_version
        preamble_bytes[6..8].copy_from_slice(&1u16.to_le_bytes()); // section_count
        let preamble = KrxPreamble::parse(&preamble_bytes).unwrap();

        let mut exec = SectionExecution {
            entry_point: 0x10,
            got_offset: 0x40,
            reloc_offset: 0,
            reloc_count: 0,
            min_stack_bytes: 256,
            min_heap_bytes: 256,
            page_size: 0,
            cache_blocks: 0,
            dma_burst_size: 0,
            payload_size: 1024,
            rt_pin_mask: 0,
            max_fault_latency_us: 0,
            reserved: [0u8; 2],
        };

        // Class Native + reloc_count == 0 + link_base == 0 => N1NativePic
        assert_eq!(
            KrxProfile::detect(&preamble, &exec, false, false),
            KrxProfile::N1NativePic
        );

        // Class Native + reloc_count == 0 + link_base != 0 => N0NativeFixed
        let mut fixed_preamble_bytes = preamble_bytes;
        fixed_preamble_bytes[20..24].copy_from_slice(&0x2000_1000u32.to_le_bytes());
        let fixed_preamble = KrxPreamble::parse(&fixed_preamble_bytes).unwrap();
        assert_eq!(
            KrxProfile::detect(&fixed_preamble, &exec, false, false),
            KrxProfile::N0NativeFixed
        );

        // Explicit profile stamped into reserved[0] overrides detection
        exec.reserved[0] = KrxProfile::P0StructuredPaged.raw_code();
        assert_eq!(
            KrxProfile::detect(&preamble, &exec, false, false),
            KrxProfile::P0StructuredPaged
        );

        exec.reserved[0] = 0;
        // Class Paged + has_leaf_map => P0StructuredPaged
        let mut paged_bytes = preamble_bytes;
        paged_bytes[4] = KrxClass::Paged as u8;
        let paged_preamble = KrxPreamble::parse(&paged_bytes).unwrap();
        assert_eq!(
            KrxProfile::detect(&paged_preamble, &exec, false, true),
            KrxProfile::P0StructuredPaged
        );
        // Class Paged + no leaf map => P1TransparentPaged
        assert_eq!(
            KrxProfile::detect(&paged_preamble, &exec, true, false),
            KrxProfile::P1TransparentPaged
        );
    }

    #[test]
    fn test_synthetic_krx_package_end_to_end_parsing() {
        let mut pkg = [0u8; 180];

        // 1. Preamble (24 bytes)
        pkg[0..4].copy_from_slice(&KRX_IDENT);
        pkg[4] = KrxClass::Paged as u8;
        pkg[5] = 5; // format version 2.5.0
        pkg[6..8].copy_from_slice(&4u16.to_le_bytes()); // 4 sections
        let header_len = 148u16;
        pkg[8..10].copy_from_slice(&header_len.to_le_bytes());

        // 2. SectionIdentity (offset 24: 4B header + 40B body = 44B, ends at 68)
        pkg[24..26].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
        pkg[26..28].copy_from_slice(&44u16.to_le_bytes());
        let name_str_off = 132u16;
        pkg[28..30].copy_from_slice(&name_str_off.to_le_bytes());
        pkg[30..33].copy_from_slice(&[1, 2, 0]); // v1.2.0
        pkg[33..35].copy_from_slice(&1u16.to_le_bytes()); // min_abi = 1
        pkg[35..41].copy_from_slice(b"F4HWN\0");

        // 3. SectionExecution (offset 68: 4B header + 28B body = 32B, ends at 100)
        pkg[68..70].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
        pkg[70..72].copy_from_slice(&32u16.to_le_bytes());
        pkg[72..74].copy_from_slice(&256u16.to_le_bytes()); // min_stack = 256
        pkg[76..78].copy_from_slice(&1024u16.to_le_bytes()); // page_size = 1024
        pkg[78] = 2; // cache_blocks = 2
        pkg[80..84].copy_from_slice(&0x08u32.to_le_bytes()); // entry_point = 0x08
        pkg[84..88].copy_from_slice(&32u32.to_le_bytes()); // payload_size = 32

        // 4. SectionCapImports (offset 100: 4B header + 8B body = 12B, ends at 112)
        pkg[100..102].copy_from_slice(&SECTION_CAP_IMPORTS.to_le_bytes());
        pkg[102..104].copy_from_slice(&12u16.to_le_bytes());
        pkg[104..106].copy_from_slice(&0x0001u16.to_le_bytes()); // interface 1 (Display)
        pkg[106..108].copy_from_slice(&1u16.to_le_bytes()); // min_abi = 1
        pkg[108..110].copy_from_slice(&1u16.to_le_bytes()); // flags = mandatory
        pkg[110..112].copy_from_slice(&0u16.to_le_bytes()); // slot = 0

        // 5. SectionLeafMap (offset 112: 4B header + 16B body = 20B, ends at 132)
        pkg[112..114].copy_from_slice(&SECTION_LEAF_MAP.to_le_bytes());
        pkg[114..116].copy_from_slice(&20u16.to_le_bytes());
        pkg[116] = 1; // leaf_id = 1
        pkg[117] = 0; // flags = 0
        pkg[118..120].copy_from_slice(&512u16.to_le_bytes()); // size = 512
        pkg[120..124].copy_from_slice(&148u32.to_le_bytes()); // flash_offset = 148
        pkg[124..126].copy_from_slice(&0u16.to_le_bytes()); // entry_offset = 0
        pkg[126..128].copy_from_slice(&0u16.to_le_bytes()); // ram_scratch = 0

        // 6. String Pool (offset 132..148)
        pkg[132..138].copy_from_slice(b"Beacon");
        let name_str_off = 132u16;
        pkg[28..30].copy_from_slice(&name_str_off.to_le_bytes());

        let header_len = 148u16;
        pkg[8..10].copy_from_slice(&header_len.to_le_bytes());

        // Validate parsing
        let preamble = KrxPreamble::parse(&pkg).unwrap();
        let section_count = preamble.section_count;
        let hlen = preamble.header_len;
        assert_eq!(section_count, 4);
        assert_eq!(hlen, 148);

        // Find Identity
        let ident_body = find_section(&pkg, 148, SECTION_IDENTITY).unwrap();
        let ident = SectionIdentity::parse(ident_body).unwrap();
        assert_eq!(ident.name(&pkg[..148]).unwrap(), "Beacon");

        // Find Execution
        let exec_body = find_section(&pkg, 148, SECTION_EXECUTION).unwrap();
        let exec = SectionExecution::parse(exec_body).unwrap();
        let ep = exec.entry_point;
        let ps = exec.payload_size;
        assert_eq!(ep, 0x08);
        assert_eq!(ps, 32);

        // Find CapImports
        let cap_body = find_section(&pkg, 148, SECTION_CAP_IMPORTS).unwrap();
        let binding = CapBinding::parse(cap_body, 0).unwrap();
        let iface = binding.interface_id;
        let slot = binding.slot_idx;
        assert_eq!(iface, 0x0001);
        assert_eq!(slot, 0);

        // Find LeafMap
        let leaf_body = find_section(&pkg, 148, SECTION_LEAF_MAP).unwrap();
        let leaf = LeafEntry::parse(leaf_body).unwrap();
        let lsize = leaf.size;
        assert_eq!(leaf.leaf_id, 1);
        assert_eq!(lsize, 512);

        // Detect profile: Paged class + leaf map => P0StructuredPaged
        let profile = KrxProfile::detect(&preamble, &exec, false, true);
        assert_eq!(profile, KrxProfile::P0StructuredPaged);
    }

    #[test]
    fn test_native_provisioned_package_full_test_bench() {
        use crate::caps::Cap;
        use crate::proc::ProcessTable;
        use crate::proc::abi::*;
        use crate::proc::memory::MemoryMapLayout;

        // ── 1. Synthesize Native Provisioned Package Binary (/apps/beacon.krx) ──
        let mut pkg = [0u8; 148]; // 132B header + 16B code payload

        // 1a. Preamble (24B)
        pkg[0..4].copy_from_slice(&KRX_IDENT);
        pkg[4] = KrxClass::Native as u8;
        pkg[5] = 0x05; // v2.5.0
        pkg[6..8].copy_from_slice(&4u16.to_le_bytes()); // 4 sections
        let header_len = 132u16;
        pkg[8..10].copy_from_slice(&header_len.to_le_bytes());

        // 1b. SectionIdentity (offset 24: 4B header + 40B body = 44B, ends at 68)
        pkg[24..26].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
        pkg[26] = 44;
        pkg[28] = 116; // name_str offset = 116
        pkg[30] = 1; // version 1.0.0
        pkg[33] = 1; // min_kernel_abi = 1
        pkg[35..44].copy_from_slice(b"KERN-TEAM");
        pkg[51] = 1; // icon_asset = 1

        // 1c. SectionExecution (offset 68: 4B header + 28B body = 32B, ends at 100)
        pkg[68..70].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
        pkg[70] = 32;
        pkg[72..74].copy_from_slice(&256u16.to_le_bytes()); // min_stack_bytes = 256
        pkg[74..76].copy_from_slice(&128u16.to_le_bytes()); // min_heap_bytes = 128
        pkg[76..78].copy_from_slice(&0u16.to_le_bytes()); // page_size = 0 (unpaged native)
        pkg[78] = 0; // cache_blocks = 0
        pkg[80..84].copy_from_slice(&0u32.to_le_bytes()); // entry_point = 0 (offset in payload)
        pkg[84..88].copy_from_slice(&16u32.to_le_bytes()); // payload_size = 16B
        pkg[92..94].copy_from_slice(&16u16.to_le_bytes()); // got_offset = 16B
        pkg[98] = KrxProfile::N1NativePic.raw_code(); // explicit profile

        // 1d. SectionType (offset 100: 4B header + 12B body = 16B, ends at 116)
        pkg[100..102].copy_from_slice(&SECTION_TYPE_LIFECYCLE.to_le_bytes());
        pkg[102] = 16;
        pkg[104] = KrxExecType::App as u8;
        pkg[106] = 1; // stack_cost = 1
        let required_caps =
            (Cap::DisplayDirect as u32) | (Cap::Keypad as u32) | (Cap::RadioRx as u32);
        pkg[112..116].copy_from_slice(&required_caps.to_le_bytes());

        // 1e. String pool (offset 116..132)
        pkg[116..122].copy_from_slice(b"Beacon");

        // 1f. Thumb-1 Native Code Payload (offset 132..148, 16 bytes)
        let thumb1_code: [u8; 16] = [
            0x02, 0x28, // cmp r0, #2 (KRX_OP_EVENT)
            0x01, 0xd1, // bne +1
            0x01, 0x20, // movs r0, #1 (Consumed)
            0x70, 0x47, // bx lr
            0x00, 0x20, // movs r0, #0 (Success / Ignored)
            0x70, 0x47, // bx lr
            0x00, 0x00, 0x00, 0x00, // alignment padding
        ];
        pkg[132..148].copy_from_slice(&thumb1_code);

        // ── 2. Validate End-to-End Preamble and Section Scanning ──
        let preamble = KrxPreamble::parse(&pkg).expect("Valid KRX preamble");
        assert_eq!(preamble.ident, KRX_IDENT);
        assert_eq!(preamble.execution_class(), Some(KrxClass::Native));
        assert_eq!(preamble.format_version, 5);
        let hlen = preamble.header_len;
        assert_eq!(hlen, 132);

        let mut scanned_ident = false;
        let mut scanned_exec = false;
        let mut scanned_type = false;

        for (hdr, body) in KrxSectionIterator::new(&pkg, 132) {
            match hdr.id {
                SECTION_IDENTITY => {
                    let ident = SectionIdentity::parse(body).expect("Valid Identity");
                    assert_eq!(ident.name(&pkg[..132]).unwrap(), "Beacon");
                    assert_eq!(ident.version, [1, 0, 0]);
                    scanned_ident = true;
                }
                SECTION_EXECUTION => {
                    let exec = SectionExecution::parse(body).expect("Valid Execution");
                    let min_stack = exec.min_stack_bytes;
                    let payload_sz = exec.payload_size;
                    let got_off = exec.got_offset;
                    assert_eq!(min_stack, 256);
                    assert_eq!(payload_sz, 16);
                    assert_eq!(got_off, 16);
                    assert_eq!(exec.reserved[0], KrxProfile::N1NativePic.raw_code());
                    // Verify RAM / stack margin headroom evaluation
                    assert!(exec.can_execute(1024, 256));
                    assert!(!exec.can_execute(300, 256));
                    scanned_exec = true;
                }
                SECTION_TYPE_LIFECYCLE => {
                    let st = SectionType::parse(body).expect("Valid Type");
                    assert_eq!(st.exec_type, KrxExecType::App as u8);
                    let cap_set = st.capabilities();
                    assert!(cap_set.contains(Cap::DisplayDirect));
                    assert!(cap_set.contains(Cap::Keypad));
                    assert!(cap_set.contains(Cap::RadioRx));
                    assert!(!cap_set.contains(Cap::BroadBandRx));
                    scanned_type = true;
                }
                _ => {}
            }
        }
        assert!(scanned_ident && scanned_exec && scanned_type);

        // ── 3. Validate Capability Binding & Simulated GOT Resolution ──
        let mut got_table = [0usize; 8];
        // Slot 0: Display VTable
        got_table[0] = &STUB_DISPLAY_VTABLE as *const _ as usize;
        // Slot 1: Radio VTable
        got_table[1] = &STUB_RADIO_VTABLE as *const _ as usize;
        // Slot 2: Audio VTable
        got_table[2] = &STUB_AUDIO_VTABLE as *const _ as usize;

        unsafe {
            let radio = &*(got_table[1] as *const RadioApi);
            assert_eq!((radio.rf_get_freq)(), 0);
            assert_eq!((radio.rf_tune)(145_000_000, 25, 12_500), 1);
            let display = &*(got_table[0] as *const DisplayApi);
            assert_eq!((display.get_framebuffer)(), core::ptr::null_mut());
        }

        // ── 4. Validate Process Lifecycle & PCB Ledger Integration ──
        let mut proc_table = ProcessTable::<4>::new();
        let pid = proc_table
            .spawn(
                "Beacon",
                1,
                crate::sched::Priority::Ui,
                crate::proc::ExecType::RelocatableNative,
                CapabilitySet::from_raw(required_caps),
            )
            .expect("Spawn Beacon process");
        assert_eq!(pid, crate::proc::Pid(1));

        let pcb = proc_table.find_mut(pid).expect("Found PCB");
        assert_eq!(pcb.state, crate::proc::ProcState::Ready);
        pcb.set_state(crate::proc::ProcState::Running);
        pcb.ledger.record_activation();
        pcb.ledger.record_event();
        assert_eq!(pcb.ledger.cpu_ticks, 1);
        assert_eq!(pcb.ledger.ipc_events, 1);

        // ── 5. Validate MemoryMapLayout Boundary & Access Protection ──
        let mut mem_layout = MemoryMapLayout::<4>::new();
        let load_base = 0x2000_1000u32;
        let code_size = 16u32;
        let ram_vma = load_base + 16;
        let ram_size = 256u32;
        mem_layout.init_native_pic(load_base, code_size, ram_vma, ram_size);

        // Code region: Execute = OK, Write = Denied
        assert!(mem_layout.validate_access(load_base, 2, false, true));
        assert!(mem_layout.validate_access(load_base + 8, 4, false, false));
        assert!(!mem_layout.validate_access(load_base, 4, true, false));

        // RAM/GOT region: Write = OK, Execute = Denied
        assert!(mem_layout.validate_access(ram_vma, 4, true, false));
        assert!(mem_layout.validate_access(ram_vma + 32, 4, false, false));
        assert!(!mem_layout.validate_access(ram_vma, 2, false, true));

        // Out of bounds: Denied
        assert!(!mem_layout.validate_access(load_base - 4, 4, false, false));
        assert!(!mem_layout.validate_access(ram_vma + ram_size + 4, 4, false, false));

        // ── 6. Full C-ABI Safe-Stub Execution Dispatch Test ──
        let api_table = KrxApiTable::safe_stub(required_caps);
        assert_eq!(api_table.abi_version, KRX_ABI_VERSION_3_0);
        assert_eq!(api_table.granted_caps, required_caps);

        // Test Simulated Guest Entry Point Handler:
        // Demonstrates standard lifecycle routing and safe stub calls
        let guest_handler = |op: u32, api: *const KrxApiTable, param: *const ()| -> u32 {
            let table = unsafe { &*api };
            match op {
                KRX_OP_START => {
                    // Initialize app state
                    0
                }
                KRX_OP_EVENT => {
                    if param.is_null() {
                        return 0;
                    }
                    let ev = unsafe { &*(param as *const KrxEvent) };
                    if ev.is_key() {
                        1 // Consumed
                    } else if ev.is_radio_bus() {
                        let _freq = unsafe { ((*table.radio).rf_get_freq)() };
                        1 // Consumed
                    } else {
                        0 // Ignored
                    }
                }
                KRX_OP_RENDER => {
                    unsafe {
                        ((*table.display).draw_rect)(0, 0, 128, 64, 1);
                        ((*table.display).commit_frame)();
                    }
                    0
                }
                KRX_OP_EXIT => 0,
                _ => 0xFFFF_FFFF,
            }
        };

        // Dispatch KRX_OP_START
        assert_eq!(
            guest_handler(KRX_OP_START, &api_table, core::ptr::null()),
            0
        );

        // Dispatch KRX_OP_EVENT (Key Press)
        let key_ev = KrxEvent {
            kind: KRX_EV_KEY,
            key: 15,
            flags: 0,
            reserved: 0,
            payload: 0, // KeyState::Pressed
        };
        assert_eq!(
            guest_handler(KRX_OP_EVENT, &api_table, &key_ev as *const _ as *const ()),
            1
        );

        // Dispatch KRX_OP_EVENT (Radio Bus)
        let bus_ev = KrxEvent {
            kind: KRX_EV_BUS,
            key: 1,
            flags: (crate::ipc::Topic::RADIO.0 & 0xFF) as u8,
            reserved: 0,
            payload: 145_000_000,
        };
        assert_eq!(
            guest_handler(KRX_OP_EVENT, &api_table, &bus_ev as *const _ as *const ()),
            1
        );

        // Dispatch KRX_OP_RENDER
        assert_eq!(
            guest_handler(KRX_OP_RENDER, &api_table, core::ptr::null()),
            0
        );

        // Dispatch KRX_OP_EXIT
        assert_eq!(guest_handler(KRX_OP_EXIT, &api_table, core::ptr::null()), 0);

        // Terminate process
        let pcb = proc_table.find_mut(pid).expect("Found PCB");
        pcb.set_state(crate::proc::ProcState::Dead);
        assert!(!pcb.is_active());
    }

    #[test]
    fn test_native_execution_fault_matrix_test_bench() {
        use crate::caps::{Cap, CapabilitySet};

        // 1. Fault: Corrupted Preamble FourCC
        let mut bad_ident = [0u8; 32];
        bad_ident[0..4].copy_from_slice(b"BAD\0");
        assert!(KrxPreamble::parse(&bad_ident).is_err());

        // 2. Fault: Unsupported Format Version
        let mut bad_ver = [0u8; 32];
        bad_ver[0..4].copy_from_slice(&KRX_IDENT);
        bad_ver[5] = 99; // Version 99.0
        assert!(KrxPreamble::parse(&bad_ver).is_err());

        // 3. Fault: Truncated Package Header
        let mut truncated = [0u8; 64];
        truncated[0..4].copy_from_slice(&KRX_IDENT);
        truncated[4] = KrxClass::Native as u8;
        truncated[5] = 5;
        truncated[8..10].copy_from_slice(&128u16.to_le_bytes()); // header_len = 128, but only 64B present!
        let preamble = KrxPreamble::parse(&truncated).unwrap();
        assert!(64 < preamble.header_len as usize); // Truncation detected!

        // 4. Fault: Missing Identity Section
        let mut no_ident = [0u8; 128];
        no_ident[0..4].copy_from_slice(&KRX_IDENT);
        no_ident[4] = KrxClass::Native as u8;
        no_ident[5] = 5;
        no_ident[6..8].copy_from_slice(&1u16.to_le_bytes());
        no_ident[8..10].copy_from_slice(&64u16.to_le_bytes());
        no_ident[24..26].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
        no_ident[26] = 32;
        let mut found_ident = false;
        for (hdr, _) in KrxSectionIterator::new(&no_ident, 64) {
            if hdr.id == SECTION_IDENTITY {
                found_ident = true;
            }
        }
        assert!(!found_ident); // Correctly missing!

        // 5. Fault: Capability Denied (requires BroadBandRx, platform only has Display + Keypad)
        let platform_caps =
            CapabilitySet::from_raw((Cap::DisplayDirect as u32) | (Cap::Keypad as u32));
        let app_required_caps =
            CapabilitySet::from_raw((Cap::DisplayDirect as u32) | (Cap::BroadBandRx as u32));
        assert!(!platform_caps.contains_all(app_required_caps));
        let missing = app_required_caps.without(platform_caps);
        assert_eq!(missing.first_cap(), Some(Cap::BroadBandRx));
        let err = KrxLaunchError::CapabilityDenied(missing.first_cap().unwrap().required_message());
        assert_eq!(err.message(), "Broadband HF RX");

        // 6. Fault: Insufficient Stack Headroom
        let mut exec_raw = [0u8; 28];
        exec_raw[0..2].copy_from_slice(&512u16.to_le_bytes()); // min_stack_bytes = 512
        let sec_exec = SectionExecution::parse(&exec_raw).unwrap();
        let current_stack_margin = 256usize;
        assert!(current_stack_margin < sec_exec.min_stack_bytes as usize);

        // 7. Fault: Insufficient System RAM
        exec_raw[2..4].copy_from_slice(&4096u16.to_le_bytes()); // min_heap_bytes = 4096
        let sec_exec_ram = SectionExecution::parse(&exec_raw).unwrap();
        let available_ram = 2048usize;
        assert!(!sec_exec_ram.can_execute(available_ram, 256));

        // 8. Fault: Relocation Truncation / Stream Corrupt
        let mut reloc_buffer = [0u32; 16];
        let reloc_stream = [0xFFu8, 0xFFu8]; // Truncated escape stream
        let res = unsafe {
            crate::proc::reloc::apply_relocs(
                reloc_buffer.as_mut_ptr() as *mut u8,
                64,
                &reloc_stream,
                0x1000,
                5, // Expects 5 relocations, only 2 bytes provided
            )
        };
        assert!(res.is_err());
    }

    #[test]
    fn test_native_app_execution_and_abi_integration_bench() {
        use crate::caps::Cap;
        use crate::proc::abi::*;

        // Construct a complete Native Package in memory:
        let mut pkg = [0u8; 320];
        let required_caps =
            (Cap::DisplayDirect as u32) | (Cap::Keypad as u32) | (Cap::RadioRx as u32);

        // 1. Preamble (24 bytes)
        pkg[0..4].copy_from_slice(&KRX_IDENT);
        pkg[4] = KrxClass::Native as u8;
        pkg[5] = 5; // format version 2.5.0
        pkg[6..8].copy_from_slice(&3u16.to_le_bytes()); // 3 sections
        pkg[8..10].copy_from_slice(&132u16.to_le_bytes()); // header_len = 132

        // 2. SectionIdentity (offset 24: 4B header + 40B body)
        pkg[24..26].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
        pkg[26] = 44;
        pkg[28] = 116; // name_str offset = 116
        pkg[30] = 1; // v1.0.0
        pkg[33] = 1; // min_abi = 1
        pkg[35..44].copy_from_slice(b"KERN-TEAM");

        // 3. SectionExecution (offset 68: 4B header + 28B body)
        pkg[68..70].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
        pkg[70] = 32;
        pkg[72..74].copy_from_slice(&256u16.to_le_bytes()); // min_stack_bytes = 256
        pkg[76..78].copy_from_slice(&512u16.to_le_bytes()); // page_size = 512
        pkg[84..88].copy_from_slice(&176u32.to_le_bytes()); // payload_size = 176
        pkg[92..94].copy_from_slice(&176u16.to_le_bytes()); // got_offset
        pkg[98] = KrxProfile::N1NativePic.raw_code();

        // 4. SectionType (offset 100: 4B header + 12B body)
        pkg[100..102].copy_from_slice(&SECTION_TYPE_LIFECYCLE.to_le_bytes());
        pkg[102] = 16;
        pkg[104] = KrxExecType::App as u8;
        pkg[112..116].copy_from_slice(&required_caps.to_le_bytes());

        // 5. String pool (offset 116..132)
        pkg[116..122].copy_from_slice(b"Beacon");

        // Verify preamble and sections parse correctly
        let preamble = KrxPreamble::parse(&pkg).expect("Valid preamble");
        assert_eq!(preamble.execution_class(), Some(KrxClass::Native));

        let mut ident_found = false;
        let mut exec_found = false;
        let mut type_found = false;
        for (hdr, body) in KrxSectionIterator::new(&pkg, 132) {
            match hdr.id {
                SECTION_IDENTITY => {
                    let ident = SectionIdentity::parse(body).unwrap();
                    assert_eq!(ident.name(&pkg[..132]), Some("Beacon"));
                    ident_found = true;
                }
                SECTION_EXECUTION => {
                    let exec = SectionExecution::parse(body).unwrap();
                    let payload_sz = exec.payload_size;
                    assert_eq!(payload_sz, 176);
                    assert_eq!(exec.profile(), Some(KrxProfile::N1NativePic));
                    exec_found = true;
                }
                SECTION_TYPE_LIFECYCLE => {
                    let st = SectionType::parse(body).unwrap();
                    let cap_mask = st.cap_mask;
                    assert_eq!(cap_mask, required_caps);
                    type_found = true;
                }
                _ => {}
            }
        }
        assert!(ident_found && exec_found && type_found);

        // Test Simulated Full App Lifecycle with state and ABI calls
        struct TestAppState {
            freq: u32,
            rendered_frames: u32,
            key_presses: u32,
            exited: bool,
        }
        static mut APP_STATE: TestAppState = TestAppState {
            freq: 0,
            rendered_frames: 0,
            key_presses: 0,
            exited: false,
        };

        let app_entry = |op: u32, api: *const KrxApiTable, param: *const ()| -> u32 {
            let table = unsafe { &*api };
            let state = unsafe { &mut *core::ptr::addr_of_mut!(APP_STATE) };
            match op {
                KRX_OP_START => {
                    state.freq = 145_500_000;
                    unsafe { ((*table.radio).rf_tune)(state.freq, 25, 12_500) };
                    0
                }
                KRX_OP_EVENT => {
                    if param.is_null() {
                        return 0;
                    }
                    let ev = unsafe { &*(param as *const KrxEvent) };
                    if ev.is_key() {
                        state.key_presses += 1;
                        if ev.key == 15 {
                            0 // KEY_EXIT: pass to system
                        } else {
                            1 // Consumed
                        }
                    } else {
                        0
                    }
                }
                KRX_OP_RENDER => {
                    state.rendered_frames += 1;
                    unsafe {
                        ((*table.display).draw_rect)(0, 0, 128, 64, 1);
                        ((*table.display).draw_str)(16, 26, b"BEACON".as_ptr(), 6, 0, 1);
                        ((*table.display).commit_frame)();
                    }
                    0
                }
                KRX_OP_EXIT => {
                    state.exited = true;
                    0
                }
                _ => 0xFFFF_FFFF,
            }
        };

        let api_table = KrxApiTable::safe_stub(required_caps);

        // 1. START
        assert_eq!(app_entry(KRX_OP_START, &api_table, core::ptr::null()), 0);
        assert_eq!(unsafe { APP_STATE.freq }, 145_500_000);

        // 2. RENDER
        assert_eq!(app_entry(KRX_OP_RENDER, &api_table, core::ptr::null()), 0);
        assert_eq!(unsafe { APP_STATE.rendered_frames }, 1);

        // 3. KEY EVENT (UP) -> Consumed (1)
        let ev_up = KrxEvent {
            kind: KRX_EV_KEY,
            key: 1,
            flags: 0,
            reserved: 0,
            payload: 0,
        };
        assert_eq!(
            app_entry(KRX_OP_EVENT, &api_table, &ev_up as *const _ as *const ()),
            1
        );
        assert_eq!(unsafe { APP_STATE.key_presses }, 1);

        // 4. KEY EVENT (EXIT) -> Pass to system (0)
        let ev_exit = KrxEvent {
            kind: KRX_EV_KEY,
            key: 15,
            flags: 0,
            reserved: 0,
            payload: 0,
        };
        assert_eq!(
            app_entry(KRX_OP_EVENT, &api_table, &ev_exit as *const _ as *const ()),
            0
        );
        assert_eq!(unsafe { APP_STATE.key_presses }, 2);

        // 5. EXIT
        assert_eq!(app_entry(KRX_OP_EXIT, &api_table, core::ptr::null()), 0);
        assert!(unsafe { APP_STATE.exited });

        // 6. High-Throughput Deterministic Dispatch Bench (10,000 rapid event & render cycles)
        unsafe {
            APP_STATE.exited = false;
        }
        for i in 0..10_000u32 {
            let tick_ev = KrxEvent {
                kind: KRX_EV_TICK,
                key: 0,
                flags: 0,
                reserved: 0,
                payload: i,
            };
            let res = app_entry(KRX_OP_EVENT, &api_table, &tick_ev as *const _ as *const ());
            assert_eq!(res, 0);

            if (i & 0x7F) == 0 {
                let r_res = app_entry(KRX_OP_RENDER, &api_table, core::ptr::null());
                assert_eq!(r_res, 0);
            }
        }
        assert_eq!(unsafe { APP_STATE.rendered_frames }, 1 + (10_000 / 128) + 1);
    }

    #[test]
    fn test_exhaustive_abi_vtable_verification_bench() {
        use crate::proc::abi::*;

        let api = KrxApiTable::safe_stub(0xFFFF_FFFF);

        // 1. CapDisplayVTable
        unsafe {
            let d = &*api.display;
            (d.clear)();
            (d.draw_pixel)(10, 20, 1);
            (d.draw_line)(0, 0, 128, 64, 1);
            (d.draw_rect)(5, 5, 20, 20, 1);
            (d.fill_rect)(10, 10, 15, 15, 1);
            let bmp = [0xFFu8; 8];
            (d.draw_bitmap)(0, 0, 8, 8, bmp.as_ptr(), 8);
            let s = b"TEST\0";
            (d.draw_str)(0, 0, s.as_ptr(), 4, 0, 1);
            (d.draw_str_align)(0, 0, s.as_ptr(), 4, 0, 1, 0);
            (d.draw_str_max)(0, 0, s.as_ptr(), 4, 0, 1, 64);
            (d.commit_frame)();
            (d.commit_rows)(0b00000001);
            let _fb = (d.get_framebuffer)();
            (d.set_inverted)(1);
            (d.set_contrast)(31);
            (d.draw_line_h)(0, 10, 50, 1);
            (d.draw_line_v)(10, 0, 50, 1);
            (d.draw_dotted_line_h)(0, 100, 25, 2, 1);
            (d.clear_corners)(10, 10, 30, 20);
            (d.fill_rect_dither)(10, 10, 30, 20, 8);
            (d.invert_rect)(10, 10, 30, 20);
            assert_eq!((d.str_width)(s.as_ptr(), 4, 0), 0);
            assert_eq!((d.draw_char)(0, 0, b'A', 0, 1), 6);
            (d.draw_icon)(0, 0, 0, 1);
            (d.scroll)(0, 1, 0);
        }

        // 2. CapRadioVTable
        unsafe {
            let r = &*api.radio;
            assert_eq!((r.rf_tune)(145_500_000, 25, 12_500), 1);
            assert_eq!((r.rf_get_freq)(), 0);
            assert_eq!((r.rf_get_rssi)(), -140);
            assert_eq!((r.rf_is_carrier)(), 0);
            (r.rf_set_modulation)(0);
            (r.rf_set_squelch)(3);
            assert_eq!((r.rf_set_tx)(0), 1);
            (r.rf_set_tx_power)(1);
            (r.rf_set_af_mode)(0);
            let mut sweep_buf = [0i16; 8];
            assert_eq!(
                (r.rf_sweep)(144_000_000, 25_000, 8, sweep_buf.as_mut_ptr()),
                0
            );
            let sym = KrxSymbol {
                freq_word: 1200,
                duration_us: 833,
            };
            assert_eq!((r.rf_symbol_tx_emit)(&sym, 1), 1);
            (r.rf_set_dual_tones)(100, 10, 200, 10);
            (r.rf_fsk_init)(1200);
            let fsk_data = b"FSK_TEST";
            assert_eq!((r.rf_fsk_send)(fsk_data.as_ptr(), 8), 1);
            let mut fsk_rx = [0u8; 16];
            assert_eq!((r.rf_fsk_recv)(fsk_rx.as_mut_ptr(), 16), 0);
            (r.rf_set_mic_mute)(1);
            assert_eq!((r.rf_set_tx_carrier)(0), 1);
            (r.rf_set_subaudible_ctcss)(670);
            (r.rf_set_subaudible_dcs)(0o023, 0);
            (r.rf_send_tail)();
            assert_eq!((r.rf_get_bandwidth)(), 12_500);
            assert_eq!((r.rf_get_modulation)(), 0);
            assert_eq!((r.rf_get_tx_state)(), 0);
            assert_eq!((r.rf_get_glitch)(), 0);
            (r.rf_set_agc)(0);
            (r.rf_set_dual_tones_hz)(700, 15, 900, 15);
            (r.rf_set_dtmf)(1, 20);
            assert_eq!((r.rf_get_snr)(), 0);
            assert_eq!((r.rf_get_noise)(), 0);
        }

        // 3. CapAudioVTable
        unsafe {
            let a = &*api.audio;
            (a.audio_play_tone)(800, 50);
            (a.audio_play_beep)(1);
            (a.audio_set_path)(1);
            assert_eq!((a.audio_capture_start)(8000), 1);
            let mut audio_buf = [0i16; 16];
            assert_eq!((a.audio_capture_read)(audio_buf.as_mut_ptr(), 16), 0);
            (a.audio_capture_stop)();
            (a.audio_set_volume)(7);
            assert_eq!((a.audio_get_volume)(), 5);
            (a.audio_stop)();
            assert_eq!((a.audio_is_playing)(), 0);
            (a.audio_set_mic_gain)(16);
            assert_eq!((a.audio_read_mic_sample)(), 0);
            assert_eq!((a.audio_is_capturing)(), 0);
        }

        // 4. CapStorageVTable
        unsafe {
            let s = &*api.storage;
            let mut cfg_buf = [0u8; 16];
            assert_eq!((s.storage_load_config)(cfg_buf.as_mut_ptr(), 16), 0);
            assert_eq!((s.storage_save_config)(cfg_buf.as_ptr(), 16), 1);
            assert_eq!((s.storage_read_asset)(1, 0, cfg_buf.as_mut_ptr(), 16), 0);
            let path = b"/user/test.txt\0";
            assert_eq!(
                (s.storage_read_file)(path.as_ptr(), 14, cfg_buf.as_mut_ptr(), 16),
                0
            );
            assert_eq!(
                (s.storage_write_file)(path.as_ptr(), 14, cfg_buf.as_ptr(), 16),
                1
            );
            assert_eq!((s.storage_remove_file)(path.as_ptr(), 14), 1);
            assert_eq!((s.storage_file_size)(path.as_ptr(), 14), 0);
            let mut entries = [KrxDirEntry {
                name: [0; 24],
                size: 0,
                is_dir: 0,
                reserved: [0; 3],
            }];
            assert_eq!(
                (s.storage_list_dir)(b"/\0".as_ptr(), 1, entries.as_mut_ptr(), 1),
                0
            );
            assert_eq!((s.storage_file_exists)(path.as_ptr(), 14), 0);
            assert_eq!(
                (s.storage_read_file_at)(path.as_ptr(), 14, 0, cfg_buf.as_mut_ptr(), 16),
                0
            );
            assert_eq!(
                (s.storage_append_file)(path.as_ptr(), 14, cfg_buf.as_ptr(), 16),
                1
            );
            let new_path = b"/user/new.txt\0";
            assert_eq!(
                (s.storage_rename_file)(path.as_ptr(), 14, new_path.as_ptr(), 13),
                1
            );
            assert_eq!((s.storage_create_dir)(b"/user/data\0".as_ptr(), 10), 1);
        }

        // 5. CapKeypadVTable
        unsafe {
            let k = &*api.keypad;
            assert_eq!((k.sys_poll_key)(), 0);
            assert_eq!((k.sys_nav_dir)(1), 0);
            assert_eq!((k.sys_nav_dir)(2), 0);
            assert_eq!((k.sys_key_matrix)(), 0);
            assert_eq!((k.sys_is_key_pressed)(1), 0);
            assert_eq!((k.sys_is_f_active)(), 0);
            assert_eq!((k.sys_is_ptt_pressed)(), 0);
            assert_eq!((k.sys_is_side1_pressed)(), 0);
            assert_eq!((k.sys_is_side2_pressed)(), 0);
            (k.sys_clear_key_buffer)();
            assert_eq!((k.sys_is_keypad_locked)(), 0);
        }

        // 6. CapPowerVTable
        unsafe {
            let p = &*api.power;
            assert_eq!((p.sys_battery_mv)(), 4000);
            assert_eq!((p.sys_battery_pct)(), 100);
            assert_eq!((p.sys_battery_ma)(), 0);
            assert_eq!((p.sys_battery_type)(), 0);
            assert_eq!((p.sys_raw_battery_adc)(), 0);
            assert_eq!((p.sys_mcu_temp)(), 250);
            assert_eq!((p.sys_is_charging)(), 0);
            assert_eq!((p.sys_is_low)(), 0);
            assert_eq!((p.sys_is_critical)(), 0);
            (p.sys_set_backlight)(1, 10);
            (p.sys_set_backlight_brightness)(255, 32);
            (p.sys_wake_backlight)();
            (p.sys_set_flashlight)(0);
            assert_eq!((p.sys_cpu_freq_hz)(), 48_000_000);
        }

        // 7. CapSerialVTable
        unsafe {
            let ser = &*api.serial;
            let msg = b"TELEMETRY";
            assert_eq!((ser.serial_write)(msg.as_ptr(), 9), 0);
            let mut ser_rx = [0u8; 16];
            assert_eq!((ser.serial_read)(ser_rx.as_mut_ptr(), 16), 0);
            assert_eq!((ser.serial_is_connected)(), 0);
            (ser.serial_flush)();
        }

        // 8. CapTimerVTable
        unsafe {
            let t = &*api.timer;
            let _ = (t.sys_ticks_ms)();
            let _ = (t.sys_uptime_secs)();
            (t.sys_delay_ms)(0);
            (t.sys_delay_us)(0);
            let mut rtc_buf = [0u8; 7];
            assert_eq!((t.sys_rtc_get)(rtc_buf.as_mut_ptr()), 1);
        }

        // 9. CapUiVTable (Compositor Spaces)
        unsafe {
            let u = &*api.ui;
            (u.ui_close_modal)();
            assert_eq!((u.ui_has_modal)(), 0);
            let title = b"PROMPT\0";
            let msg = b"BODY\0";
            (u.ui_show_popup)(1, title.as_ptr(), 6, msg.as_ptr(), 4);
            (u.ui_show_confirm)(2, title.as_ptr(), 6, msg.as_ptr(), 4);
            (u.ui_prompt_text)(3, title.as_ptr(), 6, b"\0".as_ptr(), 0, 16);
            let mut modal_res = [0u8; 16];
            assert_eq!((u.ui_get_modal_text)(modal_res.as_mut_ptr(), 16), 0);
            (u.ui_show_toast)(b"ALERT\0".as_ptr(), 5, 1500);
            assert_eq!((u.ui_is_animating)(), 0);
            let sb = KrxStatusBar::empty();
            assert_eq!((u.ui_set_status_bar)(&sb), 1);
            let mut read_sb = KrxStatusBar::empty();
            (u.ui_get_status_bar)(&mut read_sb);
            assert_eq!((u.ui_set_tray_icon)(15), 1);
            (u.ui_draw_progress_bar)(0, 0, 50, 6, 75, 1);
            (u.ui_draw_s_meter)(0, 10, -90, 8);
            (u.ui_draw_tag_badge)(0, 20, b"FM\0".as_ptr(), 2, 1);
            (u.ui_draw_key_badge)(0, 30, b"M\0".as_ptr(), 1, b"Menu\0".as_ptr(), 4);
            (u.ui_draw_capsule_button)(0, 40, 40, 10, b"OK\0".as_ptr(), 2, 1);
            (u.ui_draw_scrollbar)(120, 0, 50, 1, 10, 4);
            (u.ui_draw_menu_item)(0, b"Item\0".as_ptr(), 4, b"Val\0".as_ptr(), 3, 1, 0);
            (u.ui_draw_big_digit)(0, 0, 5, 1);
            (u.ui_draw_small_digit)(20, 0, 9, 1);
            (u.ui_draw_battery)(0, 50, 85, 1);
            (u.ui_draw_signal)(0, 55, 4, 1);
            let mut origin = [0u8; 2];
            (u.ui_draw_modal_box)(60, 40, origin.as_mut_ptr());
            (u.ui_show_confirm_labels)(
                4,
                title.as_ptr(),
                6,
                msg.as_ptr(),
                4,
                b"YES\0".as_ptr(),
                3,
                b"NO\0".as_ptr(),
                2,
            );
        }

        // 10. CapMathVTable
        unsafe {
            let m = &*api.math;
            assert_eq!((m.sys_rand32)(), 0x5A5A_5A5A);
            assert_eq!((m.crc32)(b"123456789".as_ptr(), 9), 0);
            assert_eq!((m.crc16)(b"123456789".as_ptr(), 9), 0);
            assert_eq!((m.isqrt)(144), 12);
            assert_eq!((m.isqrt)(256), 16);
            assert_eq!((m.idivmod)(100, 7), 0);
            assert_eq!((m.uidivmod)(100, 7), 0);
        }

        // 11. CapFmVTable
        unsafe {
            let f = &*api.fm;
            (f.fm_tune)(1043);
            (f.fm_set_volume)(12);
            (f.fm_mute)(0);
            assert_eq!((f.fm_get_rssi)(), 0);
        }

        // 12. CapServiceVTable
        unsafe {
            let svc = &*api.service;
            assert_eq!((svc.svc_register)(0x1234, 1, 0x2000_0000), 1);
            assert_eq!((svc.svc_resolve)(0x1234, 1), 0);
            assert_eq!((svc.svc_release)(0x1234), 1);
            assert_eq!(
                (svc.ipc_send)(b"Radio\0".as_ptr(), 5, b"TUNE\0".as_ptr(), 4),
                1
            );
            let mut ipc_out = [0u8; 16];
            assert_eq!((svc.ipc_poll)(ipc_out.as_mut_ptr(), 16), 0xFF);
            let mut uid = [0u8; 12];
            (svc.sys_device_uid)(uid.as_mut_ptr());
            assert_eq!((svc.sys_os_version)(), 0x0003_0000);
            assert_eq!((svc.sys_ram_available)(), 4096);
            assert_eq!((svc.sys_stack_margin)(), 1024);
            assert_eq!((svc.sys_proc_count)(), 0);
            let mut p_info = KrxProcInfo {
                pid: 0,
                app_id: 0,
                state: 0,
                priority: 0,
                runtime_ticks: 0,
                memory_bytes: 0,
                name: [0; 12],
                exec_type: 0,
                flags: 0,
                stack_watermark: 0,
                ipc_events: 0,
                page_faults: 0,
                slice_overruns: 0,
                hibernations: 0,
                fault_count: 0,
            };
            assert_eq!((svc.sys_proc_info)(0, &mut p_info), 1);
            assert_eq!((svc.sys_proc_kill)(99), 1);
            assert_eq!((svc.paged_call)(2, 0x40, 0x88), 0);
            assert_eq!((svc.paged_return)(), 0);
            assert_eq!((svc.leaf_call)(0, 100), 0xFFFF_FFFF);
            assert_eq!((svc.lease_acquire)(0, 1000), 1);
            assert_eq!((svc.lease_release)(0), 1);
            let mut mem_desc = crate::proc::MemoryRegionDescriptor {
                kind: 0,
                flags: 0,
                reserved: 0,
                base_vma: 0,
                size_bytes: 0,
                alignment: 4,
                pad: [0; 2],
            };
            assert_eq!((svc.mem_query)(0, &mut mem_desc), 1);
            assert_eq!((svc.sys_post_event)(1), 0);
            assert_eq!((svc.sys_publish_event)(100, b"DATA".as_ptr(), 4), 0);
            assert_eq!((svc.sys_stack_free_min)(), 1024);
            let ipc_msg =
                crate::ipc::KrxIpcMessage::new(1, 2, crate::ipc::ipc_msg_type::PING, b"PING");
            assert_eq!((svc.ipc_send_msg)(&ipc_msg), 1);
            let mut recv_msg = crate::ipc::KrxIpcMessage::empty();
            assert_eq!((svc.ipc_recv_msg)(&mut recv_msg), 1);
            assert_eq!((svc.ipc_reply)(&ipc_msg, b"PONG".as_ptr(), 4), 1);
            assert_eq!((svc.sys_find_proc)(b"Radio\0".as_ptr(), 5), 0);
            assert_eq!((svc.sys_self_pid)(), 1);
        }

        // 13. CapHwBusVTable
        unsafe {
            let hw = &*api.hw_bus;
            let tx = [0x12, 0x34];
            let mut rx = [0u8; 2];
            assert_eq!(
                (hw.hw_i2c_transfer)(0x38, 0x10, tx.as_ptr(), 2, rx.as_mut_ptr(), 2),
                1
            );
            assert_eq!((hw.hw_spi_transfer)(1, tx.as_ptr(), rx.as_mut_ptr(), 2), 1);
            (hw.hw_gpio_set)(5, 1);
            assert_eq!((hw.hw_gpio_get)(5), 0);
            assert_eq!((hw.hw_rf_read_reg)(0x30), 0);
            (hw.hw_rf_write_reg)(0x30, 0x1234);
            let mut flash_buf = [0u8; 16];
            assert_eq!(
                (hw.hw_flash_read)(0x0800_0000, flash_buf.as_mut_ptr(), 16),
                1
            );
        }
    }

    #[test]
    fn test_paged_execution_with_shadow_stack_and_leafs_bench() {
        use crate::proc::paged::*;

        // 1. Verify ShadowReturnStack push/pop and nesting
        let mut stack = ShadowReturnStack::<4>::new();
        assert_eq!(stack.sp, 0);

        // Simulate nested paged calls:
        // Frame 1: caller in block 1 at offset 0x0100 calls callee in block 2, return offset 0x0120
        let frame1 = ReturnFrame {
            caller_block_id: 1,
            caller_slot: 0,
            return_offset: 0x0120,
        };
        assert!(stack.push(frame1).is_ok());
        assert_eq!(stack.sp, 1);

        // Frame 2: block 2 at offset 0x0040 calls callee in block 3, return offset 0x0060
        let frame2 = ReturnFrame {
            caller_block_id: 2,
            caller_slot: 1,
            return_offset: 0x0060,
        };
        assert!(stack.push(frame2).is_ok());
        assert_eq!(stack.sp, 2);

        // Callee in block 3 completes, executes paged_return() -> restores frame2
        let popped2 = stack.pop().expect("Pop frame 2");
        assert_eq!(popped2.caller_block_id, 2);
        assert_eq!(popped2.return_offset, 0x0060);
        assert_eq!(stack.sp, 1);

        // Callee in block 2 completes, executes paged_return() -> restores frame1
        let popped1 = stack.pop().expect("Pop frame 1");
        assert_eq!(popped1.caller_block_id, 1);
        assert_eq!(popped1.return_offset, 0x0120);
        assert_eq!(stack.sp, 0);

        // Underflow protection
        assert_eq!(stack.pop().unwrap_err(), PagedError::ShadowStackUnderflow);

        // Overflow protection
        for i in 0..4 {
            assert!(
                stack
                    .push(ReturnFrame {
                        caller_block_id: i as u8,
                        caller_slot: 0,
                        return_offset: 0x10,
                    })
                    .is_ok()
            );
        }
        assert_eq!(
            stack
                .push(ReturnFrame {
                    caller_block_id: 99,
                    caller_slot: 0,
                    return_offset: 0x10,
                })
                .unwrap_err(),
            PagedError::ShadowStackOverflow
        );

        // 2. StructuredLeafManager overlay dispatch and swapping
        let mut leaf_mgr = StructuredLeafManager::<4>::new(0x2000_1000, 1024);
        assert_eq!(leaf_mgr.leaf_count, 0);
        assert_eq!(leaf_mgr.total_swaps, 0);

        // Configure Leaf 0 (Math/DSP) and Leaf 1 (Protocol Packet Demod)
        leaf_mgr.leaves[0] = LeafEntry {
            leaf_id: 0,
            flags: 0,
            size: 256,
            flash_offset: 0x1000,
            entry_offset: 0x00,
            ram_scratch_bytes: 0,
            reserved: [0u8; 4],
        };
        leaf_mgr.leaves[1] = LeafEntry {
            leaf_id: 1,
            flags: 0,
            size: 512,
            flash_offset: 0x1800,
            entry_offset: 0x04,
            ram_scratch_bytes: 0,
            reserved: [0u8; 4],
        };
        leaf_mgr.leaf_count = 2;

        let mut storage_reads = 0u32;
        // Dispatch Leaf 0 -> Cache Miss (loads from storage)
        let res0 = leaf_mgr
            .dispatch_leaf_trampoline(0, 42, 8, |_entry, is_hit, _ctx| {
                if !is_hit {
                    storage_reads += 1;
                }
                Ok(42 * 2)
            })
            .expect("Dispatch Leaf 0");
        assert_eq!(res0, 84);
        assert_eq!(storage_reads, 1);
        assert_eq!(leaf_mgr.active_leaf, Some(0));

        // Dispatch Leaf 0 again -> Cache Hit (no storage read!)
        let res0_hit = leaf_mgr
            .dispatch_leaf_trampoline(0, 10, 8, |_entry, is_hit, _ctx| {
                if !is_hit {
                    storage_reads += 1;
                }
                Ok(10 * 2)
            })
            .expect("Dispatch Leaf 0 Hit");
        assert_eq!(res0_hit, 20);
        assert_eq!(storage_reads, 1); // Still 1, 0 additional reads!

        // Dispatch Leaf 1 -> Overlay Swap (loads Leaf 1 into window)
        let res1 = leaf_mgr
            .dispatch_leaf_trampoline(1, 100, 8, |_entry, is_hit, _ctx| {
                if !is_hit {
                    storage_reads += 1;
                }
                Ok(100 + 7)
            })
            .expect("Dispatch Leaf 1");
        assert_eq!(res1, 107);
        assert_eq!(storage_reads, 2);
        assert_eq!(leaf_mgr.active_leaf, Some(1));
        assert_eq!(leaf_mgr.total_swaps, 2);
    }

    #[test]
    fn test_streamed_pages_and_lru_cache_bench() {
        use crate::proc::paged::*;

        let mut cache = FixedWindowCache::<8, 2>::new();
        // Configure 4 uniform 1024-byte blocks across 2 cache slots
        cache.configure(1024, 2, 0x10000, 4096, None);
        assert_eq!(cache.active_slots, 2);
        assert_eq!(cache.block_count, 4);

        // Access Block 0 -> Miss, allocated to Slot 0
        let (slot0, hit0, _entry0) = cache.resolve_or_select_slot(0).expect("Alloc block 0");
        assert!(!hit0);
        assert_eq!(slot0, 0);
        assert_eq!(cache.fault_count, 1);
        assert_eq!(cache.hit_count, 0);

        // Access Block 1 -> Miss, allocated to Slot 1
        let (slot1, hit1, _entry1) = cache.resolve_or_select_slot(1).expect("Alloc block 1");
        assert!(!hit1);
        assert_eq!(slot1, 1);
        assert_eq!(cache.fault_count, 2);
        assert_eq!(cache.hit_count, 0);

        // Access Block 0 again -> Cache Hit!
        let (slot0_again, hit0_again, _entry0_again) =
            cache.resolve_or_select_slot(0).expect("Hit block 0");
        assert!(hit0_again);
        assert_eq!(slot0_again, 0);
        assert_eq!(cache.fault_count, 2);
        assert_eq!(cache.hit_count, 1);

        // Access Block 2 -> All slots full! Evicts least recently used (Block 1 in Slot 1)
        let (slot_evicted, hit_evicted, _entry2) =
            cache.resolve_or_select_slot(2).expect("Evict block 1");
        assert!(!hit_evicted);
        assert_eq!(slot_evicted, 1); // Evicted slot 1
        assert_eq!(cache.fault_count, 3);
        assert_eq!(cache.slots[1].resident_block, Some(2));
        assert_eq!(cache.slots[0].resident_block, Some(0)); // Block 0 kept
    }

    #[test]
    fn test_declarative_framework_and_compositor_bench() {
        use crate::event::KeyCode;
        use crate::proc::declare::*;

        let mut walker = DeclareWalker::new("Telemetry");
        let _ = walker.nodes.push(DeclareNode {
            node_id: 0,
            layout: LAYOUT_MENU,
            item_count: 4,
            title_offset: 0,
            first_item_offset: 0,
        });
        for i in 0..4 {
            let _ = walker.items.push(DeclareItem {
                kind: ITEM_ACTION,
                flags: 0,
                label_offset: i as u16,
                target: 0,
                value_ref: 0,
                aux: 0,
                reserved: 0,
            });
        }

        // Verify initial state
        assert_eq!(walker.current_node_idx, 0);
        assert_eq!(walker.selected_item, 0);

        // Test key navigation (Down)
        let handled_down = walker.handle_key(KeyCode::Down);
        assert!(handled_down);
        assert_eq!(walker.selected_item, 1);

        // Test key navigation (Up)
        let handled_up = walker.handle_key(KeyCode::Up);
        assert!(handled_up);
        assert_eq!(walker.selected_item, 0);

        // Test rule triggering with boot event
        walker.trigger_rules(TRIGGER_BOOT, 0);
        assert_eq!(walker.take_command(), DeclareCommand::None);

        // Verify navigation stack push and pop
        assert_eq!(walker.nav_stack.len(), 0);
        let frame = NavFrame {
            node_idx: 0,
            selected_item: 0,
            scroll_top: 0,
        };
        assert!(walker.nav_stack.push(frame).is_ok());
        assert_eq!(walker.nav_stack.len(), 1);
        assert_eq!(walker.nav_stack.pop(), Some(frame));
        assert_eq!(walker.nav_stack.len(), 0);
    }

    #[test]
    fn test_deep_edge_case_ram_orchestration_and_process_stress_bench() {
        use crate::caps::CapabilitySet;
        use crate::proc::paged::*;
        use crate::proc::*;
        use crate::sched::Priority;
        use crate::sys::MemoryPressure;

        // ══════════════════════════════════════════════════════════════════
        // 1. RAM ORCHESTRATION & MEMORY PRESSURE EDGE CASES
        // ══════════════════════════════════════════════════════════════════

        // A. Boundary limits on SectionExecution::can_execute
        let exec = SectionExecution {
            min_stack_bytes: 512,
            min_heap_bytes: 256,
            page_size: 1024,
            cache_blocks: 2,
            dma_burst_size: 4,
            entry_point: 0x2000_0001,
            payload_size: 1024,
            reloc_offset: 0,
            reloc_count: 0,
            got_offset: 0,
            rt_pin_mask: 0,
            max_fault_latency_us: 100,
            reserved: [0; 2],
        };

        // Stack margin required = 256 bytes -> Total needed = 512 + 256 + 256 = 1024 bytes
        assert!(!exec.can_execute(1023, 256), "1 byte under must fail");
        assert!(
            exec.can_execute(1024, 256),
            "Exact required RAM must succeed"
        );
        assert!(exec.can_execute(4096, 256), "Abundant RAM must succeed");
        assert!(!exec.can_execute(0, 0), "Zero RAM must fail");

        // Arithmetic overflow defense
        let huge_exec = SectionExecution {
            min_stack_bytes: u16::MAX,
            min_heap_bytes: u16::MAX,
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
        };
        assert!(!huge_exec.can_execute(1024, usize::MAX));

        // B. Memory Pressure State Machine Verification
        assert_eq!(MemoryPressure::Normal.as_str(), "NORM");
        assert_eq!(MemoryPressure::Moderate.as_str(), "MOD");
        assert_eq!(MemoryPressure::Constrained.as_str(), "WARN");
        assert_eq!(MemoryPressure::Critical.as_str(), "CRIT");

        // ══════════════════════════════════════════════════════════════════
        // 2. DEEP TASK MANAGING & SATURATED PROCESS TABLE STRESS
        // ══════════════════════════════════════════════════════════════════
        let mut table = ProcessTable::<8>::new();

        // Spawn 4 system critical daemons (PID <= 4, protected)
        let d1 = table
            .spawn(
                "RADIO_DAEMON",
                0xF1,
                Priority::Safety,
                ExecType::StaticFirmware,
                CapabilitySet::ALL,
            )
            .unwrap();
        let d2 = table
            .spawn(
                "POWER_MGR",
                0xF2,
                Priority::Safety,
                ExecType::StaticFirmware,
                CapabilitySet::ALL,
            )
            .unwrap();
        let d3 = table
            .spawn(
                "AUDIO_SRV",
                0xF3,
                Priority::Radio,
                ExecType::StaticFirmware,
                CapabilitySet::ALL,
            )
            .unwrap();
        let d4 = table
            .spawn(
                "STORAGE_SRV",
                0xF4,
                Priority::Capture,
                ExecType::StaticFirmware,
                CapabilitySet::ALL,
            )
            .unwrap();

        assert_eq!(d1.0, 1);
        assert_eq!(d2.0, 2);
        assert_eq!(d3.0, 3);
        assert_eq!(d4.0, 4);

        // Spawn 4 user processes to completely fill the 8 slots
        let u1 = table
            .spawn(
                "VFO_APP",
                1,
                Priority::Ui,
                ExecType::RelocatableNative,
                CapabilitySet::NONE,
            )
            .unwrap();
        let u2 = table
            .spawn(
                "SETTINGS",
                2,
                Priority::Ui,
                ExecType::RelocatableNative,
                CapabilitySet::NONE,
            )
            .unwrap();
        let u3 = table
            .spawn(
                "BEACON_KRX",
                3,
                Priority::Idle,
                ExecType::RelocatableNative,
                CapabilitySet::NONE,
            )
            .unwrap();
        let u4 = table
            .spawn(
                "BENCH_KRX",
                4,
                Priority::Idle,
                ExecType::RelocatableNative,
                CapabilitySet::NONE,
            )
            .unwrap();

        assert_eq!(u4.0, 8);
        assert!(
            table
                .spawn(
                    "FAIL_APP",
                    5,
                    Priority::Idle,
                    ExecType::RelocatableNative,
                    CapabilitySet::NONE
                )
                .is_err(),
            "Table must be full"
        );

        // Suspend user apps with distinct CPU tick counts
        if let Some(pcb) = table.find_mut(u1) {
            pcb.set_state(ProcState::Suspended);
            pcb.ledger.cpu_ticks = 500;
        }
        if let Some(pcb) = table.find_mut(u2) {
            pcb.set_state(ProcState::Suspended);
            pcb.ledger.cpu_ticks = 300; // Victim 2
        }
        if let Some(pcb) = table.find_mut(u3) {
            pcb.set_state(ProcState::Suspended);
            pcb.ledger.cpu_ticks = 100; // Victim 1 (lowest CPU ticks)
        }
        if let Some(pcb) = table.find_mut(u4) {
            pcb.set_state(ProcState::Running); // Cannot evict running app
            pcb.ledger.cpu_ticks = 50;
        }

        // Test spawn_or_recycle under extreme saturation
        // Must recycle u3 (lowest CPU ticks among suspended user apps: 100 ticks)
        let recycled1 = table
            .spawn_or_recycle(
                "STREAM_P1",
                5,
                Priority::Ui,
                ExecType::RelocatableNative,
                CapabilitySet::NONE,
            )
            .unwrap();
        assert!(
            table.find(u3).is_none(),
            "Lowest tick suspended process u3 must be evicted"
        );
        assert_eq!(table.find(recycled1).unwrap().name, "STREAM_P1");

        // Next victim must be u2 (300 ticks)
        let recycled2 = table
            .spawn_or_recycle(
                "LEAF_P0",
                6,
                Priority::Ui,
                ExecType::RelocatableNative,
                CapabilitySet::NONE,
            )
            .unwrap();
        assert!(table.find(u2).is_none(), "u2 must be evicted next");
        assert_eq!(table.find(recycled2).unwrap().name, "LEAF_P0");

        // System daemons (PID 1..4) must NEVER be evicted under any circumstances
        for pid_val in 1..=4 {
            assert!(
                table.find(Pid(pid_val)).is_some(),
                "Critical daemon PID {} must survive saturation eviction",
                pid_val
            );
        }

        // ══════════════════════════════════════════════════════════════════
        // 3. DEMAND-PAGED STREAMING & LRU CACHE THRASHING STRESS
        // ══════════════════════════════════════════════════════════════════
        let mut cache = FixedWindowCache::<16, 2>::new();
        // Setup 16 blocks: Block 0 has BLOCK_FLAG_PINNED (1)
        let mut block_map_bytes = [0u8; 16 * BlockEntry::SIZE];
        for b in 0..16 {
            let offset = b * BlockEntry::SIZE;
            block_map_bytes[offset] = b as u8; // block_id
            block_map_bytes[offset + 1] = if b == 0 { BLOCK_FLAG_PINNED } else { 0 }; // flags
            block_map_bytes[offset + 2..offset + 4].copy_from_slice(&1024u16.to_le_bytes()); // size = 1024
            block_map_bytes[offset + 4..offset + 8]
                .copy_from_slice(&((b * 1024) as u32).to_le_bytes()); // flash_offset
        }
        cache.configure(1024, 2, 0, 16384, Some(&block_map_bytes));
        assert_eq!(cache.active_slots, 2);
        assert_eq!(cache.block_count, 16);

        // Resolve Block 0 -> Miss, allocated to Slot 0 (pinned!)
        let (s0, hit0, entry0) = cache.resolve_or_select_slot(0).unwrap();
        assert_eq!(s0, 0);
        assert!(!hit0);
        assert!(entry0.is_pinned());
        assert!(cache.slots[0].pinned);

        // Stream blocks 1 through 15 sequentially through the single remaining unpinned slot (Slot 1)
        for blk in 1..=15 {
            let (slot, hit, _) = cache.resolve_or_select_slot(blk).unwrap();
            assert_eq!(
                slot, 1,
                "Pinned Slot 0 must NEVER be evicted; Block {} must use Slot 1",
                blk
            );
            assert!(!hit);
            assert_eq!(
                cache.slots[0].resident_block,
                Some(0),
                "Slot 0 must remain pinned to Block 0"
            );
        }

        // Verify hit on pinned block 0
        let (s0_hit, hit0_again, _) = cache.resolve_or_select_slot(0).unwrap();
        assert_eq!(s0_hit, 0);
        assert!(hit0_again, "Pinned Block 0 must be a cache hit");

        // ══════════════════════════════════════════════════════════════════
        // 4. DEEP SHADOW RETURN STACK NESTING STRESS
        // ══════════════════════════════════════════════════════════════════
        let mut shadow = ShadowReturnStack::<4>::new();
        for i in 0..4 {
            assert!(
                shadow
                    .push(ReturnFrame {
                        caller_block_id: i,
                        caller_slot: (i % 2) as u8,
                        return_offset: (i * 0x40) as u16,
                    })
                    .is_ok()
            );
        }
        assert_eq!(
            shadow.push(ReturnFrame {
                caller_block_id: 99,
                caller_slot: 0,
                return_offset: 0
            }),
            Err(PagedError::ShadowStackOverflow)
        );

        for i in (0..4).rev() {
            let frame = shadow.pop().unwrap();
            assert_eq!(frame.caller_block_id, i);
            assert_eq!(frame.return_offset, (i * 0x40) as u16);
        }
        assert_eq!(shadow.pop(), Err(PagedError::ShadowStackUnderflow));

        // ══════════════════════════════════════════════════════════════════
        // 5. STRUCTURED LEAF MULTI-HOP TRAMPOLINE CHAIN STRESS
        // ══════════════════════════════════════════════════════════════════
        let mut leaf_mgr = StructuredLeafManager::<8>::new(0x2000_1000, 1024);
        // Setup synthetic leaf map: Leaf 1..4 (16 bytes per LeafEntry)
        let leaf_map_bytes = [
            1, 0, 0x80, 0x00, 0x10, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0,
            0, // Leaf 1: offset 0x10, size 128
            2, 0, 0x80, 0x00, 0x90, 0x00, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0,
            0, // Leaf 2: offset 0x90, size 128
            3, 0, 0x80, 0x00, 0x10, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0,
            0, // Leaf 3: offset 0x110, size 128
            4, 0, 0x80, 0x00, 0x90, 0x01, 0x00, 0x00, 0, 0, 0, 0, 0, 0, 0,
            0, // Leaf 4: offset 0x190, size 128
        ];
        leaf_mgr.configure(0x2000_1000, 1024, Some(&leaf_map_bytes));
        assert_eq!(leaf_mgr.leaf_count, 4);

        // Verify dispatch and address resolution
        let (entry1, hit1) = leaf_mgr.begin_leaf_call(1).unwrap();
        let flash_offset1 = entry1.flash_offset;
        assert_eq!(flash_offset1, 0x10);
        assert!(!hit1);
        let cont = leaf_mgr
            .end_leaf_call(LeafContinuation::call_leaf(2, 52))
            .unwrap();
        assert!(cont.is_call_leaf());

        let (entry2, hit2) = leaf_mgr.begin_leaf_call(2).unwrap();
        let flash_offset2 = entry2.flash_offset;
        assert_eq!(flash_offset2, 0x90);
        assert!(!hit2);
        let cont2 = leaf_mgr.end_leaf_call(LeafContinuation::done(104)).unwrap();
        assert!(cont2.is_done());
        assert_eq!(cont2.payload, 104);

        assert_eq!(
            leaf_mgr.begin_leaf_call(99).unwrap_err(),
            PagedError::InvalidLeafId
        );
    }

    #[test]
    fn test_krx_multi_type_lifecycle_and_flags() {
        use crate::caps::Cap;

        // Construct synthetic Daemon KRX binary package in memory
        let mut pkg = [0u8; 128];

        // 1. Preamble (24 bytes)
        pkg[0..4].copy_from_slice(&KRX_IDENT);
        pkg[4] = KrxClass::Native as u8;
        pkg[5] = 5; // format_version
        pkg[6..8].copy_from_slice(&3u16.to_le_bytes()); // section_count = 3
        pkg[8..10].copy_from_slice(&124u16.to_le_bytes()); // header_len = 124
        pkg[10..12].copy_from_slice(&0u16.to_le_bytes()); // flags = 0
        pkg[12..16].copy_from_slice(&0u32.to_le_bytes()); // payload_crc32

        // Sections layout:
        // Offset 24: SectionIdentity (4B header + 40B body = 44B, ends at 68)
        pkg[24..26].copy_from_slice(&SECTION_IDENTITY.to_le_bytes());
        pkg[26..28].copy_from_slice(&44u16.to_le_bytes());
        pkg[28..30].copy_from_slice(&108u16.to_le_bytes()); // name_str offset = 108
        pkg[30] = 1; // version major
        pkg[51] = 7; // icon_id = 7

        // Offset 68: SectionTypeLifecycle (4B header + 12B body = 16B, ends at 84)
        let flags =
            SectionType::FLAG_STARTUP | SectionType::FLAG_PERSISTENT | SectionType::FLAG_SINGLETON;
        let daemon_caps =
            (Cap::RadioRx as u32) | (Cap::AudioOutput as u32) | (Cap::SerialComm as u32);
        pkg[68..70].copy_from_slice(&SECTION_TYPE_LIFECYCLE.to_le_bytes());
        pkg[70..72].copy_from_slice(&16u16.to_le_bytes());
        pkg[72] = KrxExecType::Daemon as u8;
        pkg[73] = flags;
        pkg[74] = 2; // stack_cost
        pkg[80..84].copy_from_slice(&daemon_caps.to_le_bytes());

        // Offset 84: SectionExecution (4B header + 28B body = 32B, ends at 116)
        pkg[84..86].copy_from_slice(&SECTION_EXECUTION.to_le_bytes());
        pkg[86..88].copy_from_slice(&32u16.to_le_bytes());
        pkg[88..90].copy_from_slice(&256u16.to_le_bytes()); // min_stack_bytes

        // Offset 116: String Pool ("AprsDaemon\0")
        pkg[28..30].copy_from_slice(&116u16.to_le_bytes()); // name_str offset = 116
        let name_bytes = b"AprsDaemon\0";
        pkg[116..116 + name_bytes.len()].copy_from_slice(name_bytes);

        // Header total length: 128 bytes (padded to 4-byte boundary)
        pkg[8..10].copy_from_slice(&128u16.to_le_bytes()); // header_len = 128

        // Compute IEEE 802.3 CRC32 of header content (pkg[24..128])
        let header_crc = crypto::crc::CrcEngine::crc32(&pkg[24..128]);
        pkg[16..20].copy_from_slice(&header_crc.to_le_bytes());

        // Parse and summarize
        let summary = KrxPreamble::summarize(&pkg[..128]).expect("Valid summary");
        assert_eq!(summary.preamble.app_type, KrxClass::Native as u8);
        assert_eq!(summary.title, "AprsDaemon");
        let exec = summary.exec.expect("Has exec section");
        let min_stack = exec.min_stack_bytes;
        assert_eq!(min_stack, 256);
        assert_eq!(summary.exec_type, KrxExecType::Daemon);
        assert_eq!(summary.type_flags, flags);
        assert!(summary.is_startup());
        assert!(summary.is_persistent());
        assert!(summary.is_singleton());
        assert_eq!(summary.icon_id, 7);

        let caps = summary.caps;
        assert!(caps.contains(Cap::RadioRx));
        assert!(caps.contains(Cap::AudioOutput));
        assert!(caps.contains(Cap::SerialComm));
        assert!(!caps.contains(Cap::DisplayDirect));

        // Test Corrupted Header CRC rejection
        pkg[16] ^= 0xFF;
        assert!(KrxPreamble::summarize(&pkg[..128]).is_err());
        pkg[16] ^= 0xFF; // Restore

        // Test Service Exec Type
        pkg[72] = KrxExecType::Service as u8;
        let header_crc_svc = crypto::crc::CrcEngine::crc32(&pkg[24..128]);
        pkg[16..20].copy_from_slice(&header_crc_svc.to_le_bytes());
        let summary_svc = KrxPreamble::summarize(&pkg[..128]).expect("Valid service summary");
        assert_eq!(summary_svc.exec_type, KrxExecType::Service);

        // Test Driver Exec Type
        pkg[72] = KrxExecType::Driver as u8;
        let header_crc_drv = crypto::crc::CrcEngine::crc32(&pkg[24..128]);
        pkg[16..20].copy_from_slice(&header_crc_drv.to_le_bytes());
        let summary_drv = KrxPreamble::summarize(&pkg[..128]).expect("Valid driver summary");
        assert_eq!(summary_drv.exec_type, KrxExecType::Driver);

        // Test Module Exec Type
        pkg[72] = KrxExecType::Module as u8;
        let header_crc_mod = crypto::crc::CrcEngine::crc32(&pkg[24..128]);
        pkg[16..20].copy_from_slice(&header_crc_mod.to_le_bytes());
        let summary_mod = KrxPreamble::summarize(&pkg[..128]).expect("Valid module summary");
        assert_eq!(summary_mod.exec_type, KrxExecType::Module);
    }

    #[test]
    fn test_section_actions_parse() {
        let mut body = [0u8; 32];
        body[0..2].copy_from_slice(&1u16.to_le_bytes());
        body[2] = KrxActionDecl::SCOPE_SHORTCUT | KrxActionDecl::SCOPE_DRAWER;
        body[4..9].copy_from_slice(b"Sched");

        body[16..18].copy_from_slice(&2u16.to_le_bytes());
        body[18] = KrxActionDecl::SCOPE_SHORTCUT;
        body[20..25].copy_from_slice(b"IpcLv");

        assert_eq!(KrxActionDecl::count(&body), 2);
        let a0 = KrxActionDecl::parse(&body, 0).unwrap();
        assert_eq!(a0.action_id, 1);
        assert_eq!(a0.label_str(), "Sched");
        let a1 = KrxActionDecl::parse(&body, 1).unwrap();
        assert_eq!(a1.action_id, 2);
        assert_eq!(a1.label_str(), "IpcLv");
        assert!(KrxActionDecl::parse(&body, 2).is_none());
    }
}
