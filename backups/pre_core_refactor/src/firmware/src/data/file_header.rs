//! Unified File and Database Header Types.
//!
//! Provides zero-allocation file type identification, natural extension mapping,
//! and standard fixed-stride table database headers for Channels, Contacts, and App packages.

/// System-wide file type taxonomy with natural file extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FileType {
    GenericFile = 0,
    Directory = 1,
    Channels = 2,
    Contacts = 3,
    VfoSnapshot = 4,
    BandPlan = 5,
    FmBroadcast = 6,
    Settings = 7,
    AppExecutable = 8,
    Bitmap = 9,
    RleBitmap = 10,
    Log = 11,
    RawData = 12,
}

impl FileType {
    /// Return the canonical natural file extension for this file type.
    pub const fn extension(&self) -> &'static str {
        match self {
            FileType::GenericFile => "bin",
            FileType::Directory => "",
            FileType::Channels => "ch",
            FileType::Contacts => "ct",
            FileType::VfoSnapshot => "vfo",
            FileType::BandPlan => "bnd",
            FileType::FmBroadcast => "fm",
            FileType::Settings => "set",
            FileType::AppExecutable => "app",
            FileType::Bitmap => "bmp",
            FileType::RleBitmap => "rle",
            FileType::Log => "log",
            FileType::RawData => "raw",
        }
    }

    /// Classify a file type based on its filename or path.
    pub fn from_filename(name: &str) -> Self {
        if name.ends_with(".ch") {
            FileType::Channels
        } else if name.ends_with(".ct") {
            FileType::Contacts
        } else if name.ends_with(".vfo") {
            FileType::VfoSnapshot
        } else if name.ends_with(".bnd") {
            FileType::BandPlan
        } else if name.ends_with(".fm") {
            FileType::FmBroadcast
        } else if name.ends_with(".set") || name.ends_with(".sys") {
            FileType::Settings
        } else if name.ends_with(".app") || name.ends_with(".krx") {
            FileType::AppExecutable
        } else if name.ends_with(".bmp") {
            FileType::Bitmap
        } else if name.ends_with(".rle") {
            FileType::RleBitmap
        } else if name.ends_with(".log") {
            FileType::Log
        } else if name.ends_with(".raw") {
            FileType::RawData
        } else {
            FileType::GenericFile
        }
    }

    /// Single-character visual glyph / indicator for compact file managers.
    pub const fn glyph_char(&self) -> char {
        match self {
            FileType::GenericFile => 'F',
            FileType::Directory => 'D',
            FileType::Channels => 'C',
            FileType::Contacts => 'T',
            FileType::VfoSnapshot => 'V',
            FileType::BandPlan => 'B',
            FileType::FmBroadcast => 'R',
            FileType::Settings => 'S',
            FileType::AppExecutable => 'A',
            FileType::Bitmap | FileType::RleBitmap => 'I',
            FileType::Log => 'L',
            FileType::RawData => 'W',
        }
    }
}

/// Canonical unbranded FourCC format identifiers for native filesystem tables.
pub const TABLE_ID_CHANNELS: u32 = u32::from_be_bytes(*b"CHAN"); // "CHAN"
pub const TABLE_ID_CONTACTS: u32 = u32::from_be_bytes(*b"CONT"); // "CONT"
pub const TABLE_ID_BANDPLAN: u32 = u32::from_be_bytes(*b"BNDP"); // "BNDP"
pub const TABLE_ID_HITS: u32 = u32::from_be_bytes(*b"HITS"); // "HITS"
pub const TABLE_ID_ZONE: u32 = u32::from_be_bytes(*b"ZONE"); // "ZONE"
pub const TABLE_ID_FM_STATION: u32 = u32::from_be_bytes(*b"FMBC"); // "FMBC"
pub const TABLE_ID_CAPABILITIES: u32 = u32::from_be_bytes(*b"RCAP"); // "RCAP"
pub const FILE_ID_VFO: u32 = u32::from_be_bytes(*b"VFO "); // "VFO "
pub const FILE_ID_APP_BUNDLE: u32 = 0x58524B7F; // "\x7FKRX" / "\x7FAPP" binary bundle format identifier
pub const FILE_ID_KRX: u32 = FILE_ID_APP_BUNDLE;
pub const FILE_ID_APP_LEGACY: u32 = 0x7F505041; // "\x7FAPP"
pub const FILE_ID_BMP: u16 = 0x4D42; // "BM"
pub const FILE_ID_RLE_BITMAP: u32 = 0x4D424C52; // "RLBM"

/// Packed 16-byte header for fixed-stride record tables (Channels, Contacts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, packed)]
pub struct TableFileHeader {
    pub ident: u32,
    pub version: u16,
    pub record_count: u16,
    pub max_records: u16,
    pub record_stride: u16,
    pub active_bank: u8,
    pub flags: u8,
    pub crc16: u16,
}

impl TableFileHeader {
    pub const SIZE: usize = 16;

    pub const fn new_channels(max_records: u16) -> Self {
        Self {
            ident: TABLE_ID_CHANNELS,
            version: 0x0100,
            record_count: 0,
            max_records,
            record_stride: 64, // 64 bytes power-of-two aligned
            active_bank: 0,
            flags: 0,
            crc16: 0,
        }
    }

    pub const fn new_contacts(max_records: u16) -> Self {
        Self {
            ident: TABLE_ID_CONTACTS,
            version: 0x0100,
            record_count: 0,
            max_records,
            record_stride: 32, // 32 bytes power-of-two aligned
            active_bank: 0,
            flags: 0,
            crc16: 0,
        }
    }

    #[inline(always)]
    pub fn is_valid_channels(&self) -> bool {
        (self.ident == TABLE_ID_CHANNELS || self.ident == 0x48434B7F)
            && (self.record_stride == 64 || self.record_stride == 48)
    }

    #[inline(always)]
    pub fn is_valid_contacts(&self) -> bool {
        (self.ident == TABLE_ID_CONTACTS || self.ident == 0x54434B7F)
            && (self.record_stride == 32 || self.record_stride == 24)
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 16] {
        unsafe { &*(self as *const Self as *const [u8; 16]) }
    }

    #[inline(always)]
    pub fn as_bytes_mut(&mut self) -> &mut [u8; 16] {
        unsafe { &mut *(self as *mut Self as *mut [u8; 16]) }
    }

    #[inline(always)]
    pub fn from_bytes(bytes: &[u8; 16]) -> Self {
        unsafe { core::ptr::read_unaligned(bytes.as_ptr() as *const Self) }
    }
}
