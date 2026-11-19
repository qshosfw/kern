//! Delta Encoding & Frame Compression Firmware Framework.
//!
//! Provides zero-allocation display frame differencing (delta encoding) and RLE
//! compression interfaces for remote screen-sharing, mirror streaming, and screenshot archiving.

use gfx::{Canvas, LCD_WIDTH};

/// Header for a frame difference packet in a screen-sharing stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct FrameDeltaHeader {
    pub seq: u16,
    pub modified_pages: u8,
    pub flags: u8,
    pub payload_len: u16,
}

const _: () = {
    assert!(core::mem::size_of::<FrameDeltaHeader>() == 6);
    assert!(core::mem::align_of::<FrameDeltaHeader>() == 2);
};

impl FrameDeltaHeader {
    pub const SIZE: usize = 6;
    pub const FLAG_KEYFRAME: u8 = 1 << 0;
    pub const FLAG_RLE_COMPRESSED: u8 = 1 << 1;
}

/// Abstract display frame differencing encoder for screen-sharing subsystems.
pub trait FrameDeltaEncoder {
    /// Compute frame difference between `prev` and `curr` frames.
    ///
    /// Writes difference payload to `out` buffer and returns number of bytes written.
    fn encode_delta(&mut self, prev: &Canvas, curr: &Canvas, out: &mut [u8]) -> usize;
}

/// Abstract display frame decoder for receiving remote display streams.
pub trait FrameDeltaDecoder {
    /// Apply a frame difference payload to `canvas`.
    fn apply_delta(&mut self, canvas: &mut Canvas, payload: &[u8]) -> Result<(), ()>;
}

/// Standard page-level display delta encoder (8 vertical pages of 128 bytes each).
pub struct PageDeltaEngine {
    seq: u16,
}

impl PageDeltaEngine {
    pub const fn new() -> Self {
        Self { seq: 0 }
    }

    /// Reset frame sequence counter.
    pub fn reset(&mut self) {
        self.seq = 0;
    }

    /// Check which of the 8 pages differ between `prev` and `curr`.
    pub fn diff_pages(prev: &Canvas, curr: &Canvas) -> u8 {
        let mut mask = 0u8;
        for p in 0..8 {
            if prev.buffer[p] != curr.buffer[p] {
                mask |= 1 << p;
            }
        }
        mask
    }

    /// Encode page difference into a byte stream:
    /// `[seq: u16][mask: u8][flags: u8][len: u16][pages...]`
    pub fn encode_frame(&mut self, prev: &Canvas, curr: &Canvas, out: &mut [u8]) -> usize {
        let mask = Self::diff_pages(prev, curr);
        let changed_count = mask.count_ones() as usize;
        let required_len = FrameDeltaHeader::SIZE + (changed_count * LCD_WIDTH);

        if out.len() < required_len {
            return 0;
        }

        let seq_bytes = self.seq.to_ne_bytes();
        let len_bytes = ((changed_count * LCD_WIDTH) as u16).to_ne_bytes();
        self.seq = self.seq.wrapping_add(1);

        // Write [seq: u16][modified_pages: u8][flags: u8][payload_len: u16]
        out[0..2].copy_from_slice(&seq_bytes);
        out[2] = mask;
        out[3] = 0;
        out[4..6].copy_from_slice(&len_bytes);

        let mut offset = FrameDeltaHeader::SIZE;
        for p in 0..8 {
            if (mask & (1 << p)) != 0 {
                out[offset..offset + LCD_WIDTH].copy_from_slice(&curr.buffer[p]);
                offset += LCD_WIDTH;
            }
        }

        offset
    }

    /// Apply a decoded delta packet to `canvas`.
    pub fn apply_frame(canvas: &mut Canvas, packet: &[u8]) -> Result<(), ()> {
        if packet.len() < FrameDeltaHeader::SIZE {
            return Err(());
        }

        let modified_pages = packet[2];

        let mut offset = FrameDeltaHeader::SIZE;
        for p in 0..8 {
            if (modified_pages & (1 << p)) != 0 {
                if offset + LCD_WIDTH > packet.len() {
                    return Err(());
                }
                canvas.buffer[p].copy_from_slice(&packet[offset..offset + LCD_WIDTH]);
                canvas.dirty |= 1 << p;
                offset += LCD_WIDTH;
            }
        }

        Ok(())
    }
}

/// Standard PackBits Run-Length Encoding (RLE) engine for 1-bit monochrome graphics.
///
/// Implements standard Apple PackBits compression used across boot splashes,
/// screenshot archives, and remote display frame transmissions:
/// - Header byte `b >= 0` (0..127): `b + 1` literal uncompressed bytes follow.
/// - Header byte `b < 0 && b != -128` (-1..-127): repeat the single following byte `1 - b` times (2..128 repeats).
/// - Header byte `-128` (0x80): Reserved no-op / padding byte.
pub struct RleCompressor;

impl RleCompressor {
    /// Encode `src` slice into `dst` using standard PackBits run-length encoding.
    pub fn encode(src: &[u8], dst: &mut [u8]) -> usize {
        let mut r_idx = 0;
        let mut w_idx = 0;

        while r_idx < src.len() {
            let b = src[r_idx];
            let mut run_len = 1;
            while r_idx + run_len < src.len() && src[r_idx + run_len] == b && run_len < 128 {
                run_len += 1;
            }

            if run_len >= 2 {
                // Emit PackBits repeat: byte count stored as `1 - count` (i.e. -1 for count=2, -127 for count=128)
                if w_idx + 2 > dst.len() {
                    return 0; // Output buffer overflow
                }
                let header = (1 - (run_len as i16)) as i8 as u8;
                dst[w_idx] = header;
                dst[w_idx + 1] = b;
                w_idx += 2;
                r_idx += run_len;
            } else {
                // Collect literals
                let lit_start = r_idx;
                let mut lit_len = 0;
                while r_idx < src.len() && lit_len < 128 {
                    if r_idx + 2 < src.len()
                        && src[r_idx] == src[r_idx + 1]
                        && src[r_idx] == src[r_idx + 2]
                    {
                        break; // Stop literal on run of 3 identical bytes
                    }
                    lit_len += 1;
                    r_idx += 1;
                }

                if w_idx + 1 + lit_len > dst.len() {
                    return 0; // Buffer overflow
                }
                dst[w_idx] = (lit_len - 1) as u8;
                w_idx += 1;
                dst[w_idx..w_idx + lit_len].copy_from_slice(&src[lit_start..lit_start + lit_len]);
                w_idx += lit_len;
            }
        }

        w_idx
    }

    /// Decode PackBits RLE byte stream from `src` into `dst`.
    pub fn decode(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
        let mut r_idx = 0;
        let mut w_idx = 0;

        while r_idx < src.len() {
            let b = src[r_idx] as i8;
            r_idx += 1;

            if b >= 0 {
                // Literal copy: b + 1 bytes
                let count = (b as usize) + 1;
                if r_idx + count > src.len() || w_idx + count > dst.len() {
                    return Err(());
                }
                dst[w_idx..w_idx + count].copy_from_slice(&src[r_idx..r_idx + count]);
                r_idx += count;
                w_idx += count;
            } else if b != -128 {
                // Repeat run: 1 - b times
                let count = (1 - (b as i16)) as usize;
                if r_idx >= src.len() || w_idx + count > dst.len() {
                    return Err(());
                }
                let val = src[r_idx];
                r_idx += 1;
                for _ in 0..count {
                    dst[w_idx] = val;
                    w_idx += 1;
                }
            }
        }

        Ok(w_idx)
    }

    /// Decompress a PackBits RLE stream directly into the 1024-byte Canvas VRAM buffer.
    pub fn decompress_to_canvas(canvas: &mut Canvas, rle: &[u8]) -> bool {
        canvas.clear();
        let mut idx = 0usize;
        let mut i = 0usize;

        while i < rle.len() && idx < 1024 {
            let b = rle[i] as i8;
            i += 1;
            if b >= 0 {
                let count = (b as usize) + 1;
                let n = count.min(rle.len() - i).min(1024 - idx);
                for j in 0..n {
                    let page = (idx + j) >> 7; // 128 bytes per page
                    let col = (idx + j) & 0x7F;
                    canvas.buffer[page][col] = rle[i + j];
                }
                idx += n;
                i += count;
            } else if b != -128 {
                let count = (1 - (b as i16)) as usize;
                if i < rle.len() {
                    let val = rle[i];
                    i += 1;
                    let n = count.min(1024 - idx);
                    for j in 0..n {
                        let page = (idx + j) >> 7;
                        let col = (idx + j) & 0x7F;
                        canvas.buffer[page][col] = val;
                    }
                    idx += n;
                }
            }
        }

        canvas.mark_all_dirty();
        idx >= 1024
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_delta_encode_apply_roundtrip() {
        let prev = Canvas::new();
        let mut curr = Canvas::new();
        curr.buffer[1][7] = 0xAA;
        curr.buffer[6][120] = 0x55;

        let mut engine = PageDeltaEngine::new();
        let mut pkt = [0u8; FrameDeltaHeader::SIZE + 2 * LCD_WIDTH];
        let written = engine.encode_frame(&prev, &curr, &mut pkt);
        assert_eq!(written, FrameDeltaHeader::SIZE + 2 * LCD_WIDTH);
        assert_eq!(pkt[2], (1 << 1) | (1 << 6));

        let mut dst = Canvas::new();
        PageDeltaEngine::apply_frame(&mut dst, &pkt[..written]).unwrap();
        assert_eq!(dst.buffer[1], curr.buffer[1]);
        assert_eq!(dst.buffer[6], curr.buffer[6]);
        assert_eq!(dst.dirty, (1 << 1) | (1 << 6));
    }
}
