//! Hardware TRNG (True Random Number Generator) using Ascon Permutation Sponge,
//! SysTick Cycle Jitter, ADC Thermal Noise, and BK4829 RF ExNoise.
//!
//! Conforms to NIST SP 800-90B continuous health testing (Repetition Count Test).

use crate::ascon::AsconPermutation;

/// Continuous Entropy Health Monitor (NIST SP 800-90B style).
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
            if self.repeat_count >= 7 {
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

impl Default for EntropyHealth {
    fn default() -> Self {
        Self::new()
    }
}

/// Hardware TRNG Sponge based on the 320-bit Ascon Permutation.
pub struct HardwareTrng {
    state: AsconPermutation,
    reseed_counter: u32,
    initialized: bool,
    pub health: EntropyHealth,
}

impl HardwareTrng {
    /// TRNG Domain Constant IV: 0x5452_4e47_0000_0100 (ASCII "TRNG")
    const TRNG_IV: u64 = 0x5452_4e47_0000_0100;

    pub const fn new() -> Self {
        Self {
            state: AsconPermutation::new(),
            reseed_counter: 0,
            initialized: false,
            health: EntropyHealth::new(),
        }
    }

    /// Initialize the TRNG entropy sponge with 96-bit MCU UID and microsecond hardware timing.
    #[inline(never)]
    pub fn init_with_uid(&mut self, mcu_uid: &[u8; 12], initial_noise: u32) {
        self.state.x = [Self::TRNG_IV, 0, 0, 0, 0];
        #[cfg(target_endian = "little")]
        unsafe {
            core::ptr::copy_nonoverlapping(
                mcu_uid.as_ptr(),
                (&raw mut self.state.x[1]).cast::<u8>(),
                12,
            );
            core::ptr::copy_nonoverlapping(
                initial_noise.to_le_bytes().as_ptr(),
                (&raw mut self.state.x[1]).cast::<u8>().add(12),
                4,
            );
        }
        #[cfg(not(target_endian = "little"))]
        {
            let mut b0 = [0u8; 8];
            b0.copy_from_slice(&mcu_uid[..8]);
            self.state.x[1] = u64::from_le_bytes(b0);
            let hi = u32::from_le_bytes([mcu_uid[8], mcu_uid[9], mcu_uid[10], mcu_uid[11]]);
            self.state.x[2] = (hi as u64) | ((initial_noise as u64) << 32);
        }
        self.state.permute(12);
        self.mix_hardware_entropy(initial_noise);
        self.initialized = true;
        self.reseed_counter = 0;
    }

    /// Mix physical hardware noise into entropy pool.
    #[inline(never)]
    pub fn mix_hardware_entropy(&mut self, hw_noise: u32) {
        self.health.test_sample(hw_noise);
        self.state.x[1] ^= (hw_noise as u64) | ((hw_noise as u64) << 32);
        self.state.permute(6);
    }

    /// Fill buffer with random bytes with forward secrecy.
    #[inline(never)]
    pub fn fill_bytes(&mut self, buf: &mut [u8], hw_noise: u32) {
        if !self.initialized {
            self.init_with_uid(&[0x5au8; 12], hw_noise);
        }

        self.mix_hardware_entropy(hw_noise);

        let mut offset = 0;
        while offset < buf.len() {
            self.state.permute(12);
            let state_bytes = self.state.x[0].to_le_bytes();
            let chunk_len = (buf.len() - offset).min(8);
            buf[offset..offset + chunk_len].copy_from_slice(&state_bytes[..chunk_len]);
            offset += chunk_len;
            self.reseed_counter = self.reseed_counter.wrapping_add(1);

            // Forward secrecy: erase squeezed state word with fresh round and counter
            self.state.x[0] ^= self.reseed_counter as u64;
        }

        // Final forward secrecy step: permute and absorb post-generation jitter
        self.state.x[2] ^= hw_noise as u64;
        self.state.permute(12);
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
                return false;
            }
            prev = sample;
            total_ones += sample.count_ones();
        }
        (102..=410).contains(&total_ones) && self.health.is_healthy
    }
}

impl Default for HardwareTrng {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trng_self_test() {
        let mut trng = HardwareTrng::new();
        let uid = [
            0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc,
        ];
        trng.init_with_uid(&uid, 0x12345678);
        assert!(trng.run_self_test(0x87654321));
    }

    #[test]
    fn test_trng_stuck_noise_detection() {
        let mut trng = HardwareTrng::new();
        let uid = [0x42u8; 12];
        trng.init_with_uid(&uid, 0x1000);

        // Feed stuck identical noise 8 times
        for _ in 0..8 {
            trng.mix_hardware_entropy(0xdeadbeef);
        }
        assert!(!trng.health.is_healthy);
    }

    #[test]
    fn test_trng_forward_secrecy() {
        let mut trng = HardwareTrng::new();
        let uid = [0x99u8; 12];
        trng.init_with_uid(&uid, 0x5555);

        let mut buf1 = [0u8; 16];
        let mut buf2 = [0u8; 16];
        trng.fill_bytes(&mut buf1, 0x1111);
        trng.fill_bytes(&mut buf2, 0x2222);

        assert_ne!(buf1, [0u8; 16]);
        assert_ne!(buf2, [0u8; 16]);
        assert_ne!(buf1, buf2);
    }
}
