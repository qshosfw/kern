//! Pure `#![no_std]` Rust implementation of the Ascon-128 Cryptographic Suite (NIST SP 800-232).
//!
//! Theoretical limit optimization for Cortex-M0+ (ARMv6-M Thumb-1):
//! - Zero memory allocations (`#![no_std]`).
//! - Single compact, rolled 320-bit permutation engine `p` shared across all modes.
//! - Constant-time execution: zero data-dependent branches, zero look-up tables (cache-timing immune).
//! - Zero hardware multiplier dependency (only bitwise EOR, AND, BIC, and ROR/LSL/LSR).
//!
//! Provides:
//! - `AsconPermutation`: The core 320-bit permutation (p^12, p^8, p^6).
//! - `Ascon128`: Authenticated Encryption with Associated Data (AEAD) with 128-bit key, 128-bit nonce, and 128-bit tag.
//! - `AsconHash256`: NIST SP 800-232 256-bit cryptographic hash function.
//! - `AsconXof`: NIST SP 800-232 Extensible Output Function for sponge squeezing.

pub const KEY_SIZE: usize = 16;
pub const NONCE_SIZE: usize = 16;
pub const TAG_SIZE: usize = 16;
pub const HASH_SIZE: usize = 32;

/// Round constants for Ascon permutation (NIST SP 800-232 Table 2).
/// Added to state word x2 for rounds 0..11.
pub const ROUND_CONSTANTS: [u64; 12] = [
    0x0000_0000_0000_00f0,
    0x0000_0000_0000_00e1,
    0x0000_0000_0000_00d2,
    0x0000_0000_0000_00c3,
    0x0000_0000_0000_00b4,
    0x0000_0000_0000_00a5,
    0x0000_0000_0000_0096,
    0x0000_0000_0000_0087,
    0x0000_0000_0000_0078,
    0x0000_0000_0000_0069,
    0x0000_0000_0000_005a,
    0x0000_0000_0000_004b,
];

/// 320-bit Ascon State consisting of 5 64-bit words (x0, x1, x2, x3, x4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsconPermutation {
    pub x: [u64; 5],
}

impl AsconPermutation {
    /// Initialize state with all zeros.
    pub const fn new() -> Self {
        Self { x: [0u64; 5] }
    }

    /// Construct state from 5 64-bit words.
    pub const fn from_words(w: [u64; 5]) -> Self {
        Self { x: w }
    }

    /// Apply `rounds` rounds of the Ascon permutation in a compact 32-bit Thumb-1 loop.
    pub fn permute(&mut self, rounds: usize) {
        fn sbox32(w: &mut [u32; 5]) {
            w[0] ^= w[4];
            w[4] ^= w[3];
            w[2] ^= w[1];
            let t0 = w[0] ^ (!w[1] & w[2]);
            let t1 = w[1] ^ (!w[2] & w[3]);
            let t2 = w[2] ^ (!w[3] & w[4]);
            let t3 = w[3] ^ (!w[4] & w[0]);
            let t4 = w[4] ^ (!w[0] & w[1]);
            w[1] = t1 ^ t0;
            w[0] = t0 ^ t4;
            w[3] = t3 ^ t2;
            w[2] = !t2;
            w[4] = t4;
        }

        fn rotr_pair(hi: u32, lo: u32, r: u8) -> (u32, u32) {
            let (a, b, s) = if r >= 32 {
                (lo, hi, (r - 32) as u32)
            } else {
                (hi, lo, r as u32)
            };
            let inv = 32 - s;
            ((a >> s) | (b << inv), (b >> s) | (a << inv))
        }

        const ROT: [(u8, u8); 5] = [(19, 28), (61, 39), (1, 6), (10, 17), (7, 41)];

        let mut h = [0u32; 5];
        let mut l = [0u32; 5];
        let mut i = 0;
        while i < 5 {
            h[i] = (self.x[i] >> 32) as u32;
            l[i] = self.x[i] as u32;
            i += 1;
        }

        let start = 12usize.saturating_sub(rounds);
        for r in start..12 {
            // 1. Constant Addition (p_C: low byte of x2 only)
            l[2] ^= (0xf0usize - r * 0x0f) as u32;

            // 2. Bit-sliced 5-bit S-box on upper and lower 32-bit halves independently
            sbox32(&mut h);
            sbox32(&mut l);

            // 3. Linear Diffusion Layer (p_L: pure 32-bit shifts, zero 64-bit helpers)
            let mut w = 0;
            while w < 5 {
                let (r0h, r0l) = rotr_pair(h[w], l[w], ROT[w].0);
                let (r1h, r1l) = rotr_pair(h[w], l[w], ROT[w].1);
                h[w] ^= r0h ^ r1h;
                l[w] ^= r0l ^ r1l;
                w += 1;
            }
        }

        let mut j = 0;
        while j < 5 {
            self.x[j] = ((h[j] as u64) << 32) | (l[j] as u64);
            j += 1;
        }
    }
}

/// Ascon-128 Authenticated Encryption Engine (NIST SP 800-232).
#[derive(Clone)]
pub struct Ascon128 {
    key: [u8; KEY_SIZE],
}

impl Drop for Ascon128 {
    fn drop(&mut self) {
        crate::buffer::zeroize_bytes(&mut self.key);
    }
}

fn read_be64(b: &[u8]) -> u64 {
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

fn write_be64(v: u64, dst: &mut [u8]) {
    dst[..8].copy_from_slice(&v.to_be_bytes());
}

fn absorb_bytes(s: &mut AsconPermutation, bytes: &[u8], rounds: usize, permute_last: bool) {
    let mut off = 0;
    while off + 8 <= bytes.len() {
        let mut xb = s.x[0].to_be_bytes();
        for j in 0..8 {
            xb[j] ^= bytes[off + j];
        }
        s.x[0] = u64::from_be_bytes(xb);
        s.permute(rounds);
        off += 8;
    }
    let rem = &bytes[off..];
    let mut xb = s.x[0].to_be_bytes();
    for j in 0..rem.len() {
        xb[j] ^= rem[j];
    }
    xb[rem.len()] ^= 0x80;
    s.x[0] = u64::from_be_bytes(xb);
    if permute_last {
        s.permute(rounds);
    }
}

fn ct_tag_eq(a: &[u8; TAG_SIZE], b: &[u8; TAG_SIZE]) -> bool {
    let mut diff = 0u8;
    for i in 0..TAG_SIZE {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

impl Ascon128 {
    /// Ascon-128 Initial Value (k=128, r=64, a=12, b=6)
    pub const ASCON128_IV: u64 = 0x8040_0c06_0000_0000;

    /// Create a new Ascon-128 cipher instance with a 128-bit key.
    pub const fn new(key: [u8; KEY_SIZE]) -> Self {
        Self { key }
    }

    /// Core Ascon-128 sponge processor (`mode`: `0 = MacOnly`, `1 = Encrypt`, `2 = Decrypt`).
    /// Unifies AEAD encryption, AEAD decryption, and detached MAC computation in one code path.
    fn process_sponge(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        data: &[u8],
        mut out_buf: Option<&mut [u8]>,
        decrypt: bool,
    ) -> [u8; TAG_SIZE] {
        let k0 = read_be64(&self.key[..8]);
        let k1 = read_be64(&self.key[8..]);
        let mut s = AsconPermutation::from_words([
            Self::ASCON128_IV,
            k0,
            k1,
            read_be64(&nonce[..8]),
            read_be64(&nonce[8..]),
        ]);
        s.permute(12);
        s.x[3] ^= k0;
        s.x[4] ^= k1;
        if !associated_data.is_empty() {
            absorb_bytes(&mut s, associated_data, 6, true);
        }
        s.x[4] ^= 1;

        let mut off = 0;
        loop {
            let rem = data.len() - off;
            let n = rem.min(8);
            let mut xb = s.x[0].to_be_bytes();
            for j in 0..n {
                let b = data[off + j];
                if let Some(buf) = out_buf.as_deref_mut() {
                    if decrypt {
                        buf[off + j] = xb[j] ^ b;
                        xb[j] = b;
                    } else {
                        xb[j] ^= b;
                        buf[off + j] = xb[j];
                    }
                } else {
                    xb[j] ^= b;
                }
            }
            if rem >= 8 {
                s.x[0] = u64::from_be_bytes(xb);
                s.permute(6);
                off += 8;
            } else {
                xb[n] ^= 0x80;
                s.x[0] = u64::from_be_bytes(xb);
                break;
            }
        }

        s.x[1] ^= k0;
        s.x[2] ^= k1;
        s.permute(12);
        s.x[3] ^= k0;
        s.x[4] ^= k1;

        let mut tag = [0u8; TAG_SIZE];
        write_be64(s.x[3], &mut tag[0..8]);
        write_be64(s.x[4], &mut tag[8..16]);
        tag
    }

    pub fn duplex_crypt(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        buffer: &mut [u8],
        decrypt: bool,
    ) -> [u8; TAG_SIZE] {
        let input = unsafe { core::slice::from_raw_parts(buffer.as_ptr(), buffer.len()) };
        self.process_sponge(nonce, associated_data, input, Some(buffer), decrypt)
    }

    /// Encrypt plaintext in-place and return the 128-bit authentication tag.
    pub fn encrypt_in_place_detached(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        buffer: &mut [u8],
    ) -> [u8; TAG_SIZE] {
        self.duplex_crypt(nonce, associated_data, buffer, false)
    }

    /// Decrypt ciphertext in-place after verifying the 128-bit authentication tag in constant time.
    /// If verification fails, buffer is zeroized and `Err(())` is returned.
    pub fn decrypt_in_place_detached(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        buffer: &mut [u8],
        tag: &[u8; TAG_SIZE],
    ) -> Result<(), ()> {
        let expected_tag = self.duplex_crypt(nonce, associated_data, buffer, true);
        if ct_tag_eq(tag, &expected_tag) {
            Ok(())
        } else {
            crate::buffer::zeroize_bytes(buffer);
            Err(())
        }
    }

    /// Initialize an Ascon-128 keystream permutation state for chunked streaming (multiples of 8 B).
    pub fn keystream_state(&self, nonce: &[u8; NONCE_SIZE]) -> AsconPermutation {
        let k0 = read_be64(&self.key[..8]);
        let k1 = read_be64(&self.key[8..]);
        let mut s = AsconPermutation::from_words([
            Self::ASCON128_IV,
            k0,
            k1,
            read_be64(&nonce[..8]),
            read_be64(&nonce[8..]),
        ]);
        s.permute(12);
        s.x[3] ^= k0;
        s.x[4] ^= k1 ^ 1;
        s
    }

    /// XOR the next keystream bytes into `buffer`. All non-final chunks must be multiples of 8 bytes.
    pub fn apply_keystream_chunk(s: &mut AsconPermutation, buffer: &mut [u8]) {
        let mut offset = 0;
        while offset < buffer.len() {
            let chunk_len = (buffer.len() - offset).min(8);
            let state_bytes = s.x[0].to_be_bytes();
            for j in 0..chunk_len {
                buffer[offset + j] ^= state_bytes[j];
            }
            offset += chunk_len;
            s.permute(6);
        }
    }

    /// Apply keystream in-place to buffer (stream encryption/decryption).
    pub fn apply_keystream(&self, nonce: &[u8; NONCE_SIZE], buffer: &mut [u8]) {
        let mut s = self.keystream_state(nonce);
        Self::apply_keystream_chunk(&mut s, buffer);
    }

    /// Compute Ascon-128 MAC tag over associated data and payload.
    pub fn compute_mac(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        data: &[u8],
    ) -> [u8; TAG_SIZE] {
        self.process_sponge(nonce, associated_data, data, None, false)
    }

    /// Verify Ascon-128 MAC tag in constant time.
    pub fn verify_mac(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        data: &[u8],
        tag: &[u8; TAG_SIZE],
    ) -> Result<(), ()> {
        let expected = self.compute_mac(nonce, associated_data, data);
        if ct_tag_eq(tag, &expected) {
            Ok(())
        } else {
            Err(())
        }
    }
}

/// Ascon-Hash256 (NIST SP 800-232 256-bit hash function).
pub struct AsconHash256;

impl AsconHash256 {
    /// Ascon-Hash IV: 0x0040_0c00_0000_0100
    pub const HASH_IV: u64 = 0x0040_0c00_0000_0100;

    /// Compute 256-bit Ascon-Hash over input data.
    pub fn hash(data: &[u8]) -> [u8; HASH_SIZE] {
        let mut s = AsconPermutation::from_words([Self::HASH_IV, 0, 0, 0, 0]);
        s.permute(12);
        absorb_bytes(&mut s, data, 12, true);

        let mut out = [0u8; HASH_SIZE];
        for i in 0..4 {
            write_be64(s.x[0], &mut out[i * 8..(i + 1) * 8]);
            if i < 3 {
                s.permute(12);
            }
        }
        out
    }
}

/// Ascon-XOF (NIST SP 800-232 Extensible Output Function).
pub struct AsconXof {
    s: AsconPermutation,
}

impl AsconXof {
    /// Ascon-XOF IV: 0x0040_0c00_0000_0000
    pub const XOF_IV: u64 = 0x0040_0c00_0000_0000;

    /// Create new XOF instance.
    pub fn new() -> Self {
        let mut s = AsconPermutation::from_words([Self::XOF_IV, 0, 0, 0, 0]);
        s.permute(12);
        Self { s }
    }

    /// Absorb arbitrary bytes into the sponge state.
    pub fn absorb(&mut self, data: &[u8]) {
        absorb_bytes(&mut self.s, data, 12, true);
    }

    /// Squeeze arbitrary bytes from the sponge state.
    pub fn squeeze(&mut self, out: &mut [u8]) {
        let mut offset = 0;
        while offset < out.len() {
            let chunk_len = (out.len() - offset).min(8);
            let state_bytes = self.s.x[0].to_be_bytes();
            out[offset..offset + chunk_len].copy_from_slice(&state_bytes[..chunk_len]);
            offset += chunk_len;
            if offset < out.len() {
                self.s.permute(12);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascon128_known_vector() {
        let key = [0u8; KEY_SIZE];
        let nonce = [0u8; NONCE_SIZE];
        let cipher = Ascon128::new(key);
        let mut pt = [];
        let tag = cipher.encrypt_in_place_detached(&nonce, &[], &mut pt);

        let expected_tag = [
            0x42, 0x21, 0x3f, 0x50, 0xa8, 0x11, 0xd2, 0xd1, 0xd7, 0xe4, 0x09, 0x2a, 0xa2, 0xa4,
            0x2b, 0xa4,
        ];
        assert_eq!(tag, expected_tag);

        let key_inc: [u8; KEY_SIZE] = core::array::from_fn(|i| i as u8);
        let nonce_inc: [u8; NONCE_SIZE] = core::array::from_fn(|i| i as u8);
        let cipher_inc = Ascon128::new(key_inc);
        let tag_inc = cipher_inc.encrypt_in_place_detached(&nonce_inc, &[], &mut pt);
        let expected_inc = [
            0xe3, 0x55, 0x15, 0x9f, 0x29, 0x29, 0x11, 0xf7, 0x94, 0xcb, 0x14, 0x32, 0xa0, 0x10,
            0x3a, 0x8a,
        ];
        assert_eq!(tag_inc, expected_inc);
    }

    #[test]
    fn test_ascon128_roundtrip_all_sizes() {
        let key = [0x5au8; KEY_SIZE];
        let nonce = [0xa5u8; NONCE_SIZE];
        let cipher = Ascon128::new(key);
        let aad = b"AUTH_ASSOCIATED_DATA";

        for len in 0..65 {
            let mut original = [0u8; 65];
            for i in 0..len {
                original[i] = (i as u8).wrapping_mul(7).wrapping_add(1);
            }

            let mut buffer = [0u8; 65];
            buffer[..len].copy_from_slice(&original[..len]);

            let tag = cipher.encrypt_in_place_detached(&nonce, aad, &mut buffer[..len]);

            assert!(
                cipher
                    .decrypt_in_place_detached(&nonce, aad, &mut buffer[..len], &tag)
                    .is_ok()
            );
            assert_eq!(&buffer[..len], &original[..len]);
        }
    }

    #[test]
    fn test_ascon128_tamper_rejection() {
        let key = [0x12u8; KEY_SIZE];
        let nonce = [0x34u8; NONCE_SIZE];
        let cipher = Ascon128::new(key);
        let aad = b"AAD";

        let data = *b"Hello World Secret Message!";
        let mut copy = data;
        let tag = cipher.encrypt_in_place_detached(&nonce, aad, &mut copy);

        let mut corrupted = copy;
        corrupted[0] ^= 1;
        assert!(
            cipher
                .decrypt_in_place_detached(&nonce, aad, &mut corrupted, &tag)
                .is_err()
        );

        let mut corrupted_tag = tag;
        corrupted_tag[15] ^= 1;
        let mut copy2 = copy;
        assert!(
            cipher
                .decrypt_in_place_detached(&nonce, aad, &mut copy2, &corrupted_tag)
                .is_err()
        );

        let mut copy3 = copy;
        assert!(
            cipher
                .decrypt_in_place_detached(&nonce, b"BAD_AAD", &mut copy3, &tag)
                .is_err()
        );
    }

    #[test]
    fn test_ascon_keystream_chunked_matches_oneshot() {
        let key = [0x33u8; KEY_SIZE];
        let nonce = [0x44u8; NONCE_SIZE];
        let cipher = Ascon128::new(key);

        let mut oneshot = [0u8; 195];
        for (i, b) in oneshot.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        let mut chunked = oneshot;

        cipher.apply_keystream(&nonce, &mut oneshot);
        let mut ks = cipher.keystream_state(&nonce);
        for chunk in chunked.chunks_mut(64) {
            Ascon128::apply_keystream_chunk(&mut ks, chunk);
        }
        assert_eq!(oneshot, chunked);

        // Decryption via oneshot restores plaintext
        cipher.apply_keystream(&nonce, &mut chunked);
        for (i, &b) in chunked.iter().enumerate() {
            assert_eq!(b, (i as u8).wrapping_mul(37).wrapping_add(11));
        }
    }

    #[test]
    fn test_ascon_mac_roundtrip() {
        let key = [0x77u8; KEY_SIZE];
        let nonce = [0x88u8; NONCE_SIZE];
        let cipher = Ascon128::new(key);

        let data = b"Device silicon authentication payload";
        let aad = b"CALIB";
        let tag = cipher.compute_mac(&nonce, aad, data);

        assert!(cipher.verify_mac(&nonce, aad, data, &tag).is_ok());
        assert!(cipher.verify_mac(&nonce, b"WRONG", data, &tag).is_err());
        assert!(cipher.verify_mac(&nonce, aad, b"TAMPERED", &tag).is_err());
    }

    #[test]
    fn test_ascon_hash256_vector() {
        let digest = AsconHash256::hash(b"");
        let expected = [
            0x73, 0x46, 0xbc, 0x14, 0xf0, 0x36, 0xe8, 0x7a, 0xe0, 0x3d, 0x09, 0x97, 0x91, 0x30,
            0x88, 0xf5, 0xf6, 0x84, 0x11, 0x43, 0x4b, 0x3c, 0xf8, 0xb5, 0x4f, 0xa7, 0x96, 0xa8,
            0x0d, 0x25, 0x1f, 0x91,
        ];
        assert_eq!(digest, expected);
    }

    #[test]
    fn test_ascon_xof() {
        let mut xof = AsconXof::new();
        xof.absorb(b"Input entropy data");
        let mut out1 = [0u8; 32];
        let mut out2 = [0u8; 32];
        xof.squeeze(&mut out1);
        xof.squeeze(&mut out2);
        assert_ne!(out1, [0u8; 32]);
        assert_ne!(out2, [0u8; 32]);
        assert_ne!(out1, out2);
    }
}
