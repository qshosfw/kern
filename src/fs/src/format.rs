//! KernFS v2 On-Flash Geometry, Superblock, Block Headers, Page Tags & Inodes.
//!
//! All structures use natural `#[repr(C)]` alignment with compile-time layout assertions
//! so Cortex-M0+ never executes unaligned loads or traps on zero-copy casts.

use crypto::crc::CrcEngine;

pub const FOURCC_SUPER_V2: u32 = 0x3253_464B; // "KFS2"
pub const FOURCC_BLOCK_V2: u32 = 0x4C42_324B; // "K2BL"
pub const FILE_ID_V1: u16 = 0x4B46; // "KF" (legacy KernFS v1 ident)

pub const PAGE_SIZE: usize = 256;
pub const PAGES_PER_BLOCK: usize = 16;
pub const DATA_PAGES_PER_BLOCK: usize = 15;
pub const SUPER_BLOCKS: usize = 2; // Blocks 0 and 1
pub const FIRST_DATA_BLOCK: usize = 2;
pub const MAX_BLOCKS: usize = 512;
pub const INODES_PER_PAGE: usize = 4;
pub const INDEX_PTRS_PER_PAGE: usize = 128;
#[cfg(target_arch = "arm")]
pub const MAX_META_GROUPS: usize = 32; // 32 * 4 = 128 inodes max on 2 MB SPI Flash (must be power of two)
#[cfg(not(target_arch = "arm"))]
pub const MAX_META_GROUPS: usize = 256; // 256 * 4 = 1,024 inodes max for host stress tests

const _: () = assert!(
    MAX_META_GROUPS.is_power_of_two(),
    "MAX_META_GROUPS must be a power of two for bitmask indexing"
);
pub const MAX_NAME_LEN: usize = 24;
pub const MAX_INLINE_LEN: usize = 16;
pub const RESERVE_BLOCKS: usize = 2;

pub const ROOT_INO: u16 = 1;

pub const BLOCK_KIND_ERASED: u8 = 0xFF;
pub const BLOCK_KIND_ACTIVE: u8 = 0x7F;
pub const BLOCK_KIND_BAD: u8 = 0x00;

pub const TAG_STATE_FREE: u8 = 0xFF;
pub const TAG_STATE_COMMITTED: u8 = 0x3F;
pub const TAG_STATE_OBSOLETE: u8 = 0x00;

pub const TAG_KIND_DATA: u8 = 1;
pub const TAG_KIND_INDEX1: u8 = 2;
pub const TAG_KIND_META: u8 = 4;

pub const INODE_FLAG_DIR: u8 = 1 << 0;
pub const INODE_FLAG_INLINE: u8 = 1 << 1;
pub const INODE_FLAG_RING: u8 = 1 << 2;

pub const DEPTH_NONE_OR_INLINE: u8 = 0;
pub const DEPTH_DIRECT: u8 = 1;
pub const DEPTH_SINGLE_INDIRECT: u8 = 2;

pub const BLOCK_TBL_BAD: u8 = 0xFF;
pub const BLOCK_TBL_ERASED: u8 = 0xF0;

/// Encode `(block, page_in_block)` into a compact 16-bit physical page ID.
#[inline(always)]
pub const fn make_phys_page(block: usize, page_in_block: usize) -> u16 {
    ((block << 4) | (page_in_block & 0x0F)) as u16
}

/// Split a physical page ID into `(block, page_in_block)`.
#[inline(always)]
pub const fn split_phys_page(phys_page: u16) -> (usize, usize) {
    (
        ((phys_page >> 4) as usize) & (MAX_BLOCKS - 1),
        (phys_page & 0x0F) as usize,
    )
}

/// Byte offset on flash for a physical page ID.
#[inline(always)]
pub const fn page_flash_offset(phys_page: u16) -> usize {
    (phys_page as usize) << 8
}

/// Byte offset of a `PageTag` in flash for `(block, page_in_block)` (page 1..=15).
#[inline(always)]
pub const fn tag_flash_offset(block: usize, page_in_block: usize) -> usize {
    (block << 12) | (page_in_block << 4)
}

/// Ping-pong Superblock (32 bytes, stored in Block 0 and Block 1).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SuperblockV2 {
    pub fourcc: u32,
    pub version: u8,
    pub flags: u8,
    pub total_blocks: u16,
    pub format_epoch: u32,
    pub seq: u32,
    pub global_gen: u32,
    pub next_ino: u16,
    pub block_seq: u16,
    pub root_meta_hint: u16,
    pub reserved: u16,
    pub crc32: u32,
}

impl SuperblockV2 {
    pub fn to_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, 32) }
    }

    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < 32 {
            return None;
        }
        let mut sb = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), sb.as_mut_ptr() as *mut u8, 32);
            let sb = sb.assume_init();
            if sb.fourcc == FOURCC_SUPER_V2
                && sb.version == 2
                && CrcEngine::crc32(&buf[..28]) == sb.crc32
            {
                Some(sb)
            } else {
                None
            }
        }
    }

    pub fn finalize(&mut self) {
        self.crc32 = 0;
        let bytes = self.to_bytes();
        self.crc32 = CrcEngine::crc32(&bytes[..28]);
    }
}

/// Per-block header stored at bytes `0..16` of Page 0 in Blocks `2..TOTAL_BLOCKS`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BlockHeader {
    pub fourcc: u32,
    pub format_epoch: u32,
    pub erase_count: u32,
    pub block_seq: u16,
    pub kind: u8,
    pub reserved: u8,
}

impl BlockHeader {
    pub const fn new(format_epoch: u32, erase_count: u32, block_seq: u16, kind: u8) -> Self {
        Self {
            fourcc: FOURCC_BLOCK_V2,
            format_epoch,
            erase_count,
            block_seq,
            kind,
            reserved: 0xFF,
        }
    }

    pub fn from_bytes(buf: &[u8]) -> Self {
        let mut hdr = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), hdr.as_mut_ptr() as *mut u8, 16);
            hdr.assume_init()
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, 16) }
    }
}

/// Per-page reverse-map tag (16 bytes; 15 tags follow `BlockHeader` in Page 0).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PageTag {
    pub ino: u16,
    pub page_idx: u16,
    pub generation: u32,
    pub state: u8,
    pub kind: u8,
    pub _pad: u16,
    pub crc32: u32,
}

impl PageTag {
    pub const EMPTY: Self = Self {
        ino: 0,
        page_idx: 0,
        generation: 0,
        state: 0,
        kind: 0,
        _pad: 0,
        crc32: 0,
    };

    pub fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, 16) }
    }

    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self as *mut Self as *mut u8, 16) }
    }
}

/// 64-byte Inode (4 per 256-byte metadata page).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Inode {
    pub perm: u8,
    pub flags: u8,
    pub depth: u8,
    pub name_len: u8,
    pub record_shift: u8,
    pub owner_pkg: u8,
    pub ino: u16,
    pub parent_ino: u16,
    pub root_page: u16,
    pub size: u32,
    pub generation: u32,
    pub type_fourcc: u32,
    pub name: [u8; MAX_NAME_LEN],
    pub inline_data: [u8; MAX_INLINE_LEN],
}

impl Inode {
    pub const EMPTY: Self = Self {
        ino: 0,
        parent_ino: 0,
        size: 0,
        generation: 0,
        root_page: 0,
        perm: 0,
        flags: 0,
        depth: 0,
        record_shift: 0,
        name_len: 0,
        owner_pkg: 0,
        type_fourcc: 0,
        name: [0; MAX_NAME_LEN],
        inline_data: [0; MAX_INLINE_LEN],
    };

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.ino >= 2
    }

    #[inline(always)]
    pub const fn is_dir(&self) -> bool {
        (self.flags & INODE_FLAG_DIR) != 0
    }

    #[inline(always)]
    pub const fn is_inline(&self) -> bool {
        (self.flags & INODE_FLAG_INLINE) != 0
    }

    #[inline(always)]
    pub fn l1_page(&self, idx: usize) -> u16 {
        if idx >= 8 {
            return 0;
        }
        u16::from_le_bytes([self.inline_data[idx * 2], self.inline_data[idx * 2 + 1]])
    }

    #[inline(always)]
    pub fn set_l1_page(&mut self, idx: usize, page: u16) {
        if idx < 8 {
            let b = page.to_le_bytes();
            self.inline_data[idx * 2] = b[0];
            self.inline_data[idx * 2 + 1] = b[1];
        }
    }

    #[inline(always)]
    pub fn name_bytes(&self) -> &[u8] {
        let l = (self.name_len as usize).min(MAX_NAME_LEN);
        &self.name[..l]
    }

    pub fn name_str(&self) -> &str {
        let b = self.name_bytes();
        if b.iter().all(|&c| c < 0x80) {
            unsafe { core::str::from_utf8_unchecked(b) }
        } else {
            ""
        }
    }

    pub fn set_name(&mut self, b: &[u8]) {
        let l = b.len().min(MAX_NAME_LEN);
        self.name = [0; MAX_NAME_LEN];
        self.name[..l].copy_from_slice(&b[..l]);
        self.name_len = l as u8;
    }

    #[inline(always)]
    pub fn slot_is_free(page_buf: &[u8; PAGE_SIZE], slot: usize) -> bool {
        let off = (slot & 3) * 64 + core::mem::offset_of!(Self, ino);
        u16::from_le_bytes([page_buf[off], page_buf[off + 1]]) < 2
    }

    pub fn read_slot_into(page_buf: &[u8; PAGE_SIZE], slot: usize, out: &mut Self) {
        unsafe {
            core::ptr::copy_nonoverlapping(
                page_buf.as_ptr().add((slot & 3) * 64),
                out as *mut Self as *mut u8,
                64,
            );
        }
    }

    pub fn write_slot(&self, page_buf: &mut [u8; PAGE_SIZE], slot: usize) {
        unsafe {
            core::ptr::copy_nonoverlapping(
                self as *const Self as *const u8,
                page_buf.as_mut_ptr().add((slot & 3) * 64),
                64,
            );
        }
    }
}

/// Checks if a 64-byte raw sector header belongs to legacy KernFS v1.
pub fn is_valid_v1_header(header_buf: &[u8]) -> bool {
    if header_buf.len() < 64 {
        return false;
    }
    let ident = u16::from_le_bytes([header_buf[0], header_buf[1]]);
    let ver = header_buf[2];
    let flags = header_buf[3];
    let path_len = header_buf[4] as usize;
    let payload_len = u32::from_le_bytes([
        header_buf[38],
        header_buf[39],
        header_buf[40],
        header_buf[41],
    ]) as usize;
    let stored_crc = u32::from_le_bytes([
        header_buf[50],
        header_buf[51],
        header_buf[52],
        header_buf[53],
    ]);
    ident == FILE_ID_V1
        && ver == 1
        && (flags == 0x7F || flags == 0x01)
        && path_len > 0
        && path_len <= 32
        && payload_len <= (4096 - 64)
        && CrcEngine::crc32(&header_buf[..50]) == stored_crc
}

/// Read an index entry (`u16` physical page ID) from a 256-byte index page.
#[inline(always)]
pub fn read_idx_u16(page_buf: &[u8; PAGE_SIZE], idx: usize) -> u16 {
    let off = (idx & 0x7F) * 2;
    u16::from_le_bytes([page_buf[off], page_buf[off + 1]])
}

/// Write an index entry (`u16` physical page ID) into a 256-byte index page.
#[inline(always)]
pub fn write_idx_u16(page_buf: &mut [u8; PAGE_SIZE], idx: usize, val: u16) {
    let off = (idx & 0x7F) * 2;
    let b = val.to_le_bytes();
    page_buf[off] = b[0];
    page_buf[off + 1] = b[1];
}

const _: () = {
    assert!(core::mem::size_of::<SuperblockV2>() == 32);
    assert!(core::mem::align_of::<SuperblockV2>() == 4);
    assert!(core::mem::size_of::<BlockHeader>() == 16);
    assert!(core::mem::align_of::<BlockHeader>() == 4);
    assert!(core::mem::size_of::<PageTag>() == 16);
    assert!(core::mem::align_of::<PageTag>() == 4);
    assert!(core::mem::size_of::<Inode>() == 64);
    assert!(core::mem::align_of::<Inode>() == 4);
};
