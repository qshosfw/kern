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

    /// Apply `rounds` rounds of the Ascon permutation in a compact rolled loop.
    /// - For p^12 (a=12): rounds = 12 (uses constants 0..12)
    /// - For p^8  (b=8):  rounds = 8  (uses constants 4..12)
    /// - For p^6  (b=6):  rounds = 6  (uses constants 6..12)
    #[inline(never)]
    pub fn permute(&mut self, rounds: usize) {
        let start = 12usize.saturating_sub(rounds);
        for r in start..12 {
            // NIST SP 800-232 Table 2: round constant r is 0xf0 - r * 0x0f (low byte only).
            self.round((0xf0usize - r * 0x0f) as u64);
        }
    }

    /// Single round: constant addition, 5-bit bit-sliced S-box, linear diffusion layer.
    #[inline(always)]
    fn round(&mut self, cr: u64) {
        // 1. Constant Addition (p_C)
        self.x[2] ^= cr;

        // 2. Non-linear Substitution Layer (p_S: 5-bit S-box across 64 slices)
        self.x[0] ^= self.x[4];
        self.x[4] ^= self.x[3];
        self.x[2] ^= self.x[1];

        let t0 = self.x[0] ^ (!self.x[1] & self.x[2]);
        let t1 = self.x[1] ^ (!self.x[2] & self.x[3]);
        let t2 = self.x[2] ^ (!self.x[3] & self.x[4]);
        let t3 = self.x[3] ^ (!self.x[4] & self.x[0]);
        let t4 = self.x[4] ^ (!self.x[0] & self.x[1]);

        let s1 = t1 ^ t0;
        let s0 = t0 ^ t4;
        let s3 = t3 ^ t2;
        let s2 = !t2;

        self.x[0] = s0;
        self.x[1] = s1;
        self.x[2] = s2;
        self.x[3] = s3;
        self.x[4] = t4;

        // 3. Linear Diffusion Layer (p_L: 64-bit right rotations and XORs)
        self.x[0] ^= self.x[0].rotate_right(19) ^ self.x[0].rotate_right(28);
        self.x[1] ^= self.x[1].rotate_right(61) ^ self.x[1].rotate_right(39);
        self.x[2] ^= self.x[2].rotate_right(1) ^ self.x[2].rotate_right(6);
        self.x[3] ^= self.x[3].rotate_right(10) ^ self.x[3].rotate_right(17);
        self.x[4] ^= self.x[4].rotate_right(7) ^ self.x[4].rotate_right(41);
    }
}

/// Ascon-128 Authenticated Encryption Engine (NIST SP 800-232).
#[derive(Clone)]
pub struct Ascon128 {
    key: [u8; KEY_SIZE],
}

impl Ascon128 {
    /// Ascon-128 Initial Value (k=128, r=64, a=12, b=6)
    pub const ASCON128_IV: u64 = 0x8040_0c06_0000_0000;

    /// Create a new Ascon-128 cipher instance with a 128-bit key.
    pub const fn new(key: [u8; KEY_SIZE]) -> Self {
        Self { key }
    }

    #[inline(always)]
    fn init_state(&self, nonce: &[u8; NONCE_SIZE]) -> (AsconPermutation, u64, u64) {
        let k0 = u64::from_be_bytes([
            self.key[0],
            self.key[1],
            self.key[2],
            self.key[3],
            self.key[4],
            self.key[5],
            self.key[6],
            self.key[7],
        ]);
        let k1 = u64::from_be_bytes([
            self.key[8],
            self.key[9],
            self.key[10],
            self.key[11],
            self.key[12],
            self.key[13],
            self.key[14],
            self.key[15],
        ]);
        let n0 = u64::from_be_bytes([
            nonce[0], nonce[1], nonce[2], nonce[3], nonce[4], nonce[5], nonce[6], nonce[7],
        ]);
        let n1 = u64::from_be_bytes([
            nonce[8], nonce[9], nonce[10], nonce[11], nonce[12], nonce[13], nonce[14], nonce[15],
        ]);

        let mut s = AsconPermutation::from_words([Self::ASCON128_IV, k0, k1, n0, n1]);
        s.permute(12);
        s.x[3] ^= k0;
        s.x[4] ^= k1;

        (s, k0, k1)
    }

    #[inline(always)]
    fn process_associated_data(s: &mut AsconPermutation, associated_data: &[u8]) {
        if !associated_data.is_empty() {
            let full_blocks = associated_data.len() / 8;
            for i in 0..full_blocks {
                let chunk = &associated_data[i * 8..(i + 1) * 8];
                let block = u64::from_be_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
                ]);
                s.x[0] ^= block;
                s.permute(6);
            }
            let rem = &associated_data[full_blocks * 8..];
            let mut pad_block = [0u8; 8];
            pad_block[..rem.len()].copy_from_slice(rem);
            pad_block[rem.len()] = 0x80;
            let block = u64::from_be_bytes(pad_block);
            s.x[0] ^= block;
            s.permute(6);
        }
        // 1-bit domain separation
        s.x[4] ^= 1;
    }

    /// Encrypt plaintext in-place and return the 128-bit authentication tag.
    pub fn encrypt_in_place_detached(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        buffer: &mut [u8],
    ) -> [u8; TAG_SIZE] {
        let (mut s, k0, k1) = self.init_state(nonce);
        Self::process_associated_data(&mut s, associated_data);

        // Process full 8-byte plaintext blocks
        let full_blocks = buffer.len() / 8;
        for i in 0..full_blocks {
            let chunk = &mut buffer[i * 8..(i + 1) * 8];
            let p_word = u64::from_be_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            s.x[0] ^= p_word;
            chunk.copy_from_slice(&s.x[0].to_be_bytes());
            s.permute(6);
        }

        // Process final partial or empty block
        let rem_start = full_blocks * 8;
        let rem = &mut buffer[rem_start..];
        let rem_len = rem.len();
        let mut pad_block = [0u8; 8];

        let state_bytes = s.x[0].to_be_bytes();
        for j in 0..rem_len {
            let c = rem[j] ^ state_bytes[j];
            pad_block[j] = rem[j];
            rem[j] = c;
        }
        pad_block[rem_len] = 0x80;
        s.x[0] ^= u64::from_be_bytes(pad_block);

        // Finalization
        s.x[1] ^= k0;
        s.x[2] ^= k1;
        s.permute(12);
        s.x[3] ^= k0;
        s.x[4] ^= k1;

        let mut tag = [0u8; TAG_SIZE];
        tag[0..8].copy_from_slice(&s.x[3].to_be_bytes());
        tag[8..16].copy_from_slice(&s.x[4].to_be_bytes());
        tag
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
        let (mut s, k0, k1) = self.init_state(nonce);
        Self::process_associated_data(&mut s, associated_data);

        // Process full 8-byte ciphertext blocks
        let full_blocks = buffer.len() / 8;
        for i in 0..full_blocks {
            let chunk = &mut buffer[i * 8..(i + 1) * 8];
            let c_word = u64::from_be_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            let p_word = s.x[0] ^ c_word;
            s.x[0] = c_word;
            chunk.copy_from_slice(&p_word.to_be_bytes());
            s.permute(6);
        }

        // Process final partial or empty block
        let rem_start = full_blocks * 8;
        let rem = &mut buffer[rem_start..];
        let rem_len = rem.len();
        let mut pad_block = [0u8; 8];

        let state_bytes = s.x[0].to_be_bytes();
        for j in 0..rem_len {
            let p = rem[j] ^ state_bytes[j];
            pad_block[j] = p;
            rem[j] = p;
        }
        pad_block[rem_len] = 0x80;
        s.x[0] ^= u64::from_be_bytes(pad_block);

        // Finalization
        s.x[1] ^= k0;
        s.x[2] ^= k1;
        s.permute(12);
        s.x[3] ^= k0;
        s.x[4] ^= k1;

        let mut expected_tag = [0u8; TAG_SIZE];
        expected_tag[0..8].copy_from_slice(&s.x[3].to_be_bytes());
        expected_tag[8..16].copy_from_slice(&s.x[4].to_be_bytes());

        // Constant-time tag verification
        let mut diff = 0u8;
        for (a, b) in tag.iter().zip(expected_tag.iter()) {
            diff |= a ^ b;
        }

        if diff == 0 {
            Ok(())
        } else {
            // Secure wipe on authentication failure
            for byte in buffer.iter_mut() {
                unsafe { core::ptr::write_volatile(byte, 0) };
            }
            Err(())
        }
    }

    /// Apply keystream in-place to buffer (stream encryption/decryption).
    pub fn apply_keystream(&self, nonce: &[u8; NONCE_SIZE], buffer: &mut [u8]) {
        let (mut s, _, _) = self.init_state(nonce);
        s.x[4] ^= 1; // Domain separation

        let mut offset = 0;
        while offset < buffer.len() {
            let chunk_len = (buffer.len() - offset).min(8);
            let state_bytes = s.x[0].to_be_bytes();
            for j in 0..chunk_len {
                buffer[offset + j] ^= state_bytes[j];
            }
            offset += chunk_len;
            if offset < buffer.len() {
                s.permute(6);
            }
        }
    }

    /// Compute Ascon-128 MAC tag over associated data and payload.
    pub fn compute_mac(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        data: &[u8],
    ) -> [u8; TAG_SIZE] {
        let (mut s, k0, k1) = self.init_state(nonce);
        Self::process_associated_data(&mut s, associated_data);

        // Absorb data in 8-byte blocks
        let full_blocks = data.len() / 8;
        for i in 0..full_blocks {
            let chunk = &data[i * 8..(i + 1) * 8];
            let word = u64::from_be_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            s.x[0] ^= word;
            s.permute(6);
        }

        let rem = &data[full_blocks * 8..];
        let mut pad_block = [0u8; 8];
        pad_block[..rem.len()].copy_from_slice(rem);
        pad_block[rem.len()] = 0x80;
        s.x[0] ^= u64::from_be_bytes(pad_block);

        // Finalization
        s.x[1] ^= k0;
        s.x[2] ^= k1;
        s.permute(12);
        s.x[3] ^= k0;
        s.x[4] ^= k1;

        let mut tag = [0u8; TAG_SIZE];
        tag[0..8].copy_from_slice(&s.x[3].to_be_bytes());
        tag[8..16].copy_from_slice(&s.x[4].to_be_bytes());
        tag
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
        let mut diff = 0u8;
        for (a, b) in tag.iter().zip(expected.iter()) {
            diff |= a ^ b;
        }
        if diff == 0 { Ok(()) } else { Err(()) }
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

        // Absorb message in 64-bit blocks
        let full_blocks = data.len() / 8;
        for i in 0..full_blocks {
            let chunk = &data[i * 8..(i + 1) * 8];
            let word = u64::from_be_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            s.x[0] ^= word;
            s.permute(12);
        }

        // Pad last block with 0x80
        let rem = &data[full_blocks * 8..];
        let mut pad_block = [0u8; 8];
        pad_block[..rem.len()].copy_from_slice(rem);
        pad_block[rem.len()] = 0x80;
        s.x[0] ^= u64::from_be_bytes(pad_block);
        s.permute(12);

        // Squeeze 256 bits (4 x 64-bit words)
        let mut out = [0u8; HASH_SIZE];
        out[0..8].copy_from_slice(&s.x[0].to_be_bytes());
        s.permute(12);
        out[8..16].copy_from_slice(&s.x[0].to_be_bytes());
        s.permute(12);
        out[16..24].copy_from_slice(&s.x[0].to_be_bytes());
        s.permute(12);
        out[24..32].copy_from_slice(&s.x[0].to_be_bytes());
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
        let full_blocks = data.len() / 8;
        for i in 0..full_blocks {
            let chunk = &data[i * 8..(i + 1) * 8];
            let word = u64::from_be_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]);
            self.s.x[0] ^= word;
            self.s.permute(12);
        }

        let rem = &data[full_blocks * 8..];
        let mut pad_block = [0u8; 8];
        pad_block[..rem.len()].copy_from_slice(rem);
        pad_block[rem.len()] = 0x80;
        self.s.x[0] ^= u64::from_be_bytes(pad_block);
        self.s.permute(12);
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
