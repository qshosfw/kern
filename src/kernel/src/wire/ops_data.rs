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

use utils::bytes::ByteReader;

#[inline(never)]
fn read_len_ascii<'a>(r: &mut ByteReader<'a>) -> Option<&'a str> {
    let len = r.read_u8()? as usize;
    utils::format::ascii_str(r.read_bytes(len)?)
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
        let mut r = ByteReader::new(payload);
        Some(Self {
            mode: r.read_u8()?,
            total_size: r.read_u32_le()?,
            chunk_size: r.read_u16_le()?,
            path: read_len_ascii(&mut r)?,
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
        let mut r = ByteReader::new(payload);
        Some(Self {
            transfer_id: r.read_u16_le()?,
            offset: r.read_u32_le()?,
            length: r.read_u16_le()?,
            data: r.remaining_slice(),
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
        let mut r = ByteReader::new(payload);
        Some(Self {
            transfer_id: r.read_u16_le()?,
            expected_crc32: r.read_u32_le()?,
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
        let mut r = ByteReader::new(payload);
        Some(Self {
            transfer_id: r.read_u16_le()?,
            reason: r.read_u8()?,
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
            path: read_len_ascii(&mut ByteReader::new(payload))?,
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
        let mut r = ByteReader::new(payload);
        Some(Self {
            cursor: r.read_u16_le()?,
            path: read_len_ascii(&mut r)?,
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
            path: read_len_ascii(&mut ByteReader::new(payload))?,
        })
    }
}
