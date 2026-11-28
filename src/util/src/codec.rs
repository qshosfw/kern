//! Unified zero-heap `#![no_std]` compression codecs (`PackBits`, in-place `LZSS`, and stackless `BPE`).
//!
//! Consolidates run-length, dictionary, and byte-pair decoders across boot splash assets,
//! hibernation swap blocks, `P1 v2` compressed code pages, and `LNG1` localization string pools.

/// PackBits Run-Length Encoding (RLE) byte compression (`2..=128` byte runs).
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

/// Strict PackBits RLE decompression; returns `Err(())` if `src` is truncated or overflows `dst`.
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

/// Truncation-tolerant PackBits decoder for partial/clamped framebuffers (e.g. `1024 B` VRAM splash).
pub fn packbits_decompress_clamped(src: &[u8], dst: &mut [u8]) -> usize {
    let mut r = 0usize;
    let mut w = 0usize;
    while r < src.len() && w < dst.len() {
        let hdr = src[r];
        r += 1;
        if hdr <= 127 {
            let n = ((hdr as usize) + 1).min(src.len() - r).min(dst.len() - w);
            dst[w..w + n].copy_from_slice(&src[r..r + n]);
            r += n;
            w += n;
        } else if hdr > 128 && r < src.len() {
            let n = (257 - (hdr as usize)).min(dst.len() - w);
            let b = src[r];
            r += 1;
            dst[w..w + n].fill(b);
            w += n;
        }
    }
    w
}

/// Decompress a byte-aligned `LZSS` stream (`flag` byte LSB-first: `0` = literal,
/// `1` = 2-byte match `[off_lo, ((off_hi & 0x0F) << 4) | (len - 3)]`, distance `1..=4096`, length `3..=18`).
pub fn lzss_decompress(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
    let mut r = 0usize;
    let mut w = 0usize;
    while r < src.len() {
        let mut flags = src[r];
        r += 1;
        for _ in 0..8 {
            if r >= src.len() {
                break;
            }
            if (flags & 1) == 0 {
                if w >= dst.len() {
                    return Err(());
                }
                dst[w] = src[r];
                r += 1;
                w += 1;
            } else {
                if r + 2 > src.len() {
                    return Err(());
                }
                let lo = src[r] as usize;
                let hi_len = src[r + 1] as usize;
                r += 2;
                let dist = (lo | ((hi_len & 0xF0) << 4)) + 1;
                let len = (hi_len & 0x0F) + 3;
                if dist > w || w + len > dst.len() {
                    return Err(());
                }
                for _ in 0..len {
                    dst[w] = dst[w - dist];
                    w += 1;
                }
            }
            flags >>= 1;
        }
    }
    Ok(w)
}

/// Zero-scratch in-place `LZSS` decompressor where compressed bytes sit at `buf[comp_off..comp_end]`.
/// Safe whenever the packer verified `w <= r` across all tokens (guaranteed with `<= 16 B` tail slack).
pub fn lzss_decompress_inplace(
    buf: &mut [u8],
    comp_off: usize,
    comp_len: usize,
) -> Result<usize, ()> {
    let comp_end = comp_off.checked_add(comp_len).ok_or(())?;
    if comp_end > buf.len() {
        return Err(());
    }
    let mut r = comp_off;
    let mut w = 0usize;
    while r < comp_end {
        let mut flags = buf[r];
        r += 1;
        for _ in 0..8 {
            if r >= comp_end {
                break;
            }
            if (flags & 1) == 0 {
                if w >= r {
                    if w > r {
                        return Err(());
                    }
                }
                buf[w] = buf[r];
                r += 1;
                w += 1;
            } else {
                if r + 2 > comp_end {
                    return Err(());
                }
                let lo = buf[r] as usize;
                let hi_len = buf[r + 1] as usize;
                r += 2;
                let dist = (lo | ((hi_len & 0xF0) << 4)) + 1;
                let len = (hi_len & 0x0F) + 3;
                if dist > w || w + len > r {
                    return Err(());
                }
                for _ in 0..len {
                    buf[w] = buf[w - dist];
                    w += 1;
                }
            }
            flags >>= 1;
        }
    }
    Ok(w)
}

/// Number of unused UTF-8/ASCII byte codes (`42` tokens -> `84 B` pair table, padded to `96 B` in `LNG1`).
pub const BPE_TOKEN_COUNT: usize = 42;
pub const BPE_DICT_BYTES: usize = 96;

/// Map a byte to a BPE pair index `0..42`, or `None` if `b` is a literal UTF-8/ASCII byte.
/// Uses strictly bytes that never appear in NFC-normalized UTF-8 UI strings:
/// - `0x01..=0x09` (9 tokens: `0..9`)
/// - `0x0B..=0x1F` (21 tokens: `9..30`, leaving `\n = 0x0A` untouched!)
/// - `0x7F` (1 token: `30`)
/// - `0xC0..=0xC1` (2 tokens: `31..33`, invalid overlong UTF-8 leads)
/// - `0xF5..=0xFD` (9 tokens: `33..42`, invalid >U+10FFFF UTF-8 leads)
#[inline(always)]
pub const fn bpe_token_index(b: u8) -> Option<usize> {
    if b >= 0x01 && b <= 0x09 {
        Some((b - 0x01) as usize)
    } else if b >= 0x0B && b <= 0x1F {
        Some(9 + (b - 0x0B) as usize)
    } else if b == 0x7F {
        Some(30)
    } else if b == 0xC0 || b == 0xC1 {
        Some(31 + (b - 0xC0) as usize)
    } else if b >= 0xF5 && b <= 0xFD {
        Some(33 + (b - 0xF5) as usize)
    } else {
        None
    }
}

/// Expand a BPE-compressed UTF-8 string into `out` using a `16 B` bounded stack (`0` heap!).
pub fn bpe_expand(dict: &[u8; BPE_DICT_BYTES], src: &[u8], out: &mut [u8]) -> Option<usize> {
    let mut stack = [0u8; 16];
    let mut w = 0usize;
    for &raw in src {
        stack[0] = raw;
        let mut sp = 1usize;
        while sp > 0 {
            sp -= 1;
            let sym = stack[sp];
            if let Some(tidx) = bpe_token_index(sym) {
                if sp + 2 > stack.len() {
                    return None;
                }
                let p = tidx << 1;
                // Push right first so left is popped first (in-order expansion).
                stack[sp] = dict[p + 1];
                stack[sp + 1] = dict[p];
                sp += 2;
            } else {
                if w >= out.len() {
                    return None;
                }
                out[w] = sym;
                w += 1;
            }
        }
    }
    Some(w)
}

/// Decode escape-run RLE (`esc <count>` skips `count` `esc` bytes; else literal) into `dst`
/// starting at logical byte offset `start`.
pub fn decode_rle_escape(src: &[u8], esc: u8, start: usize, dst: &mut [u8]) {
    dst.fill(esc);
    let end = start + dst.len();
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < src.len() && pos < end {
        let b = src[i];
        i += 1;
        if b == esc {
            pos += src[i] as usize;
            i += 1;
        } else {
            if pos >= start {
                dst[pos - start] = b;
            }
            pos += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rle_escape_decodes_full_and_partial_ranges() {
        let mut out = [0u8; 6];
        let enc = [0xAA, 0xFF, 0x04, 0xBB];
        decode_rle_escape(&enc, 0xFF, 0, &mut out);
        assert_eq!(out, [0xAA, 0xFF, 0xFF, 0xFF, 0xFF, 0xBB]);
        let mut sub = [0u8; 2];
        decode_rle_escape(&enc, 0xFF, 4, &mut sub);
        assert_eq!(sub, [0xFF, 0xBB]);
    }

    #[test]
    fn packbits_roundtrip() {
        let data = b"AAAAABcdeFFFFFF00000000xyz";
        let mut comp = [0u8; 64];
        let mut dec = [0u8; 64];
        let clen = packbits_compress(data, &mut comp).unwrap();
        assert!(clen < data.len());
        let dlen = packbits_decompress(&comp[..clen], &mut dec).unwrap();
        assert_eq!(&dec[..dlen], data);
    }

    #[test]
    fn lzss_out_of_place_and_inplace() {
        // 1 flag byte `0b0000_0010`: literal 'A', match dist=1 len=5 ("AAAAA"), literal 'B' -> "AAAAAAB"
        let stream = [0b0000_0010u8, b'A', 0x00, 0x02, b'B'];
        let mut out = [0u8; 16];
        let n = lzss_decompress(&stream, &mut out).unwrap();
        assert_eq!(&out[..n], b"AAAAAAB");

        let mut inplace = [0u8; 16];
        let off = 16 - stream.len();
        inplace[off..].copy_from_slice(&stream);
        let n2 = lzss_decompress_inplace(&mut inplace, off, stream.len()).unwrap();
        assert_eq!(&inplace[..n2], b"AAAAAAB");
    }

    #[test]
    fn bpe_recursive_expansion_preserves_utf8() {
        let mut dict = [0u8; BPE_DICT_BYTES];
        // Token 0 (0x01) = "Fr", Token 1 (0x02) = [0x01, 'e'] ("Fre"), Token 2 (0x03) = [0x02, 'q'] ("Freq")
        dict[0] = b'F';
        dict[1] = b'r';
        dict[2] = 0x01;
        dict[3] = b'e';
        dict[4] = 0x02;
        dict[5] = b'q';
        let compressed = [0x03, b' ', 0xD0, 0x90]; // "Freq А"
        let mut out = [0u8; 16];
        let n = bpe_expand(&dict, &compressed, &mut out).unwrap();
        assert_eq!(&out[..n], "Freq А".as_bytes());
    }
}
