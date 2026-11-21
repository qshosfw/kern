//! Microkernel Fixed-Size Power-of-Two Record Storage & Two-Tier VFO Persistence Contracts.
//!
//! Enforces $O(1)$ bit-shift record seeking (`offset = 16 + (index << shift)`),
//! sector-boundary alignment (4 KB LittleFS/KernFS blocks), and CRC32-sealed
//! 160-byte composite VFO snapshots (`/user/vfo_a.vfo`, `/user/vfo_b.vfo`).

use crypto::crc::CrcEngine;

/// Standard 16-byte table header size (`2^4`).
pub const TABLE_HEADER_SIZE: usize = 16;

/// FourCC format identifier for two-tier `.vfo` state files (`"VFOS"`).
pub const FILE_ID_VFO_STATE: [u8; 4] = *b"VFOS";

/// $O(1)$ bit-shift record byte offset inside a 16-byte-headed table file.
#[inline(always)]
pub const fn record_offset(index: usize, stride_shift: u32) -> usize {
    TABLE_HEADER_SIZE + (index << stride_shift)
}

/// 32-byte (`2^5`) header for a 160-byte Two-Tier VFO persistence file (`/user/vfo_a.vfo`, `/user/vfo_b.vfo`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct VfoStateHeader {
    pub ident: [u8; 4],
    pub version: u8,
    pub active_slot: u8,
    pub dual_watch: u8,
    pub cross_band: u8,
    pub save_epoch: u32,
    pub reserved: [u8; 16],
    pub crc32: u32,
}

const _: () = assert!(core::mem::size_of::<VfoStateHeader>() == 32);

/// 160-byte composite Two-Tier VFO snapshot (`32 B` header + `2 × 64 B` channel slots).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct VfoStateFile {
    pub header: VfoStateHeader,
    pub slot_a: [u8; 64],
    pub slot_b: [u8; 64],
}

const _: () = assert!(core::mem::size_of::<VfoStateFile>() == 160);

impl VfoStateFile {
    pub const fn empty() -> Self {
        Self {
            header: VfoStateHeader {
                ident: FILE_ID_VFO_STATE,
                version: 1,
                active_slot: 0,
                dual_watch: 0,
                cross_band: 0,
                save_epoch: 0,
                reserved: [0; 16],
                crc32: 0,
            },
            slot_a: [0; 64],
            slot_b: [0; 64],
        }
    }

    #[inline(always)]
    pub fn as_bytes(&self) -> &[u8; 160] {
        unsafe { &*(self as *const Self as *const [u8; 160]) }
    }

    /// Compute and store CRC-32 over header `[0..28]` + `slot_a` + `slot_b`.
    pub fn seal(&mut self) {
        self.header.ident = FILE_ID_VFO_STATE;
        let bytes = self.as_bytes();
        let mut crc = 0xFFFF_FFFFu32;
        crc = CrcEngine::crc32_update(crc, &bytes[..28]);
        crc = CrcEngine::crc32_update(crc, &bytes[32..160]);
        self.header.crc32 = !crc;
    }

    pub fn is_valid(&self) -> bool {
        if self.header.ident != FILE_ID_VFO_STATE {
            return false;
        }
        let bytes = self.as_bytes();
        let mut crc = 0xFFFF_FFFFu32;
        crc = CrcEngine::crc32_update(crc, &bytes[..28]);
        crc = CrcEngine::crc32_update(crc, &bytes[32..160]);
        self.header.crc32 == !crc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_offset_uses_power_of_two_shift() {
        assert_eq!(record_offset(0, 6), 16);
        assert_eq!(record_offset(1, 6), 80);
        assert_eq!(record_offset(10, 5), 336);
    }

    #[test]
    fn vfo_state_file_seals_and_detects_corruption() {
        let mut f = VfoStateFile::empty();
        f.header.active_slot = 1;
        f.header.save_epoch = 42;
        f.slot_a[0] = 0xAA;
        f.slot_b[63] = 0x55;
        assert!(!f.is_valid());
        f.seal();
        assert!(f.is_valid());
        f.slot_b[63] ^= 1;
        assert!(!f.is_valid());
    }
}
