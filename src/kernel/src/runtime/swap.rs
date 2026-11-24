//! KRX State Hibernation Swap Header & PackBits RLE Codec.

use super::image::SWAP_IDENT;

/// Swap image flag: payload compressed using PackBits RLE byte-run algorithm
pub const SWAP_FLAG_COMPRESSED_RLE: u16 = 0x0001;

/// Fixed 16-byte power-of-two swap header for state hibernation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(4))]
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
        let mut hdr = core::mem::MaybeUninit::<Self>::uninit();
        #[cfg(target_endian = "little")]
        let hdr = unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), hdr.as_mut_ptr().cast::<u8>(), Self::SIZE);
            hdr.assume_init()
        };
        #[cfg(not(target_endian = "little"))]
        let hdr = Self {
            ident: [buf[0], buf[1], buf[2], buf[3]],
            app_id: u16::from_le_bytes([buf[4], buf[5]]),
            original_len: u16::from_le_bytes([buf[6], buf[7]]),
            compressed_len: u16::from_le_bytes([buf[8], buf[9]]),
            flags: u16::from_le_bytes([buf[10], buf[11]]),
            crc16: u16::from_le_bytes([buf[12], buf[13]]),
            reserved: [buf[14], buf[15]],
        };
        hdr.is_valid().then_some(hdr)
    }

    pub fn to_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        #[cfg(target_endian = "little")]
        unsafe {
            core::ptr::copy_nonoverlapping(
                (self as *const Self).cast::<u8>(),
                out.as_mut_ptr(),
                Self::SIZE,
            );
        }
        #[cfg(not(target_endian = "little"))]
        {
            out[0..4].copy_from_slice(&self.ident);
            out[4..6].copy_from_slice(&self.app_id.to_le_bytes());
            out[6..8].copy_from_slice(&self.original_len.to_le_bytes());
            out[8..10].copy_from_slice(&self.compressed_len.to_le_bytes());
            out[10..12].copy_from_slice(&self.flags.to_le_bytes());
            out[12..14].copy_from_slice(&self.crc16.to_le_bytes());
            out[14..16].copy_from_slice(&self.reserved);
        }
        out
    }

    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        u32::from_ne_bytes(self.ident) == u32::from_ne_bytes(SWAP_IDENT)
    }

    pub fn is_rle(&self) -> bool {
        (self.flags & SWAP_FLAG_COMPRESSED_RLE) != 0
    }
}

/// PackBits Run-Length Encoding (RLE) byte compression.
///
/// Encodes runs of 2..128 identical bytes as `[1 - count, byte]` and literal
/// sequences (broken at the next run of 3) as `[count - 1, bytes...]`.
#[inline(never)]
pub fn packbits_compress(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let mut r = 0usize;
    let mut w = 0usize;
    let slen = src.len();
    let dlen = dst.len();

    while r < slen {
        let b = src[r];
        let mut run = 1usize;
        while r + run < slen && run < 128 && src[r + run] == b {
            run += 1;
        }

        if run >= 2 {
            if w + 2 > dlen {
                return Err(());
            }
            dst[w] = (257 - run) as u8;
            dst[w + 1] = b;
            w += 2;
            r += run;
        } else {
            let start = r;
            while r < slen
                && r - start < 128
                && !(r + 2 < slen && src[r] == src[r + 1] && src[r] == src[r + 2])
            {
                r += 1;
            }
            let len = r - start;
            if w + 1 + len > dlen {
                return Err(());
            }
            dst[w] = (len - 1) as u8;
            w += 1;
            dst[w..w + len].copy_from_slice(&src[start..r]);
            w += len;
        }
    }

    Ok(w)
}

/// PackBits Run-Length Encoding (RLE) byte decompression.
#[inline(never)]
pub fn packbits_decompress(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let mut r = 0usize;
    let mut w = 0usize;
    let slen = src.len();
    let dlen = dst.len();

    while r < slen {
        let hdr = src[r];
        r += 1;

        if hdr <= 127 {
            let n = (hdr as usize) + 1;
            if r + n > slen || w + n > dlen {
                return Err(());
            }
            dst[w..w + n].copy_from_slice(&src[r..r + n]);
            r += n;
            w += n;
        } else if hdr > 128 {
            let n = 257 - (hdr as usize);
            if r >= slen || w + n > dlen {
                return Err(());
            }
            let b = src[r];
            r += 1;
            dst[w..w + n].fill(b);
            w += n;
        }
    }

    Ok(w)
}
