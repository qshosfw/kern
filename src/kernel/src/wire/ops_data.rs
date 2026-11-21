//! RigWire Data Domain (0x03) Standard Operations.
//!
//! Implements candidate RigWire Block, File, and Stream transfer specifications:
//! - TRANSFER_OPEN (0x01): Negotiate chunk size, mode, and target path.
//! - TRANSFER_CHUNK (0x02): Stream chunked blocks with offset tracking.
//! - TRANSFER_CLOSE (0x03): Commit transfer and verify CRC-32 checksum.
//! - TRANSFER_ABORT (0x04): Cancel active transfer and release scratch resources.
//! - TRANSFER_STAT (0x05): Query file and directory metadata.
//! - TRANSFER_LIST (0x06): Directory pagination.
//! - TRANSFER_REMOVE (0x07): Unlink file.
//! - PACKET_SEND (0x10) / PACKET_RECEIVE (0x90): Datagram packets.

pub const OP_TRANSFER_OPEN: u8 = 0x01;
pub const OP_TRANSFER_CHUNK: u8 = 0x02;
pub const OP_TRANSFER_CLOSE: u8 = 0x03;
pub const OP_TRANSFER_ABORT: u8 = 0x04;
pub const OP_TRANSFER_STAT: u8 = 0x05;
pub const OP_TRANSFER_LIST: u8 = 0x06;
pub const OP_TRANSFER_REMOVE: u8 = 0x07;
pub const OP_PACKET_SEND: u8 = 0x10;
pub const OP_PACKET_RECEIVE: u8 = 0x90;

pub const TRANSFER_MODE_READ: u8 = 0;
pub const TRANSFER_MODE_WRITE: u8 = 1;

#[inline(never)]
fn read_len_ascii(s: &[u8]) -> Option<&str> {
    let (&len, rest) = s.split_first()?;
    utils::format::ascii_str(rest.get(..len as usize)?)
}

#[inline(always)]
fn le16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

#[inline(always)]
fn le32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

/// TRANSFER_OPEN Request Parser
pub struct TransferOpenRequest<'a> {
    pub mode: u8,
    pub total_size: u32,
    pub chunk_size: u16,
    pub path: &'a str,
}

impl<'a> TransferOpenRequest<'a> {
    pub fn parse(payload: &'a [u8]) -> Option<Self> {
        if payload.len() < 8 {
            return None;
        }
        Some(Self {
            mode: payload[0],
            total_size: le32(payload, 1),
            chunk_size: le16(payload, 5),
            path: read_len_ascii(&payload[7..])?,
        })
    }
}

/// TRANSFER_CHUNK Request Parser
pub struct TransferChunkRequest<'a> {
    pub transfer_id: u16,
    pub offset: u32,
    pub length: u16,
    pub data: &'a [u8],
}

impl<'a> TransferChunkRequest<'a> {
    pub fn parse(payload: &'a [u8]) -> Option<Self> {
        if payload.len() < 8 {
            return None;
        }
        Some(Self {
            transfer_id: le16(payload, 0),
            offset: le32(payload, 2),
            length: le16(payload, 6),
            data: &payload[8..],
        })
    }
}

/// TRANSFER_CLOSE Request Parser
pub struct TransferCloseRequest {
    pub transfer_id: u16,
    pub expected_crc32: u32,
}

impl TransferCloseRequest {
    pub fn parse(payload: &[u8]) -> Option<Self> {
        if payload.len() < 6 {
            return None;
        }
        Some(Self {
            transfer_id: le16(payload, 0),
            expected_crc32: le32(payload, 2),
        })
    }
}

/// TRANSFER_ABORT Request Parser
pub struct TransferAbortRequest {
    pub transfer_id: u16,
    pub reason: u8,
}

impl TransferAbortRequest {
    pub fn parse(payload: &[u8]) -> Option<Self> {
        if payload.len() < 3 {
            return None;
        }
        Some(Self {
            transfer_id: le16(payload, 0),
            reason: payload[2],
        })
    }
}

/// TRANSFER_STAT Request Parser
pub struct TransferStatRequest<'a> {
    pub path: &'a str,
}

impl<'a> TransferStatRequest<'a> {
    pub fn parse(payload: &'a [u8]) -> Option<Self> {
        Some(Self {
            path: read_len_ascii(payload)?,
        })
    }
}

/// TRANSFER_LIST Request Parser
pub struct TransferListRequest<'a> {
    pub cursor: u16,
    pub path: &'a str,
}

impl<'a> TransferListRequest<'a> {
    pub fn parse(payload: &'a [u8]) -> Option<Self> {
        if payload.len() < 3 {
            return None;
        }
        Some(Self {
            cursor: le16(payload, 0),
            path: read_len_ascii(&payload[2..])?,
        })
    }
}

/// TRANSFER_REMOVE Request Parser
pub struct TransferRemoveRequest<'a> {
    pub path: &'a str,
}

impl<'a> TransferRemoveRequest<'a> {
    pub fn parse(payload: &'a [u8]) -> Option<Self> {
        Some(Self {
            path: read_len_ascii(payload)?,
        })
    }
}
