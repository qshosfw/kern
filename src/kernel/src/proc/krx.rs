//! KRX (KERN Realtime eXecutable) Binary Container & Header Parser
//!
//! Normative implementation of KRX-ABI 2.5.0 container format and section headers.
//! Provides zero-allocation, explicit little-endian parsing safe for ARM Cortex-M0+.

use crate::caps::CapabilitySet;

pub const KRX_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x01, 0xFE];
pub const BOOT_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x02, 0xFD];
pub const CALIB_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x03, 0xFC];
pub const SLAB_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x04, 0xFB];
pub const SWAP_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x05, 0xFA];
pub const PCB_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x06, 0xF9];
pub const KERNFS_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x07, 0xF8];
pub const SIG_MAGIC: [u8; 4] = [0xC0, 0xDE, 0x08, 0xF7];

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
        match self {
            Self::Native => "Native",
            Self::Paged => "Paged",
            Self::VMachine => "VM",
            Self::Declare => "Declare",
        }
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
        match self {
            Self::App => "App",
            Self::Service => "Service",
            Self::Daemon => "Daemon",
            Self::Driver => "Driver",
            Self::Module => "Module",
        }
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

impl KrxLaunchError {
    pub const fn title(&self) -> &'static str {
        match self {
            Self::FileNotFound => "File Not Found",
            Self::ReadFailed => "Storage Error",
            Self::InvalidPreamble => "Bad Package",
            Self::CrcMismatch => "CRC Error",
            Self::MissingIdentity => "Bad Identity",
            Self::MissingExecution => "Corrupted Binary",
            Self::CapabilityDenied(_) => "Missing Hardware",
            Self::InsufficientRam => "Out of RAM",
            Self::InsufficientStack => "Stack Overflow",
            Self::RelocationFailed => "Reloc Error",
            Self::ImportBindingFailed => "API Unresolved",
            Self::VmDisabled => "VM Disabled",
            Self::PayloadTooLarge => "Package Too Large",
            Self::ExecutionFailed => "Launch Failed",
            Self::InvalidEntryPoint => "Bad Entry Point",
            Self::TruncatedPackage => "Truncated File",
            Self::UnsupportedProfile => "Unsupported Profile",
        }
    }

    pub const fn message(&self) -> &'static str {
        match self {
            Self::FileNotFound => "Package file not found on flash",
            Self::ReadFailed => "Failed reading file from flash",
            Self::InvalidPreamble => "Invalid KRX package header",
            Self::CrcMismatch => "Package CRC verification failed",
            Self::MissingIdentity => "Missing application identity",
            Self::MissingExecution => "Missing execution manifest",
            Self::CapabilityDenied(cap) => cap,
            Self::InsufficientRam => "Not enough free RAM to execute",
            Self::InsufficientStack => "Insufficient stack headroom",
            Self::RelocationFailed => "Binary relocation failed",
            Self::ImportBindingFailed => "Required kernel API unavailable",
            Self::VmDisabled => "Virtual machine feature disabled",
            Self::PayloadTooLarge => "Binary exceeds execution arena",
            Self::ExecutionFailed => "Execution initialization failed",
            Self::InvalidEntryPoint => "Entry point not Thumb-1 compatible or out of range",
            Self::TruncatedPackage => "Binary is smaller than declared payload",
            Self::UnsupportedProfile => "Execution profile not supported by host",
        }
    }
}

/// KRX Preamble (20 bytes, little-endian, non-ASCII magic)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct KrxPreamble {
    pub magic: [u8; 4],
    pub app_type: u8,
    pub format_version: u8,
    pub section_count: u16,
    pub header_len: u16,
    pub flags: u16,
    pub payload_crc32: u32,
    pub header_crc32: u32,
    pub link_base: u32,
}

impl KrxPreamble {
    pub const SIZE: usize = 20;

    /// Parse preamble from little-endian byte slice
    pub fn parse(buf: &[u8]) -> Result<Self, ()> {
        if buf.len() < Self::SIZE {
            return Err(());
        }
        let magic = [buf[0], buf[1], buf[2], buf[3]];
        if magic != KRX_MAGIC {
            return Err(());
        }
        let app_type = buf[4];
        let format_version = buf[5];
        let section_count = u16::from_le_bytes([buf[6], buf[7]]);
        let header_len = u16::from_le_bytes([buf[8], buf[9]]);
        let flags = u16::from_le_bytes([buf[10], buf[11]]);
        let payload_crc32 = u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]);
        let header_crc32 = u32::from_le_bytes([buf[16], buf[17], buf[18], buf[19]]);
        let link_base = if buf.len() >= 24 {
            u32::from_le_bytes([buf[20], buf[21], buf[22], buf[23]])
        } else {
            0
        };

        Ok(Self {
            magic,
            app_type,
            format_version,
            section_count,
            header_len,
            flags,
            payload_crc32,
            header_crc32,
            link_base,
        })
    }

    #[inline(always)]
    pub const fn execution_class(&self) -> Option<KrxClass> {
        KrxClass::from_u8(self.app_type)
    }
}

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

/// Standard 4-byte Section Header prefix
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct SectionHeader {
    pub id: u16,
    pub length: u16, // Covers header + body, 4-aligned
}

impl SectionHeader {
    pub const SIZE: usize = 4;

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

/// Identity Section (0x0001, 40 bytes normative)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SectionIdentity {
    pub name_str: u16,
    pub version: [u8; 3],
    pub min_kernel_abi: u16,
    pub author: [u8; 12],
    pub build_timestamp: u32,
    pub icon_asset: u16,
    pub flags: u16,
    pub reserved: [u8; 13],
}

const _: () = assert!(core::mem::size_of::<SectionIdentity>() == 40);

impl SectionIdentity {
    pub const SIZE: usize = 40;

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
        if offset == 0 || offset >= header_buf.len() {
            return None;
        }
        let mut end = offset;
        while end < header_buf.len() && header_buf[end] != 0 && (end - offset) < 32 {
            end += 1;
        }
        if end > offset {
            core::str::from_utf8(&header_buf[offset..end]).ok()
        } else {
            None
        }
    }
}

/// Execution Manifest Section (0x0002, 28 bytes normative)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
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
        if buf.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            min_stack_bytes: u16::from_le_bytes([buf[0], buf[1]]),
            min_heap_bytes: u16::from_le_bytes([buf[2], buf[3]]),
            page_size: u16::from_le_bytes([buf[4], buf[5]]),
            cache_blocks: buf[6],
            dma_burst_size: buf[7],
            entry_point: u32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]),
            payload_size: u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]]),
            reloc_offset: u16::from_le_bytes([buf[16], buf[17]]),
            reloc_count: u16::from_le_bytes([buf[18], buf[19]]),
            got_offset: u16::from_le_bytes([buf[20], buf[21]]),
            rt_pin_mask: u16::from_le_bytes([buf[22], buf[23]]),
            max_fault_latency_us: u16::from_le_bytes([buf[24], buf[25]]),
            reserved: [buf[26], buf[27]],
        })
    }

    /// Check whether this section can safely execute given available system RAM and required stack safety margin.
    pub fn can_execute(&self, available_ram: usize, stack_margin: usize) -> bool {
        let total_required = (self.min_stack_bytes as usize)
            .saturating_add(self.min_heap_bytes as usize)
            .saturating_add(stack_margin);
        available_ram >= total_required
    }

    /// Explicitly requested execution profile if specified in reserved header bytes.
    #[inline(always)]
    pub const fn profile(&self) -> Option<KrxProfile> {
        KrxProfile::from_u8(self.reserved[0])
    }
}

/// CapBinding Record (6 bytes, Section 8.1 normative)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct CapBinding {
    pub interface_id: u16,
    pub min_abi: u16,
    pub flags: u16,
    pub slot_idx: u16,
}

impl CapBinding {
    pub const SIZE: usize = 6;
    pub const SIZE_WITH_SLOT: usize = 8;
    pub const FLAG_MANDATORY: u16 = 1 << 0;
    pub const FLAG_OPTIONAL: u16 = 1 << 1;
    pub const FLAG_EXPORT: u16 = 1 << 2;
    pub const FLAG_PIN_PROVIDER: u16 = 1 << 3;

    /// Parse a `CapBinding` record from little-endian bytes (6-byte or 8-byte record).
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
#[repr(C, packed)]
pub struct HwClaim {
    pub kind: u8, // 0: PIN, 1: IRQ, 2: DMA_CH, 3: BUS
    pub id: u8,
    pub flags: u8,
    pub reserved: u8,
}

/// Type & Lifecycle Section (0x0009, 12 bytes base header)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
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

/// Paged Block Directory Entry (0x0005, 8 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct BlockEntry {
    pub block_id: u8,
    pub flags: u8,
    pub size: u16,
    pub flash_offset: u32,
}

// ── P0 Structured Paged Leaf Overlays & Continuations (§5.1 / KRX 3.2) ──────

pub const FLAG_LEAF_PINNED: u8 = 1 << 0;
pub const FLAG_LEAF_RT: u8 = 1 << 1;
pub const FLAG_LEAF_PREFETCH: u8 = 1 << 2;

/// P0 Structured Paged Leaf Overlay Descriptor (0x000A / ORDS-01 16-byte power-of-two geometry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
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
#[repr(C, packed)]
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
#[repr(C, packed)]
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
#[repr(C, packed)]
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
#[repr(C, packed)]
pub struct DeclareNode {
    pub node_id: u16,
    pub layout: u8,
    pub item_count: u8,
    pub title_offset: u16,
    pub first_item_offset: u16,
}

/// Declare UI Item Record (12 bytes)
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
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
#[repr(C, packed)]
pub struct KrxIndexEntry {
    pub name: [u8; 16],
    pub class: u8,
    pub exec_type: u8,
    pub icon_id: u16,
    pub caps: u16,
    pub file_size: u32,
    pub crc16: u16,
    pub flags: u16,
    pub reserved: [u8; 4],
}

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
            reserved: [0; 4],
        }
    }

    pub fn name_str(&self) -> &str {
        let len = self.name.iter().position(|&b| b == 0).unwrap_or(16);
        unsafe { core::str::from_utf8_unchecked(&self.name[..len]) }
    }

    pub fn capability_set(&self) -> CapabilitySet {
        CapabilitySet::from_raw(self.caps as u32)
    }
}

/// Swap image flag: payload compressed using PackBits RLE byte-run algorithm
pub const SWAP_FLAG_COMPRESSED_RLE: u16 = 0x0001;

/// Fixed 16-byte power-of-two Swap Header for state hibernation (KRX-ABI 2.5.0 / ORDS-01)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct KrxSwapHeader {
    pub magic: [u8; 4],
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
            magic: SWAP_MAGIC,
            app_id,
            original_len,
            compressed_len,
            flags,
            crc16,
            reserved: [0; 2],
        }
    }

    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let magic = [buf[0], buf[1], buf[2], buf[3]];
        if magic != SWAP_MAGIC {
            return None;
        }
        let app_id = u16::from_le_bytes([buf[4], buf[5]]);
        let original_len = u16::from_le_bytes([buf[6], buf[7]]);
        let compressed_len = u16::from_le_bytes([buf[8], buf[9]]);
        let flags = u16::from_le_bytes([buf[10], buf[11]]);
        let crc16 = u16::from_le_bytes([buf[12], buf[13]]);
        let reserved = [buf[14], buf[15]];

        Some(Self {
            magic,
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
        out[0..4].copy_from_slice(&self.magic);
        out[4..6].copy_from_slice(&self.app_id.to_le_bytes());
        out[6..8].copy_from_slice(&self.original_len.to_le_bytes());
        out[8..10].copy_from_slice(&self.compressed_len.to_le_bytes());
        out[10..12].copy_from_slice(&self.flags.to_le_bytes());
        out[12..14].copy_from_slice(&self.crc16.to_le_bytes());
        out[14..16].copy_from_slice(&self.reserved);
        out
    }

    pub fn is_valid(&self) -> bool {
        self.magic == SWAP_MAGIC
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
        let magic = parsed.magic;
        let app_id = parsed.app_id;
        let orig_len = parsed.original_len;
        let comp_len = parsed.compressed_len;
        let flags = parsed.flags;
        let crc = parsed.crc16;

        assert_eq!(magic, SWAP_MAGIC);
        assert_eq!(app_id, 2);
        assert_eq!(orig_len, 64);
        assert_eq!(comp_len, 28);
        assert_eq!(flags, SWAP_FLAG_COMPRESSED_RLE);
        assert_eq!(crc, 0xABCD);
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
        assert_eq!(err_cap.message(), "Requires Broadband HF RX");

        let err_ram = KrxLaunchError::InsufficientRam;
        assert_eq!(err_ram.title(), "Out of RAM");
        assert_eq!(err_ram.message(), "Not enough free RAM to execute");

        let err_stack = KrxLaunchError::InsufficientStack;
        assert_eq!(err_stack.title(), "Stack Overflow");
        assert_eq!(err_stack.message(), "Insufficient stack headroom");

        let err_vm = KrxLaunchError::VmDisabled;
        assert_eq!(err_vm.title(), "VM Disabled");
        assert_eq!(err_vm.message(), "Virtual machine feature disabled");

        let err_fnf = KrxLaunchError::FileNotFound;
        assert_eq!(err_fnf.title(), "File Not Found");
        assert_eq!(err_fnf.message(), "Package file not found on flash");

        let err_reloc = KrxLaunchError::RelocationFailed;
        assert_eq!(err_reloc.title(), "Reloc Error");
        assert_eq!(err_reloc.message(), "Binary relocation failed");

        let err_api = KrxLaunchError::ImportBindingFailed;
        assert_eq!(err_api.title(), "API Unresolved");
        assert_eq!(err_api.message(), "Required kernel API unavailable");

        let err_corrupt = KrxLaunchError::MissingExecution;
        assert_eq!(err_corrupt.title(), "Corrupted Binary");
        assert_eq!(err_corrupt.message(), "Missing execution manifest");
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
}
