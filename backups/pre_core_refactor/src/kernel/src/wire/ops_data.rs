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
        let mode = payload[0];
        let total_size = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
        let chunk_size = u16::from_le_bytes([payload[5], payload[6]]);
        let path_len = payload[7] as usize;
        if payload.len() < 8 + path_len {
            return None;
        }
        let path = core::str::from_utf8(&payload[8..8 + path_len]).ok()?;
        Some(Self {
            mode,
            total_size,
            chunk_size,
            path,
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
        let transfer_id = u16::from_le_bytes([payload[0], payload[1]]);
        let offset = u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
        let length = u16::from_le_bytes([payload[6], payload[7]]);
        let data = if payload.len() > 8 {
            &payload[8..]
        } else {
            &[]
        };
        Some(Self {
            transfer_id,
            offset,
            length,
            data,
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
        let transfer_id = u16::from_le_bytes([payload[0], payload[1]]);
        let expected_crc32 = u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]);
        Some(Self {
            transfer_id,
            expected_crc32,
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
        let transfer_id = u16::from_le_bytes([payload[0], payload[1]]);
        let reason = payload[2];
        Some(Self {
            transfer_id,
            reason,
        })
    }
}

/// TRANSFER_STAT Request Parser
pub struct TransferStatRequest<'a> {
    pub path: &'a str,
}

impl<'a> TransferStatRequest<'a> {
    pub fn parse(payload: &'a [u8]) -> Option<Self> {
        if payload.is_empty() {
            return None;
        }
        let path_len = payload[0] as usize;
        if payload.len() < 1 + path_len {
            return None;
        }
        let path = core::str::from_utf8(&payload[1..1 + path_len]).ok()?;
        Some(Self { path })
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
        let cursor = u16::from_le_bytes([payload[0], payload[1]]);
        let path_len = payload[2] as usize;
        if payload.len() < 3 + path_len {
            return None;
        }
        let path = core::str::from_utf8(&payload[3..3 + path_len]).ok()?;
        Some(Self { cursor, path })
    }
}

/// TRANSFER_REMOVE Request Parser
pub struct TransferRemoveRequest<'a> {
    pub path: &'a str,
}

impl<'a> TransferRemoveRequest<'a> {
    pub fn parse(payload: &'a [u8]) -> Option<Self> {
        if payload.is_empty() {
            return None;
        }
        let path_len = payload[0] as usize;
        if payload.len() < 1 + path_len {
            return None;
        }
        let path = core::str::from_utf8(&payload[1..1 + path_len]).ok()?;
        Some(Self { path })
    }
}
