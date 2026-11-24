//! Microkernel Fixed-Size Power-of-Two Record Storage & Two-Tier VFO Persistence Contracts.
//!
//! Enforces $O(1)$ bit-shift record seeking (`offset = 16 + (index << shift)`),
//! sector-boundary alignment (4 KB LittleFS/KernFS blocks), and CRC32-sealed
//! 160-byte composite VFO snapshots (`/user/vfo_a.vfo`, `/user/vfo_b.vfo`).

use crypto::crc::CrcEngine;

/// Standard 16-byte table header size (`2^4`).
pub const TABLE_HEADER_SIZE: usize = 16;

/// Canonical FourCC table format identifiers (`AGENTS.md` §2).
pub const TABLE_ID_CHANNELS: [u8; 4] = *b"CHAN";
pub const TABLE_ID_CONTACTS: [u8; 4] = *b"CONT";
pub const TABLE_ID_BANDPLANS: [u8; 4] = *b"BNDP";
pub const TABLE_ID_ZONES: [u8; 4] = *b"ZONE";
pub const TABLE_ID_HITS: [u8; 4] = *b"HITS";
pub const TABLE_ID_FM: [u8; 4] = *b"FMBC";

/// FourCC format identifier for two-tier `.vfo` state files (`"VFOS"`).
pub const FILE_ID_VFO_STATE: [u8; 4] = *b"VFOS";

/// Naturally aligned 16-byte (`2^4`) structured table file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct TableHeader {
    pub ident: [u8; 4],
    pub version: u8,
    pub stride_shift: u8,
    pub flags: u16,
    pub capacity: u16,
    pub count: u16,
    pub crc32: u32,
}

const _: () = assert!(core::mem::size_of::<TableHeader>() == TABLE_HEADER_SIZE);

impl TableHeader {
    pub const fn new(ident: [u8; 4], stride_shift: u8, capacity: u16) -> Self {
        Self {
            ident,
            version: 1,
            stride_shift,
            flags: 0,
            capacity,
            count: 0,
            crc32: 0,
        }
    }

    #[inline(always)]
    pub const fn record_size(&self) -> usize {
        1usize << (self.stride_shift as u32)
    }

    #[inline(always)]
    pub const fn offset_of(&self, index: usize) -> Option<usize> {
        if index < self.capacity as usize {
            Some(record_offset(index, self.stride_shift as u32))
        } else {
            None
        }
    }

    pub fn seal(&mut self) {
        let bytes = unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, 12) };
        self.crc32 = CrcEngine::crc32(bytes);
    }

    pub fn is_valid(&self, expected_ident: [u8; 4]) -> bool {
        let bytes = unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, 12) };
        self.ident == expected_ident
            && self.stride_shift >= 4
            && self.stride_shift <= 9
            && self.crc32 == CrcEngine::crc32(bytes)
    }
}

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
    fn table_header_seals_and_computes_o1_offsets() {
        let mut hdr = TableHeader::new(TABLE_ID_CHANNELS, 6, 512);
        assert_eq!(hdr.record_size(), 64);
        assert_eq!(hdr.offset_of(0), Some(16));
        assert_eq!(hdr.offset_of(511), Some(16 + (511 << 6)));
        assert_eq!(hdr.offset_of(512), None);
        assert!(!hdr.is_valid(TABLE_ID_CHANNELS));
        hdr.seal();
        assert!(hdr.is_valid(TABLE_ID_CHANNELS));
        assert!(!hdr.is_valid(TABLE_ID_CONTACTS));
        hdr.count = 1;
        assert!(!hdr.is_valid(TABLE_ID_CHANNELS));
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
