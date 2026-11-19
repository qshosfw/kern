//! KRX State Hibernation Swap Header & PackBits RLE Codec.

use super::image::SWAP_IDENT;

/// Swap image flag: payload compressed using PackBits RLE byte-run algorithm
pub const SWAP_FLAG_COMPRESSED_RLE: u16 = 0x0001;

/// Fixed 16-byte power-of-two Swap Header for state hibernation (KRX-ABI 2.5.0 / ORDS-01)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct KrxSwapHeader {
    pub ident: [u8; 4],
    pub app_id: u16,
    pub original_len: u16,
    pub compressed_len: u16,
    pub flags: u16,
    pub crc16: u16,
    pub reserved: [u8; 2],
}

const _: () = assert!(core::mem::size_of::<KrxSwapHeader>() == 16);

impl KrxSwapHeader {
    pub const SIZE: usize = 16;

    pub const fn new(
        app_id: u16,
        original_len: u16,
        compressed_len: u16,
        flags: u16,
        crc16: u16,
    ) -> Self {
        Self {
            ident: SWAP_IDENT,
            app_id,
            original_len,
            compressed_len,
            flags,
            crc16,
            reserved: [0; 2],
        }
    }

    #[inline(never)]
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        let ident = [buf[0], buf[1], buf[2], buf[3]];
        if ident != SWAP_IDENT {
            return None;
        }
        let app_id = u16::from_le_bytes([buf[4], buf[5]]);
        let original_len = u16::from_le_bytes([buf[6], buf[7]]);
        let compressed_len = u16::from_le_bytes([buf[8], buf[9]]);
        let flags = u16::from_le_bytes([buf[10], buf[11]]);
        let crc16 = u16::from_le_bytes([buf[12], buf[13]]);
        let reserved = [buf[14], buf[15]];

        Some(Self {
            ident,
            app_id,
            original_len,
            compressed_len,
            flags,
            crc16,
            reserved,
        })
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..4].copy_from_slice(&self.ident);
        out[4..6].copy_from_slice(&self.app_id.to_le_bytes());
        out[6..8].copy_from_slice(&self.original_len.to_le_bytes());
        out[8..10].copy_from_slice(&self.compressed_len.to_le_bytes());
        out[10..12].copy_from_slice(&self.flags.to_le_bytes());
        out[12..14].copy_from_slice(&self.crc16.to_le_bytes());
        out[14..16].copy_from_slice(&self.reserved);
        out
    }

    pub fn is_valid(&self) -> bool {
        self.ident == SWAP_IDENT
    }

    pub fn is_rle(&self) -> bool {
        (self.flags & SWAP_FLAG_COMPRESSED_RLE) != 0
    }
}

/// PackBits Run-Length Encoding (RLE) byte compression.
///
/// Encodes runs of 3..128 identical bytes as `[-count + 1, byte]`.
/// Encodes non-repetitive literal sequences as `[count - 1, bytes...]`.
pub fn packbits_compress(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let mut in_idx = 0;
    let mut out_idx = 0;

    while in_idx < src.len() {
        // 1. Detect run of identical consecutive bytes
        let mut run_len = 1;
        while in_idx + run_len < src.len() && run_len < 128 && src[in_idx + run_len] == src[in_idx]
        {
            run_len += 1;
        }

        if run_len >= 3 || (run_len == 2 && in_idx + run_len == src.len()) {
            if out_idx + 2 > dst.len() {
                return Err(());
            }
            dst[out_idx] = (257 - run_len) as u8;
            dst[out_idx + 1] = src[in_idx];
            out_idx += 2;
            in_idx += run_len;
        } else {
            // 2. Collect literal bytes until a 3+ identical byte run starts
            let mut lit_len = 0;
            while in_idx + lit_len < src.len() && lit_len < 128 {
                let mut next_run = 1;
                while in_idx + lit_len + next_run < src.len()
                    && next_run < 3
                    && src[in_idx + lit_len + next_run] == src[in_idx + lit_len]
                {
                    next_run += 1;
                }
                if next_run >= 3 {
                    break;
                }
                lit_len += 1;
            }

            if lit_len > 0 {
                if out_idx + 1 + lit_len > dst.len() {
                    return Err(());
                }
                dst[out_idx] = (lit_len - 1) as u8;
                out_idx += 1;
                dst[out_idx..out_idx + lit_len].copy_from_slice(&src[in_idx..in_idx + lit_len]);
                out_idx += lit_len;
                in_idx += lit_len;
            }
        }
    }

    Ok(out_idx)
}

/// PackBits Run-Length Encoding (RLE) byte decompression.
pub fn packbits_decompress(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let mut in_idx = 0;
    let mut out_idx = 0;

    while in_idx < src.len() {
        let header = src[in_idx];
        in_idx += 1;

        if header <= 127 {
            let count = (header as usize) + 1;
            if in_idx + count > src.len() || out_idx + count > dst.len() {
                return Err(());
            }
            dst[out_idx..out_idx + count].copy_from_slice(&src[in_idx..in_idx + count]);
            in_idx += count;
            out_idx += count;
        } else if header > 128 {
            let count = 257 - (header as usize);
            if in_idx >= src.len() || out_idx + count > dst.len() {
                return Err(());
            }
            let byte = src[in_idx];
            in_idx += 1;
            for i in 0..count {
                dst[out_idx + i] = byte;
            }
            out_idx += count;
        }
        // header == 128 is treated as a no-op
    }

    Ok(out_idx)
}
