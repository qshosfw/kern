//! RigWire v2 Naturally Aligned 8-Byte Frame Header & Channel Identifiers.

/// Wire format identifier: high nibble `0x5` (`'R'`), low nibble `0x2` (RigWire v2).
pub const WIRE_IDENT_V2: u8 = 0x52;

/// QoS bits (`flags & 0x03`): 0=Critical, 1=High, 2=Normal, 3=Bulk.
pub const FLAG_QOS_MASK: u8 = 0x03;
/// Payload encrypted in-place with Ascon-128 AEAD (implies `FLAG_AUTH`).
pub const FLAG_ENC: u8 = 1 << 2;
/// 16-byte Ascon-128 MAC tag covers `WireHeader` + payload.
pub const FLAG_AUTH: u8 = 1 << 3;
/// Bypass strict monotonic sequence check (pipelined / isochronous streams).
pub const FLAG_NO_SEQ: u8 = 1 << 4;
/// Reliable request expecting a matching `FLAG_RESP` reply.
pub const FLAG_ACK_REQ: u8 = 1 << 5;
/// Error response: payload carries `[abi::Status as u8]`.
pub const FLAG_ERR: u8 = 1 << 6;
/// Reply frame matching a request `seq`.
pub const FLAG_RESP: u8 = 1 << 7;

pub const HEADER_LEN: usize = 8;
pub const CRC_LEN: usize = 2;
pub const AUTH_TAG_LEN: usize = 16;
/// Maximum payload bytes (`8 B` transfer/stream subheader + `256 B` full KernFS flash page).
pub const MAX_PAYLOAD_LEN: usize = 264;
pub const MIN_RECORD_LEN: usize = HEADER_LEN + CRC_LEN; // 10 bytes

/// RigWire v2 Core Channels (`0x00..=0x0F` reserved, `0x10..=0xEF` dynamic nodes).
pub const CH_CORE: u8 = 0x00;
pub const CH_CONTROL: u8 = 0x01;
pub const CH_EVENTS: u8 = 0x02;
pub const CH_DATA: u8 = 0x03;

/// Naturally aligned 8-byte RigWire v2 frame envelope (zero padding, Cortex-M0+ `ldrh`-friendly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct WireHeader {
    pub ident_ver: u8,
    pub flags: u8,
    pub channel: u8,
    pub op: u8,
    pub seq: u16,
    pub len: u16,
}

const _: () = assert!(core::mem::size_of::<WireHeader>() == HEADER_LEN);

impl WireHeader {
    #[inline(always)]
    pub const fn is_response(&self) -> bool {
        (self.flags & FLAG_RESP) != 0
    }

    #[inline(always)]
    pub const fn is_error(&self) -> bool {
        (self.flags & FLAG_ERR) != 0
    }

    #[inline(always)]
    pub const fn is_no_seq(&self) -> bool {
        (self.flags & FLAG_NO_SEQ) != 0
    }

    #[inline(always)]
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < HEADER_LEN {
            return None;
        }
        Some(Self {
            ident_ver: b[0],
            flags: b[1],
            channel: b[2],
            op: b[3],
            seq: u16::from_le_bytes([b[4], b[5]]),
            len: u16::from_le_bytes([b[6], b[7]]),
        })
    }

    #[inline(always)]
    pub fn write_to(&self, dst: &mut [u8]) {
        dst[0] = self.ident_ver;
        dst[1] = self.flags;
        dst[2] = self.channel;
        dst[3] = self.op;
        let [q0, q1] = self.seq.to_le_bytes();
        let [l0, l1] = self.len.to_le_bytes();
        dst[4] = q0;
        dst[5] = q1;
        dst[6] = l0;
        dst[7] = l1;
    }
}
