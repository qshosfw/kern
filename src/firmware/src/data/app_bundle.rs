//! Application Package & Bundle Format Specification.
//!
//! Provides metadata and executable descriptor headers for installable application
//! packages on KernFS, specifying container preambles, section registries,
//! required hardware capabilities, memory budgets, and entry-point descriptors.

pub use super::file_header::{FILE_ID_APP_BUNDLE, FILE_ID_APP_LEGACY, FILE_ID_KRX};

/// Canonical package container format identifier sequence: `[0xC0, 0xDE, 0x01, 0xFE]`.
pub const APP_PACKAGE_IDENT: [u8; 4] = [0xC0, 0xDE, 0x01, 0xFE];

/// Execution class: how the payload executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AppClass {
    Native = 0,
    Paged = 1,
    VMachine = 2,
    Declare = 3,
}

impl AppClass {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
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

/// Exec type: what the package is in the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExecType {
    App = 1,
    Service = 2,
    Daemon = 3,
    Driver = 4,
    Module = 5,
}

impl ExecType {
    pub const fn from_raw(raw: u8) -> Option<Self> {
        match raw {
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
}

/// Normative 20-byte Preamble for executable application packages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct AppPreamble {
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

const _: () = assert!(core::mem::size_of::<AppPreamble>() == 24);

impl AppPreamble {
    pub const SIZE: usize = 24;
    pub const CURRENT_VERSION: u8 = 0x05;

    pub const FLAG_SEALED: u16 = 1 << 0;
    pub const FLAG_COMPRESSED: u16 = 1 << 1;
    pub const FLAG_HAS_ASSETS: u16 = 1 << 2;
    pub const FLAG_HAS_EXPORTS: u16 = 1 << 3;
    pub const FLAG_HEADLESS: u16 = 1 << 4;

    pub const fn empty() -> Self {
        Self {
            ident: APP_PACKAGE_IDENT,
            app_type: AppClass::Declare as u8,
            format_version: Self::CURRENT_VERSION,
            section_count: 0,
            header_len: Self::SIZE as u16,
            flags: 0,
            payload_crc32: 0,
            header_crc32: 0,
            link_base: 0,
        }
    }

    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        (self.ident == APP_PACKAGE_IDENT
            || u32::from_le_bytes(self.ident) == FILE_ID_APP_BUNDLE
            || u32::from_le_bytes(self.ident) == FILE_ID_APP_LEGACY)
            && self.header_len <= 256
            && (self.header_len & 3) == 0
    }

    pub fn execution_class(&self) -> AppClass {
        AppClass::from_raw(self.app_type).unwrap_or(AppClass::Declare)
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < Self::SIZE {
            return None;
        }
        let preamble = unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) };
        if preamble.is_valid() {
            Some(preamble)
        } else {
            None
        }
    }
}

/// Standard Section Table Entry (8 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct SectionEntry {
    pub section_id: u16,
    pub flags: u16,
    pub offset: u16,
    pub length: u16,
}

const _: () = assert!(core::mem::size_of::<SectionEntry>() == 8);

pub const SECTION_ID_IDENTITY: u16 = 0x0001;
pub const SECTION_ID_EXECUTION: u16 = 0x0002;
pub const SECTION_ID_CAP_IMPORTS: u16 = 0x0003;
pub const SECTION_ID_ASSETS: u16 = 0x0004;
pub const SECTION_ID_CONFIG: u16 = 0x0007;
pub const SECTION_ID_LIFECYCLE: u16 = 0x0009;

/// Identity Section (0x0001, 40 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub const fn empty() -> Self {
        Self {
            name_str: 0,
            version: [1, 0, 0],
            min_kernel_abi: 1,
            author: [0; 12],
            build_timestamp: 0,
            icon_asset: 0,
            flags: 0,
            reserved: [0; 13],
        }
    }

    pub fn set_author(&mut self, author_str: &str) {
        self.author = [0; 12];
        let bytes = author_str.as_bytes();
        let len = bytes.len().min(12);
        self.author[..len].copy_from_slice(&bytes[..len]);
    }

    pub fn author_as_str(&self) -> &str {
        let len = self
            .author
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.author.len());
        unsafe { core::str::from_utf8_unchecked(&self.author[..len]) }
    }
}

/// Execution Manifest Section (0x0002, 28 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub const fn empty() -> Self {
        Self {
            min_stack_bytes: 256,
            min_heap_bytes: 0,
            page_size: 0,
            cache_blocks: 1,
            dma_burst_size: 0,
            entry_point: 0,
            payload_size: 0,
            reloc_offset: 0,
            reloc_count: 0,
            got_offset: 0,
            rt_pin_mask: 0,
            max_fault_latency_us: 0,
            reserved: [0; 2],
        }
    }
}

/// Type & Lifecycle Section (0x0009, 12 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct SectionLifecycle {
    pub exec_type: u8,
    pub flags: u8,
    pub stack_cost: u8,
    pub restart_budget: u8,
    pub depends_count: u8,
    pub provides_count: u8,
    pub hw_claim_count: u8,
    pub restart_policy: u8,
    pub cap_mask: u32,
}

const _: () = assert!(core::mem::size_of::<SectionLifecycle>() == 12);

impl SectionLifecycle {
    pub const FLAG_STARTUP: u8 = 1 << 0;
    pub const FLAG_PERSISTENT: u8 = 1 << 1;
    pub const FLAG_RESTART_ON_FAIL: u8 = 1 << 2;
    pub const FLAG_SINGLETON: u8 = 1 << 3;

    pub const fn empty() -> Self {
        Self {
            exec_type: ExecType::App as u8,
            flags: 0,
            stack_cost: 4, // 4 x 32 = 128 bytes
            restart_budget: 0,
            depends_count: 0,
            provides_count: 0,
            hw_claim_count: 0,
            restart_policy: 0,
            cap_mask: 0,
        }
    }

    pub fn role(&self) -> ExecType {
        ExecType::from_raw(self.exec_type).unwrap_or(ExecType::App)
    }

    #[inline(always)]
    pub fn capabilities(&self) -> kernel::caps::CapabilitySet {
        kernel::caps::CapabilitySet::from_raw(self.cap_mask)
    }

    #[inline(always)]
    pub fn set_capabilities(&mut self, caps: kernel::caps::CapabilitySet) {
        self.cap_mask = caps.raw();
    }
}

/// Standard 48-byte header for `.app` executable packages (legacy & fast-scan compatible).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct AppHeader {
    pub ident: u32,
    pub version: u16,
    pub flags: u16,
    pub capabilities: u32,
    pub code_offset: u32,
    pub code_size: u32,
    pub data_size: u32,
    pub entry_point: u32,
    pub name: [u8; 16],
    pub reserved: [u8; 4],
}

const _: () = assert!(core::mem::size_of::<AppHeader>() == 48);

impl AppHeader {
    pub const SIZE: usize = 48;

    #[inline(always)]
    pub fn capabilities(&self) -> kernel::caps::CapabilitySet {
        kernel::caps::CapabilitySet::from_raw(self.capabilities)
    }

    #[inline(always)]
    pub fn set_capabilities(&mut self, caps: kernel::caps::CapabilitySet) {
        self.capabilities = caps.raw();
    }

    pub const fn empty() -> Self {
        Self {
            ident: FILE_ID_APP_BUNDLE,
            version: 0x0100,
            flags: 0,
            capabilities: 0,
            code_offset: Self::SIZE as u32,
            code_size: 0,
            data_size: 0,
            entry_point: 0,
            name: [0; 16],
            reserved: [0; 4],
        }
    }

    pub fn new(name: &str, capabilities: u32, code_size: u32) -> Self {
        let mut hdr = Self::empty();
        hdr.capabilities = capabilities;
        hdr.code_size = code_size;
        hdr.set_name(name);
        hdr
    }

    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        (self.ident == FILE_ID_APP_BUNDLE
            || self.ident == FILE_ID_APP_LEGACY
            || u32::from_le_bytes(APP_PACKAGE_IDENT) == self.ident)
            && self.code_offset >= Self::SIZE as u32
    }

    pub fn name_as_str(&self) -> &str {
        let len = self
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.name.len());
        unsafe { core::str::from_utf8_unchecked(&self.name[..len]) }
    }

    pub fn set_name(&mut self, s: &str) {
        self.name = [0; 16];
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(15);
        self.name[..copy_len].copy_from_slice(&bytes[..copy_len]);
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 48] {
        unsafe { &*(self as *const Self as *const [u8; 48]) }
    }

    #[inline(always)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 48] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 48]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 48]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}
