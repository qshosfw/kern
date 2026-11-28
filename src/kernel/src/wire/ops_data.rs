//! RigWire v2 Data & Storage Channel (`CH_DATA = 0x03`) Opcodes.
//!
//! Covers streaming file/partition transfers and Merkle sector-hash delta sync.

pub const OP_TRANSFER_OPEN: u8 = 0x01;
pub const OP_TRANSFER_CHUNK: u8 = 0x02;
pub const OP_TRANSFER_CLOSE: u8 = 0x03;
pub const OP_TRANSFER_ABORT: u8 = 0x04;
pub const OP_TRANSFER_STAT: u8 = 0x05;
pub const OP_TRANSFER_LIST: u8 = 0x06;
pub const OP_TRANSFER_REMOVE: u8 = 0x07;
/// Compute FNV-1a 32-bit hashes per 256-byte page/sector slice for O(delta) table sync.
pub const OP_TRANSFER_SECTOR_HASH: u8 = 0x08;

pub const TRANSFER_MODE_READ: u8 = 0;
pub const TRANSFER_MODE_WRITE: u8 = 1;

/// Single-indirect KernFS v2.1 page limit: 12 L1 index pages × 128 pages × 256 B = 384 KiB.
pub const MAX_WIRE_FILE_SIZE: usize = 12 * 128 * 256;

/// Active transfer session state for streaming KernFS reads/writes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ActiveTransfer {
    pub active: bool,
    pub mode: u8,
    pub transfer_id: u16,
    pub chunk_size: u16,
    pub ino: u16,
    pub total_size: u32,
    pub current_len: u32,
    pub last_activity_ms: u32,
    pub path: utils::format::InlineStr<31>,
}

impl ActiveTransfer {
    pub const fn empty() -> Self {
        Self {
            active: false,
            mode: TRANSFER_MODE_READ,
            transfer_id: 0,
            chunk_size: 0,
            ino: 0,
            total_size: 0,
            current_len: 0,
            last_activity_ms: 0,
            path: utils::format::InlineStr::new(),
        }
    }

    #[inline(always)]
    pub fn path_str(&self) -> &str {
        self.path.as_str()
    }
}

/// Event streaming subscription state.
#[derive(Clone, Copy)]
pub struct EventSubscription {
    pub active_mask: u32,
    pub credits: u16,
}

impl EventSubscription {
    pub const fn empty() -> Self {
        Self {
            active_mask: 0,
            credits: 0,
        }
    }
}

#[inline(always)]
pub fn trim_trailing_slash(path: &str) -> &str {
    let b = path.as_bytes();
    if b.len() > 1 && b[b.len() - 1] == b'/' {
        &path[..b.len() - 1]
    } else {
        path
    }
}
