//! ChaCha20-Poly1305 AEAD (RFC 8439) Authenticated Encryption Engine.
//! Optimized for 32-bit ARM Cortex-M0+ with zero lookup tables, zero dynamic allocations,
//! 26-bit limb Poly1305 prime field reduction modulo 2^130 - 5, and constant-time tag verification.

pub const KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const TAG_SIZE: usize = 16;

/// ChaCha20-Poly1305 AEAD Engine
#[derive(Clone)]
pub struct ChaCha20Poly1305 {
    key: [u8; KEY_SIZE],
}

#[inline(always)]
fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

impl ChaCha20Poly1305 {
    pub const fn new(key: [u8; KEY_SIZE]) -> Self {
        Self { key }
    }

    #[inline(always)]
    fn qr(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
        let mut va = x[a];
        let mut vb = x[b];
        let mut vc = x[c];
        let mut vd = x[d];

        #[cfg(target_arch = "arm")]
        unsafe {
            core::arch::asm!(
                "adds {a}, {b}",
                "eors {d}, {a}",
                "movs {tmp}, #16",
                "rors {d}, {tmp}",

                "adds {c}, {d}",
                "eors {b}, {c}",
                "movs {tmp}, #20",
                "rors {b}, {tmp}",

                "adds {a}, {b}",
                "eors {d}, {a}",
                "movs {tmp}, #24",
                "rors {d}, {tmp}",

                "adds {c}, {d}",
                "eors {b}, {c}",
                "movs {tmp}, #25",
                "rors {b}, {tmp}",

                a = inout(reg) va,
                b = inout(reg) vb,
                c = inout(reg) vc,
                d = inout(reg) vd,
                tmp = out(reg) _,
                options(nomem, nostack),
            );
        }

        #[cfg(not(target_arch = "arm"))]
        {
            va = va.wrapping_add(vb);
            vd ^= va;
            vd = vd.rotate_left(16);
            vc = vc.wrapping_add(vd);
            vb ^= vc;
            vb = vb.rotate_left(12);
            va = va.wrapping_add(vb);
            vd ^= va;
            vd = vd.rotate_left(8);
            vc = vc.wrapping_add(vd);
            vb ^= vc;
            vb = vb.rotate_left(7);
        }

        x[a] = va;
        x[b] = vb;
        x[c] = vc;
        x[d] = vd;
    }

    /// Single ChaCha20 double-round: 4 column quarter-rounds followed by 4 diagonal quarter-rounds
    #[inline(always)]
    pub fn double_round(x: &mut [u32; 16]) {
        Self::qr(x, 0, 4, 8, 12);
        Self::qr(x, 1, 5, 9, 13);
        Self::qr(x, 2, 6, 10, 14);
        Self::qr(x, 3, 7, 11, 15);

        Self::qr(x, 0, 5, 10, 15);
        Self::qr(x, 1, 6, 11, 12);
        Self::qr(x, 2, 7, 8, 13);
        Self::qr(x, 3, 4, 9, 14);
    }

    /// 20 rounds of ChaCha (10 column + diagonal double rounds)
    #[inline(never)]
    pub fn rounds(x: &mut [u32; 16]) {
        for _ in 0..10 {
            Self::double_round(x);
        }
    }

    /// Generate a 64-byte ChaCha20 keystream block (RFC 8439 Section 2.3)
    pub fn block(&self, nonce: &[u8; NONCE_SIZE], counter: u32, out: &mut [u8; 64]) {
        let mut state = [0u32; 16];
        // Constants "expand 32-byte k"
        state[0] = 0x61707865;
        state[1] = 0x3320646e;
        state[2] = 0x79622d32;
        state[3] = 0x6b206574;

        // Key
        for i in 0..8 {
            state[4 + i] = read_u32_le(&self.key, i * 4);
        }

        // Counter
        state[12] = counter;

        // Nonce
        for i in 0..3 {
            state[13 + i] = read_u32_le(nonce, i * 4);
        }

        let mut working = state;
        Self::rounds(&mut working);

        for i in 0..16 {
            let val = working[i].wrapping_add(state[i]);
            out[i * 4..(i + 1) * 4].copy_from_slice(&val.to_le_bytes());
        }
    }

    /// Generate Poly1305 subkey from block counter 0 (first 32 bytes)
    fn generate_poly1305_key(&self, nonce: &[u8; NONCE_SIZE]) -> [u8; 32] {
        let mut key_block = [0u8; 64];
        self.block(nonce, 0, &mut key_block);
        let mut poly_key = [0u8; 32];
        poly_key.copy_from_slice(&key_block[..32]);
        poly_key
    }

    /// Compute Poly1305 MAC tag using 5 26-bit limbs arithmetic modulo 2^130 - 5
    fn compute_poly1305_tag(
        &self,
        poly_key: &[u8; 32],
        aad: &[u8],
        ciphertext: &[u8],
    ) -> [u8; TAG_SIZE] {
        // Clamp r (first 16 bytes of poly_key)
        let mut pad_r = [0u8; 17];
        pad_r[..16].copy_from_slice(&poly_key[..16]);
        pad_r[3] &= 15;
        pad_r[7] &= 15;
        pad_r[11] &= 15;
        pad_r[15] &= 15;
        pad_r[4] &= 252;
        pad_r[8] &= 252;
        pad_r[12] &= 252;

        let r = Self::decode_26bit_limbs(&pad_r);

        // Accumulator state: h[0]..h[4]
        let mut h = [0u32; 5];

        // 1. Process AAD and Ciphertext
        for data in [aad, ciphertext] {
            for chunk in data.chunks(16) {
                Self::poly1305_process_block(&mut h, &r, chunk);
            }
        }

        // 2. Process lengths block (8 bytes AAD len + 8 bytes Ciphertext len)
        let mut len_block = [0u8; 16];
        len_block[..8].copy_from_slice(&(aad.len() as u64).to_le_bytes());
        len_block[8..16].copy_from_slice(&(ciphertext.len() as u64).to_le_bytes());
        Self::poly1305_process_block(&mut h, &r, &len_block);

        // Fully reduce h modulo 2^130 - 5
        let mut c = 0u32;
        for i in 0..5 {
            h[i] += c;
            c = h[i] >> 26;
            h[i] &= 0x3FFFFFF;
        }
        h[0] += c * 5;
        let c = h[0] >> 26;
        h[0] &= 0x3FFFFFF;
        h[1] += c;

        // Compute h + 5
        let mut g = [0u32; 5];
        let mut c = 5u32;
        for i in 0..4 {
            let total = h[i] + c;
            c = total >> 26;
            g[i] = total & 0x3FFFFFF;
        }
        g[4] = h[4].wrapping_add(c).wrapping_sub(1 << 26);

        // Constant-time mask for (g[4] >> 31)
        let mask = (g[4] >> 31).wrapping_sub(1);
        for i in 0..5 {
            h[i] = (h[i] & !mask) | (g[i] & mask);
        }

        // Add s (poly_key[16..32])
        let mut tag = [0u8; 16];
        let mut c = 0u64;
        for i in 0..4 {
            let sh_low = (i * 6) as u32;
            let sh_high = 26 - sh_low;
            let s_val = read_u32_le(poly_key, 16 + i * 4) as u64;
            let fi = ((h[i] >> sh_low) | (h[i + 1] << sh_high)) as u64 + s_val + c;
            c = fi >> 32;
            tag[i * 4..i * 4 + 4].copy_from_slice(&(fi as u32).to_le_bytes());
        }
        tag
    }

    fn decode_26bit_limbs(bytes: &[u8; 17]) -> [u32; 5] {
        let mut limbs = [0u32; 5];
        for i in 0..4 {
            limbs[i] = (read_u32_le(bytes, i * 3) >> (i * 2)) & 0x3FFFFFF;
        }
        limbs[4] = (read_u32_le(bytes, 12) >> 8) | ((bytes[16] as u32) << 24);
        limbs
    }

    fn poly1305_process_block(h: &mut [u32; 5], r: &[u32; 5], block: &[u8]) {
        let mut pad = [0u8; 17];
        pad[..block.len()].copy_from_slice(block);
        pad[block.len()] = 1;

        let limbs = Self::decode_26bit_limbs(&pad);
        for i in 0..5 {
            h[i] += limbs[i];
        }

        let mut d = [0u64; 5];
        for i in 0..5 {
            let mut sum = 0u64;
            for j in 0..=i {
                sum += (h[j] as u64) * (r[i - j] as u64);
            }
            for j in (i + 1)..5 {
                sum += (h[j] as u64) * (r[5 + i - j] as u64 * 5);
            }
            d[i] = sum;
        }

        let mut c = 0u64;
        for i in 0..5 {
            let total = d[i] + c;
            c = total >> 26;
            h[i] = (total & 0x3FFFFFF) as u32;
        }

        h[0] += (c as u32) * 5;
        let c = h[0] >> 26;
        h[0] &= 0x3FFFFFF;
        h[1] += c;
    }

    pub fn apply_keystream(&self, nonce: &[u8; NONCE_SIZE], buffer: &mut [u8]) {
        let mut counter = 1u32;
        let mut keystream = [0u8; 64];

        for chunk in buffer.chunks_mut(64) {
            self.block(nonce, counter, &mut keystream);
            counter = counter.wrapping_add(1);
            for (b, k) in chunk.iter_mut().zip(keystream.iter()) {
                *b ^= *k;
            }
        }
    }

    /// Encrypt payload in-place using ChaCha20 stream cipher and compute Poly1305 MAC tag.
    pub fn encrypt_in_place_detached(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        buffer: &mut [u8],
    ) -> Result<[u8; TAG_SIZE], ()> {
        let poly_key = self.generate_poly1305_key(nonce);
        self.apply_keystream(nonce, buffer);
        let tag = self.compute_poly1305_tag(&poly_key, associated_data, buffer);
        Ok(tag)
    }

    /// Compute Poly1305 MAC tag signature over unencrypted payload and associated data.
    pub fn compute_mac(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        data: &[u8],
    ) -> [u8; TAG_SIZE] {
        let poly_key = self.generate_poly1305_key(nonce);
        self.compute_poly1305_tag(&poly_key, associated_data, data)
    }

    /// Verify Poly1305 MAC tag signature over unencrypted payload in constant time.
    pub fn verify_mac(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        data: &[u8],
        tag: &[u8; TAG_SIZE],
    ) -> Result<(), ()> {
        let expected_tag = self.compute_mac(nonce, associated_data, data);
        let mut diff = 0u8;
        for (a, b) in tag.iter().zip(expected_tag.iter()) {
            diff |= a ^ b;
        }
        if diff == 0 { Ok(()) } else { Err(()) }
    }

    /// Decrypt payload in-place using ChaCha20 after verifying Poly1305 MAC tag (constant-time).
    pub fn decrypt_in_place_detached(
        &self,
        nonce: &[u8; NONCE_SIZE],
        associated_data: &[u8],
        buffer: &mut [u8],
        tag: &[u8; TAG_SIZE],
    ) -> Result<(), ()> {
        self.verify_mac(nonce, associated_data, buffer, tag)?;
        self.apply_keystream(nonce, buffer);
        Ok(())
    }
}

#[cfg(test)]
pub struct CryptoBenchmarkResult {
    pub bytes_processed: usize,
    pub encrypt_success: bool,
    pub decrypt_success: bool,
}

#[cfg(test)]
impl ChaCha20Poly1305 {
    /// Benchmark ChaCha20-Poly1305 AEAD throughput and verify operational integrity
    pub fn run_self_benchmark(test_bytes: &mut [u8]) -> CryptoBenchmarkResult {
        let key = [0x42u8; KEY_SIZE];
        let nonce = [0x07u8; NONCE_SIZE];
        let aad = b"KERN-SYS-AAD";

        let aead = Self::new(key);
        let len = test_bytes.len();

        let tag_res = aead.encrypt_in_place_detached(&nonce, aad, test_bytes);
        let encrypt_success = tag_res.is_ok();

        let decrypt_success = if let Ok(tag) = tag_res {
            aead.decrypt_in_place_detached(&nonce, aad, test_bytes, &tag)
                .is_ok()
        } else {
            false
        };

        CryptoBenchmarkResult {
            bytes_processed: len,
            encrypt_success,
            decrypt_success,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chacha20_block_rfc8439() {
        let key = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ];
        let nonce = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00,
        ];
        let cipher = ChaCha20Poly1305::new(key);
        let mut out = [0u8; 64];
        cipher.block(&nonce, 1, &mut out);

        let expected_first_16 = [
            0x10, 0xf1, 0xe7, 0xe4, 0xd1, 0x3b, 0x59, 0x15, 0x50, 0x0f, 0xdd, 0x1f, 0xa3, 0x20,
            0x71, 0xc4,
        ];
        assert_eq!(&out[..16], &expected_first_16);
    }

    #[test]
    fn test_roundtrip_encryption() {
        let key = [0x11u8; 32];
        let nonce = [0x22u8; 12];
        let aad = b"HEADER_METADATA";
        let mut data = *b"Hello Microkernel Cryptography World!";

        let aead = ChaCha20Poly1305::new(key);
        let orig = data;

        let tag = aead
            .encrypt_in_place_detached(&nonce, aad, &mut data)
            .unwrap();
        assert_ne!(data, orig);

        aead.decrypt_in_place_detached(&nonce, aad, &mut data, &tag)
            .unwrap();
        assert_eq!(data, orig);
    }
}
