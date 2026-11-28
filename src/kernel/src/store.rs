//! Microkernel Fixed-Size Power-of-Two Record Storage Contracts.
//!
//! Enforces $O(1)$ bit-shift record seeking (`offset = 16 + (index << shift)`)
//! and sector-boundary alignment (4 KB KernFS blocks).

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
}
