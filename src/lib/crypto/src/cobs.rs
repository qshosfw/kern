//! High-Assurance, Zero-Allocation Consistent Overhead Byte Stuffing (COBS).
//!
//! Provides normative in-place and buffered COBS framing for serial protocols.
//! Guarantees that the frame delimiter byte `0x00` never appears in the encoded body.

pub const COBS_DELIMITER: u8 = 0x00;

pub struct Cobs;

impl Cobs {
    /// Calculate maximum worst-case encoded length for raw byte length `len`.
    #[inline(always)]
    pub const fn max_encoded_len(len: usize) -> usize {
        len + (len / 254) + 1
    }

    /// Encode `src` into `dst`. Returns encoded byte length on success.
    #[inline(never)]
    pub fn encode(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
        let max_len = Self::max_encoded_len(src.len());
        if dst.len() < max_len {
            return Err(());
        }

        let mut read_idx = 0;
        let mut write_idx = 1;
        let mut code_idx = 0;
        let mut code: u8 = 1;

        while read_idx < src.len() {
            let byte = src[read_idx];
            read_idx += 1;

            if byte == 0 {
                dst[code_idx] = code;
                code_idx = write_idx;
                write_idx += 1;
                code = 1;
            } else {
                dst[write_idx] = byte;
                write_idx += 1;
                code += 1;

                if code == 0xFF {
                    dst[code_idx] = code;
                    code_idx = write_idx;
                    write_idx += 1;
                    code = 1;
                }
            }
        }

        dst[code_idx] = code;
        Ok(write_idx)
    }

    /// Decode `src` into `dst`. Returns decoded byte length on success.
    pub fn decode(src: &[u8], dst: &mut [u8]) -> Result<usize, ()> {
        if src.is_empty() {
            return Ok(0);
        }

        let mut read_idx = 0;
        let mut write_idx = 0;

        while read_idx < src.len() {
            let code = src[read_idx];
            read_idx += 1;

            if code == 0 {
                return Err(()); // Zero byte inside encoded segment is invalid
            }

            let len = (code - 1) as usize;
            if read_idx + len > src.len() || write_idx + len > dst.len() {
                return Err(());
            }

            for _ in 0..len {
                let byte = src[read_idx];
                if byte == 0 {
                    return Err(());
                }
                dst[write_idx] = byte;
                read_idx += 1;
                write_idx += 1;
            }

            if code < 0xFF && read_idx < src.len() {
                if write_idx >= dst.len() {
                    return Err(());
                }
                dst[write_idx] = 0;
                write_idx += 1;
            }
        }

        Ok(write_idx)
    }

    /// In-place COBS decoder. Decodes directly within `buf` in-place.
    /// Invariant: write_idx <= read_idx at all steps.
    #[inline(never)]
    pub fn decode_inplace(buf: &mut [u8], len: usize) -> Result<usize, ()> {
        let len = len.min(buf.len());
        if len == 0 {
            return Ok(0);
        }
        let buf = &mut buf[..len];

        let mut read_idx = 0;
        let mut write_idx = 0;

        while read_idx < len {
            let code = buf[read_idx];
            read_idx += 1;

            if code == 0 {
                return Err(()); // Malformed COBS containing 0x00
            }

            let block_len = (code - 1) as usize;
            if read_idx + block_len > len {
                return Err(());
            }

            for _ in 0..block_len {
                let byte = buf[read_idx];
                if byte == 0 {
                    return Err(());
                }
                buf[write_idx] = byte;
                read_idx += 1;
                write_idx += 1;
            }

            if code < 0xFF && read_idx < len {
                buf[write_idx] = 0;
                write_idx += 1;
            }
        }

        Ok(write_idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cobs_empty() {
        let mut enc = [0u8; 8];
        let mut dec = [0u8; 8];
        assert_eq!(Cobs::encode(&[], &mut enc), Ok(1));
        assert_eq!(enc[0], 1); // code 1 for 0 bytes

        assert_eq!(Cobs::decode(&[], &mut dec), Ok(0));
        assert_eq!(Cobs::decode_inplace(&mut dec, 0), Ok(0));
    }

    #[test]
    fn test_cobs_standard_vectors() {
        let test_cases: &[(&[u8], &[u8])] = &[
            (&[0x00], &[0x01, 0x01]),
            (&[0x00, 0x00], &[0x01, 0x01, 0x01]),
            (&[0x11, 0x22, 0x00, 0x33], &[0x03, 0x11, 0x22, 0x02, 0x33]),
            (&[0x11, 0x22, 0x33, 0x44], &[0x05, 0x11, 0x22, 0x33, 0x44]),
        ];

        let mut enc_buf = [0u8; 16];
        let mut dec_buf = [0u8; 16];

        for &(raw, expected_enc) in test_cases {
            let enc_len = Cobs::encode(raw, &mut enc_buf).expect("encode");
            assert_eq!(&enc_buf[..enc_len], expected_enc);

            let dec_len = Cobs::decode(&enc_buf[..enc_len], &mut dec_buf).expect("decode");
            assert_eq!(&dec_buf[..dec_len], raw);

            let mut inplace_buf = [0u8; 16];
            inplace_buf[..enc_len].copy_from_slice(&enc_buf[..enc_len]);
            let in_len = Cobs::decode_inplace(&mut inplace_buf, enc_len).expect("inplace");
            assert_eq!(&inplace_buf[..in_len], raw);
        }
    }

    #[test]
    fn test_cobs_254_and_255_bytes() {
        let mut raw254 = [0u8; 254];
        for i in 0..254 {
            raw254[i] = ((i % 250) + 1) as u8;
        }

        let mut enc_buf = [0u8; 300];
        let mut dec_buf = [0u8; 300];

        let enc_len = Cobs::encode(&raw254, &mut enc_buf).expect("encode 254");
        assert_eq!(enc_buf[0], 0xFF);
        let dec_len = Cobs::decode(&enc_buf[..enc_len], &mut dec_buf).expect("decode 254");
        assert_eq!(&dec_buf[..dec_len], &raw254[..]);

        let mut inplace_buf = [0u8; 300];
        inplace_buf[..enc_len].copy_from_slice(&enc_buf[..enc_len]);
        let in_len = Cobs::decode_inplace(&mut inplace_buf, enc_len).expect("inplace 254");
        assert_eq!(&inplace_buf[..in_len], &raw254[..]);

        // 255 non-zero bytes (splits across 0xFF and second block)
        let mut raw255 = [0u8; 255];
        for i in 0..255 {
            raw255[i] = ((i % 250) + 1) as u8;
        }
        let enc_len2 = Cobs::encode(&raw255, &mut enc_buf).expect("encode 255");
        let dec_len2 = Cobs::decode(&enc_buf[..enc_len2], &mut dec_buf).expect("decode 255");
        assert_eq!(&dec_buf[..dec_len2], &raw255[..]);

        inplace_buf[..enc_len2].copy_from_slice(&enc_buf[..enc_len2]);
        let in_len2 = Cobs::decode_inplace(&mut inplace_buf, enc_len2).expect("inplace 255");
        assert_eq!(&inplace_buf[..in_len2], &raw255[..]);
    }

    #[test]
    fn test_cobs_malformed_rejection() {
        let mut dec_buf = [0u8; 16];
        // Stream containing zero byte
        assert_eq!(Cobs::decode(&[0x02, 0x00, 0x01], &mut dec_buf), Err(()));

        let mut inplace = [0x02, 0x00, 0x01];
        assert_eq!(Cobs::decode_inplace(&mut inplace, 3), Err(()));

        // Truncated stream: code 5 implies 4 data bytes, but only 2 available
        assert_eq!(Cobs::decode(&[0x05, 0x11, 0x22], &mut dec_buf), Err(()));
        let mut inplace2 = [0x05, 0x11, 0x22];
        assert_eq!(Cobs::decode_inplace(&mut inplace2, 3), Err(()));
    }
}
