//! Zero-Allocation CRC-16 and CRC-32 Calculation Engine.

const fn make_crc32_nibble_lut() -> [u32; 16] {
    let mut lut = [0u32; 16];
    let mut i = 0usize;
    while i < 16 {
        let mut r = i as u32;
        let mut b = 0;
        while b < 4 {
            if (r & 1) != 0 {
                r = (r >> 1) ^ 0xEDB8_8320;
            } else {
                r >>= 1;
            }
            b += 1;
        }
        lut[i] = r;
        i += 1;
    }
    lut
}

static CRC32_NIBBLE: [u32; 16] = make_crc32_nibble_lut();

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

    /// Progressively update IEEE 802.3 CRC-32 state using a 16-entry nibble LUT (4x faster on M0+).
    pub fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
        for &byte in data {
            crc ^= byte as u32;
            crc = (crc >> 4) ^ CRC32_NIBBLE[(crc & 0x0F) as usize];
            crc = (crc >> 4) ^ CRC32_NIBBLE[(crc & 0x0F) as usize];
        }
        crc
    }

    /// Calculate CRC-32 for payload and flash integrity checking.
    pub fn crc32(data: &[u8]) -> u32 {
        !Self::crc32_update(0xFFFF_FFFF, data)
    }
}

#[cfg(test)]
mod tests {
    use super::CrcEngine;

    #[test]
    fn test_crc32_known_vector() {
        assert_eq!(CrcEngine::crc32(b"123456789"), 0xCBF4_3926);
        let progressive =
            !CrcEngine::crc32_update(CrcEngine::crc32_update(0xFFFF_FFFF, b"1234"), b"56789");
        assert_eq!(progressive, 0xCBF4_3926);
    }
}
