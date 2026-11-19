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
    Zones = 13,
    ScannerHits = 14,
    ToneSequence = 15,
    Calibration = 16,
}

impl FileType {
    /// Return the canonical natural file extension for this file type (ORDS-01).
    pub const fn extension(&self) -> &'static str {
        match self {
            FileType::GenericFile => "bin",
            FileType::Directory => "",
            FileType::Channels => "ch",
            FileType::Contacts => "ct",
            FileType::VfoSnapshot => "vfo",
            FileType::BandPlan => "bnd",
            FileType::FmBroadcast => "fm",
            FileType::Settings => "cfg",
            FileType::AppExecutable => "krx",
            FileType::Bitmap => "bmp",
            FileType::RleBitmap => "rle",
            FileType::Log => "log",
            FileType::RawData => "raw",
            FileType::Zones => "zn",
            FileType::ScannerHits => "hit",
            FileType::ToneSequence => "ton",
            FileType::Calibration => "dat",
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
        } else if name.ends_with(".zn") {
            FileType::Zones
        } else if name.ends_with(".hit") {
            FileType::ScannerHits
        } else if name.ends_with(".fm") {
            FileType::FmBroadcast
        } else if name.ends_with(".cfg") || name.ends_with(".set") || name.ends_with(".sys") {
            FileType::Settings
        } else if name.ends_with(".krx") || name.ends_with(".app") {
            FileType::AppExecutable
        } else if name.ends_with(".ton") {
            FileType::ToneSequence
        } else if name.ends_with(".dat") || name.ends_with(".sig") {
            FileType::Calibration
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
            FileType::Zones => 'Z',
            FileType::ScannerHits => 'H',
            FileType::ToneSequence => 'M',
            FileType::Calibration => 'K',
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

/// Naturally-aligned 16-byte header for fixed-stride record tables (Channels, Contacts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
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

const _: () = {
    assert!(core::mem::size_of::<TableFileHeader>() == 16);
    assert!(core::mem::align_of::<TableFileHeader>() == 4);
};

impl TableFileHeader {
    pub const SIZE: usize = 16;

    pub const fn new_table(ident: u32, record_stride: u16, max_records: u16) -> Self {
        Self {
            ident,
            version: 0x0100,
            record_count: 0,
            max_records,
            record_stride,
            active_bank: 0,
            flags: 0,
            crc16: 0,
        }
    }

    pub const fn new_channels(max_records: u16) -> Self {
        Self::new_table(TABLE_ID_CHANNELS, 64, max_records)
    }

    pub const fn new_contacts(max_records: u16) -> Self {
        Self::new_table(TABLE_ID_CONTACTS, 32, max_records)
    }

    pub const fn new_zones(max_records: u16) -> Self {
        Self::new_table(TABLE_ID_ZONE, 32, max_records)
    }

    pub const fn new_bandplans(max_records: u16) -> Self {
        Self::new_table(TABLE_ID_BANDPLAN, 32, max_records)
    }

    pub const fn new_hits(max_records: u16) -> Self {
        Self::new_table(TABLE_ID_HITS, 32, max_records)
    }

    pub const fn new_fm_stations(max_records: u16) -> Self {
        Self::new_table(TABLE_ID_FM_STATION, 16, max_records)
    }

    /// Return the power-of-two shift $N$ such that `record_stride == (1 << N)`.
    /// Returns `None` if `record_stride` is not a valid ORDS-01 power-of-two stride (`16`, `32`, `64`).
    #[inline(always)]
    pub const fn stride_shift(&self) -> Option<u32> {
        match self.record_stride {
            16 => Some(4),
            32 => Some(5),
            64 => Some(6),
            _ => None,
        }
    }

    /// Compute the exact byte offset `16 + (index << N)` in $O(1)$ without division or multiplication.
    #[inline(always)]
    pub const fn record_offset(&self, index: u16) -> Option<usize> {
        match self.stride_shift() {
            Some(shift) => Some(Self::SIZE + ((index as usize) << shift)),
            None => None,
        }
    }

    #[inline(always)]
    pub const fn is_valid_channels(&self) -> bool {
        self.ident == TABLE_ID_CHANNELS && self.record_stride == 64
    }

    #[inline(always)]
    pub const fn is_valid_contacts(&self) -> bool {
        self.ident == TABLE_ID_CONTACTS && self.record_stride == 32
    }

    #[inline(always)]
    pub const fn is_valid_zones(&self) -> bool {
        self.ident == TABLE_ID_ZONE && self.record_stride == 32
    }

    #[inline(always)]
    pub const fn is_valid_bandplans(&self) -> bool {
        self.ident == TABLE_ID_BANDPLAN && self.record_stride == 32
    }

    #[inline(always)]
    pub const fn is_valid_hits(&self) -> bool {
        self.ident == TABLE_ID_HITS && self.record_stride == 32
    }

    #[inline(always)]
    pub const fn is_valid_fm_stations(&self) -> bool {
        self.ident == TABLE_ID_FM_STATION && self.record_stride == 16
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
        Self {
            ident: u32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            version: u16::from_ne_bytes([bytes[4], bytes[5]]),
            record_count: u16::from_ne_bytes([bytes[6], bytes[7]]),
            max_records: u16::from_ne_bytes([bytes[8], bytes[9]]),
            record_stride: u16::from_ne_bytes([bytes[10], bytes[11]]),
            active_bank: bytes[12],
            flags: bytes[13],
            crc16: u16::from_ne_bytes([bytes[14], bytes[15]]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_header_roundtrip_and_strict_stride() {
        let mut ch = TableFileHeader::new_channels(200);
        ch.record_count = 42;
        ch.active_bank = 3;
        ch.flags = 0x15;
        ch.crc16 = 0xA55A;
        let decoded = TableFileHeader::from_bytes(ch.as_bytes());
        assert_eq!(decoded, ch);
        assert!(decoded.is_valid_channels());

        let bad_ch_stride = TableFileHeader {
            record_stride: 48,
            ..ch
        };
        assert!(!bad_ch_stride.is_valid_channels());

        let legacy_ch_id = TableFileHeader {
            ident: 0x48434B7F,
            ..ch
        };
        assert!(!legacy_ch_id.is_valid_channels());

        let ct = TableFileHeader::new_contacts(64);
        let decoded_ct = TableFileHeader::from_bytes(ct.as_bytes());
        assert_eq!(decoded_ct, ct);
        assert!(decoded_ct.is_valid_contacts());

        let bad_ct_stride = TableFileHeader {
            record_stride: 24,
            ..ct
        };
        assert!(!bad_ct_stride.is_valid_contacts());
    }
}
