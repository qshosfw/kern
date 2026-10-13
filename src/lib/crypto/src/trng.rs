//! Hardware TRNG (True Random Number Generator) using BK4829 RF ExNoise, ADC Thermal Noise, and SysTick Micro-Jitter.
//! Inspired by DeltaFW's TRNG entropy pool architecture.

use crate::chacha20poly1305::ChaCha20Poly1305;

/// Continuous Entropy Health Monitor (NIST SP 800-90B style)
#[derive(Debug, Clone, Copy)]
pub struct EntropyHealth {
    pub last_sample: u32,
    pub repeat_count: u8,
    pub samples_tested: u16,
    pub is_healthy: bool,
}

impl EntropyHealth {
    pub const fn new() -> Self {
        Self {
            last_sample: 0,
            repeat_count: 0,
            samples_tested: 0,
            is_healthy: true,
        }
    }

    /// Repetition Count Test (RCT): Detects stuck noise sources.
    /// Returns false if the same raw noise value occurs >= 8 times consecutively.
    pub fn test_sample(&mut self, sample: u32) -> bool {
        self.samples_tested = self.samples_tested.saturating_add(1);
        if sample == self.last_sample && self.samples_tested > 1 {
            self.repeat_count = self.repeat_count.saturating_add(1);
            if self.repeat_count >= 8 {
                self.is_healthy = false;
                return false;
            }
        } else {
            self.last_sample = sample;
            self.repeat_count = 0;
        }
        true
    }
}

pub struct HardwareTrng {
    state: [u32; 16],
    reseed_counter: u32,
    initialized: bool,
    mix_idx: usize,
    pub health: EntropyHealth,
}

impl HardwareTrng {
    pub const fn new() -> Self {
        Self {
            state: [0; 16],
            reseed_counter: 0,
            initialized: false,
            mix_idx: 4,
            health: EntropyHealth::new(),
        }
    }

    #[inline(always)]
    fn rotl32(x: u32, n: u32) -> u32 {
        (x << n) | (x >> (32 - n))
    }

    /// Initialize the ChaCha20 entropy state with 96-bit MCU UID and microsecond hardware timing.
    pub fn init_with_uid(&mut self, mcu_uid: &[u8; 12], initial_systick: u32) {
        let uid_words = [
            u32::from_le_bytes([mcu_uid[0], mcu_uid[1], mcu_uid[2], mcu_uid[3]]),
            u32::from_le_bytes([mcu_uid[4], mcu_uid[5], mcu_uid[6], mcu_uid[7]]),
            u32::from_le_bytes([mcu_uid[8], mcu_uid[9], mcu_uid[10], mcu_uid[11]]),
        ];
        self.init(&uid_words, initial_systick);
    }

    /// Initialize the ChaCha20 entropy state with constant magic and MCU UID / SysTick seeds.
    pub fn init(&mut self, uid_words: &[u32; 3], initial_systick: u32) {
        // ChaCha20 "expand 32-byte k" constants
        self.state[0] = 0x61707865;
        self.state[1] = 0x3320646e;
        self.state[2] = 0x79622d32;
        self.state[3] = 0x6b206574;

        // Seed initial key state with Hardware UID
        self.state[4] = uid_words[0];
        self.state[5] = uid_words[1];
        self.state[6] = uid_words[2];

        for i in 7..16 {
            self.state[i] = initial_systick.wrapping_add(i as u32 * 0x9E3779B9);
        }

        self.mix_idx = 4;
        self.reseed_counter = 0;
        self.initialized = true;
    }

    /// Mix physical hardware noise into entropy pool.
    pub fn mix_hardware_entropy(&mut self, hw_noise: u32) {
        self.health.test_sample(hw_noise);
        let idx = self.mix_idx;
        self.state[idx] = Self::rotl32(self.state[idx] ^ hw_noise, 13);
        self.mix_idx = 4 + ((self.mix_idx - 4 + 1) % 8);
    }

    /// Fill buffer with random bytes (for nonces, salts, and AES keys)
    pub fn fill_bytes(&mut self, buf: &mut [u8], hw_noise: u32) {
        let mut offset = 0;
        while offset < buf.len() {
            if !self.initialized {
                let uid = [0x12345678, 0x9ABCDEF0, 0x01234567];
                self.init(&uid, hw_noise);
            }

            self.mix_hardware_entropy(hw_noise.wrapping_add(offset as u32));

            let mut working = self.state;
            ChaCha20Poly1305::rounds(&mut working);
            for i in 0..16 {
                working[i] = working[i].wrapping_add(self.state[i]);
            }

            // Forward secrecy: erase key portion with block output
            for i in 0..4 {
                self.state[4 + i] ^= working[i];
            }

            self.state[12] = self.state[12].wrapping_add(1);
            self.reseed_counter += 1;

            let mut word_idx = 0;
            while offset < buf.len() && word_idx < 16 {
                let bytes = working[word_idx].to_le_bytes();
                let chunk_len = (buf.len() - offset).min(4);
                buf[offset..offset + chunk_len].copy_from_slice(&bytes[..chunk_len]);
                offset += chunk_len;
                word_idx += 1;
            }
        }
    }

    /// Generate a 32-bit random integer with fast key erasure (forward secrecy).
    pub fn get_u32(&mut self, hw_noise: u32) -> u32 {
        let mut val = [0u8; 4];
        self.fill_bytes(&mut val, hw_noise);
        u32::from_le_bytes(val)
    }

    /// Self-test entropy pipeline: validates that generator does not return static/stuck words
    /// and that bit distribution across 16 words is balanced (Hamming weight between 20% and 80%).
    pub fn run_self_test(&mut self, base_noise: u32) -> bool {
        let mut prev = 0u32;
        let mut total_ones = 0u32;
        for i in 0..16 {
            let sample = self.get_u32(base_noise.wrapping_add((i as u32).wrapping_mul(0x9E3779B9)));
            if i > 0 && sample == prev {
                return false; // Repeated identical word output
            }
            prev = sample;
            total_ones += sample.count_ones();
        }
        // Across 16 32-bit words (512 bits), total ones should be between 102 and 410 bits (~20% to ~80%)
        total_ones >= 102 && total_ones <= 410 && self.health.is_healthy
    }
}
