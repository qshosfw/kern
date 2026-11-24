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
