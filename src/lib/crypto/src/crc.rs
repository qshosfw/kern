//! Zero-Allocation CRC-16 and CRC-32 Calculation Engine.

pub struct CrcEngine;

impl CrcEngine {
    /// Calculate CRC-16 CCITT (0x1021) for RF packet verification.
    pub fn crc16_ccitt(data: &[u8]) -> u16 {
        let mut crc: u16 = 0xFFFF;
        for &byte in data {
            crc ^= (byte as u16) << 8;
            for _ in 0..8 {
                if (crc & 0x8000) != 0 {
                    crc = (crc << 1) ^ 0x1021;
                } else {
                    crc <<= 1;
                }
            }
        }
        crc
    }

    /// Progressively update CRC-32 state.
    pub fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
        for &byte in data {
            crc ^= byte as u32;
            for _ in 0..8 {
                let mask = (-(crc as i32 & 1)) as u32;
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        crc
    }

    /// Calculate CRC-32 for payload and flash integrity checking.
    pub fn crc32(data: &[u8]) -> u32 {
        !Self::crc32_update(0xFFFF_FFFF, data)
    }
}
