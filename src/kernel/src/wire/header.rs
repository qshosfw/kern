//! RigWire Frame Header, Status Codes, and Domain Identifiers.

pub const WIRE_SYNC_ID: u8 = 0x52; // 'R'
pub const WIRE_MAJOR: u8 = 0x01;

pub const FLAG_RESPONSE: u8 = 0x01;
pub const FLAG_EVENT: u8 = 0x02;

pub const HEADER_LEN: usize = 15;
pub const CRC_LEN: usize = 2;
pub const MIN_RECORD_LEN: usize = HEADER_LEN + CRC_LEN; // 17 bytes

/// RigWire Service Domain IDs (§8.5)
pub const SERVICE_CORE: u8 = 0x00;
pub const SERVICE_CONTROL: u8 = 0x01;
pub const SERVICE_EVENTS: u8 = 0x02;
pub const SERVICE_DATA: u8 = 0x03;

/// Wire Status Codes (§6)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireStatus {
    Ok = 0,
    Accepted = 1,
    BadLength = 2,
    BadRange = 3,
    BadVersion = 4,
    BadFlags = 5,
    BadSession = 6,
    StaleSequence = 7,
    DuplicateConflict = 8,
    Unsupported = 9,
    Busy = 10,
    StaleRevision = 11,
    StaleComponent = 12,
    SafetyDenied = 13,
    AuthRequired = 14,
    AuthzDenied = 15,
    NotPersistable = 16,
    UnsupportedAtomicity = 17,
    Failed = 18,
    InProgress = 19,
    StaleCatalogue = 20,
    UnsupportedField = 21,
    NotFound = 22,
    IoError = 23,
    Timeout = 27,
    Internal = 28,
}

impl WireStatus {
    pub const fn to_u8(self) -> u8 {
        self as u8
    }

    pub const fn from_u8(val: u8) -> Self {
        match val {
            0 => Self::Ok,
            1 => Self::Accepted,
            2 => Self::BadLength,
            3 => Self::BadRange,
            4 => Self::BadVersion,
            5 => Self::BadFlags,
            6 => Self::BadSession,
            7 => Self::StaleSequence,
            8 => Self::DuplicateConflict,
            9 => Self::Unsupported,
            10 => Self::Busy,
            11 => Self::StaleRevision,
            12 => Self::StaleComponent,
            13 => Self::SafetyDenied,
            14 => Self::AuthRequired,
            15 => Self::AuthzDenied,
            16 => Self::NotPersistable,
            17 => Self::UnsupportedAtomicity,
            18 => Self::Failed,
            19 => Self::InProgress,
            20 => Self::StaleCatalogue,
            21 => Self::UnsupportedField,
            22 => Self::NotFound,
            23 => Self::IoError,
            27 => Self::Timeout,
            _ => Self::Internal,
        }
    }
}

/// Parsed Wire Record Header (16 bytes, power-of-two geometry, naturally aligned)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct WireHeader {
    pub session_id: u32,
    pub sequence: u32,
    pub payload_len: u16,
    pub ident: u8,
    pub major: u8,
    pub flags: u8,
    pub service: u8,
    pub opcode: u8,
}

const _: () = assert!(core::mem::size_of::<WireHeader>() == 16);

impl WireHeader {
    #[inline(always)]
    pub fn is_response(&self) -> bool {
        (self.flags & FLAG_RESPONSE) != 0
    }

    #[inline(always)]
    pub fn is_event(&self) -> bool {
        (self.flags & FLAG_EVENT) != 0
    }
}
