//! KernFS FileSystem Engine: Ultra-compact, Zero-Alloc `#![no_std]`, Crash-Safe, Wear-Leveled VFS.

use crate::device::{BlockDevice, FsError};
use crypto::crc::CrcEngine;

pub const MAGIC_KERNFS: u16 = 0x4B46; // "KF"
pub const HEADER_SIZE: usize = 64;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SectorHeader {
    pub magic: u16,       // 0x4B46
    pub version: u8,      // 0x01
    pub flags: u8,        // 0x01: Active, 0x02: Encrypted, 0x04: Compressed, 0x80: Deleted
    pub path_len: u8,     // Length of path string (max 32)
    pub permission: u8,   // File permission bitmask
    pub path: [u8; 32],   // Path string (e.g. "/sys/calib.dat")
    pub payload_len: u32, // Payload size in bytes
    pub seq_num: u32,     // Sequence counter for atomic commits
    pub payload_crc: u32, // CRC32 checksum over payload bytes
    pub header_crc: u32,  // CRC32 checksum over header fields
}

#[inline(always)]
fn read_u16_le(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

fn read_u32_le(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        buf[offset],
        buf[offset + 1],
        buf[offset + 2],
        buf[offset + 3],
    ])
}

#[inline(always)]
fn write_u16_le(buf: &mut [u8], offset: usize, val: u16) {
    let b = val.to_le_bytes();
    buf[offset] = b[0];
    buf[offset + 1] = b[1];
}

fn write_u32_le(buf: &mut [u8], offset: usize, val: u32) {
    let b = val.to_le_bytes();
    buf[offset] = b[0];
    buf[offset + 1] = b[1];
    buf[offset + 2] = b[2];
    buf[offset + 3] = b[3];
}

#[derive(Clone, Copy)]
struct CacheEntry {
    path_hash: u32,
    payload_crc: u32,
    sector: u16,
    payload_len: u16,
    perm: u8,
    active: u8,
}

impl CacheEntry {
    const EMPTY: Self = Self {
        path_hash: 0,
        payload_crc: 0,
        sector: 0,
        payload_len: 0,
        perm: 0,
        active: 0,
    };
}

const CACHE_CAPACITY: usize = 24;

pub struct KernFS<'a, D: BlockDevice> {
    pub dev: &'a mut D,
    next_alloc_sector: usize,
    global_seq: u32,
    // Zero-alloc O(1) cache for active files (16B per entry)
    cache: [CacheEntry; CACHE_CAPACITY],
    cached_used: u16,
    cached_deleted: u16,
}

impl<'a, D: BlockDevice> KernFS<'a, D> {
    pub fn new(dev: &'a mut D) -> Self {
        Self {
            dev,
            next_alloc_sector: 0,
            global_seq: 1,
            cache: [CacheEntry::EMPTY; CACHE_CAPACITY],
            cached_used: 0,
            cached_deleted: 0,
        }
    }

    fn cache_insert(&mut self, path_hash: u32, sector: usize, len: usize, crc: u32, perm: u8) {
        let entry_val = CacheEntry {
            path_hash,
            payload_crc: crc,
            sector: sector as u16,
            payload_len: len as u16,
            perm,
            active: 1,
        };
        for entry in self.cache.iter_mut() {
            if entry.active != 0 && entry.path_hash == path_hash {
                *entry = entry_val;
                return;
            }
        }
        for entry in self.cache.iter_mut() {
            if entry.active == 0 {
                *entry = entry_val;
                return;
            }
        }
    }

    fn cache_remove(&mut self, path_hash: u32) {
        for entry in self.cache.iter_mut() {
            if entry.active != 0 && entry.path_hash == path_hash {
                *entry = CacheEntry::EMPTY;
                break;
            }
        }
    }

    /// Mount filesystem, scan sector headers, calculate global sequence number.
    pub fn mount(&mut self) -> Result<(), FsError> {
        let mut max_seq = 0u32;
        let mut found_valid = false;
        let mut header_buf = [0u8; HEADER_SIZE];
        let mut used = 0u16;
        let mut deleted = 0u16;

        for sector in 0..D::TOTAL_BLOCKS {
            let offset = sector * D::BLOCK_SIZE;
            if self.dev.read(offset, &mut header_buf).is_ok() {
                let magic = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];
                let seq = read_u32_le(&header_buf, 42);

                if magic == MAGIC_KERNFS && (flags == 0x7F || flags == 0x01) {
                    found_valid = true;
                    used += 1;
                    if seq > max_seq {
                        max_seq = seq;
                    }

                    let path_len = header_buf[4] as usize;
                    if path_len > 0 && path_len <= 32 {
                        let path_hash = CrcEngine::crc32(&header_buf[6..6 + path_len]);
                        let payload_len = read_u32_le(&header_buf, 38) as usize;
                        let crc = read_u32_le(&header_buf, 46);
                        let perm = header_buf[5];

                        self.cache_insert(path_hash, sector, payload_len, crc, perm);
                    }
                } else if flags == 0x00 {
                    deleted += 1;
                }
            }
        }

        self.cached_used = used;
        self.cached_deleted = deleted;
        self.global_seq = max_seq + 1;
        if found_valid {
            Ok(())
        } else {
            Err(FsError::NotFound)
        }
    }

    /// Format active filesystem blocks (fast 16-sector format for instant boot turnaround).
    pub fn format(&mut self, progress_cb: Option<&mut dyn FnMut(u8)>) -> Result<(), FsError> {
        let total = 16;
        let mut cb = progress_cb;

        for sector in 0..total {
            let addr = sector * D::BLOCK_SIZE;
            self.dev.erase_sector(addr)?;

            if let Some(ref mut f) = cb {
                let percent = (((sector + 1) * 100) / total) as u8;
                f(percent);
            }
        }

        self.next_alloc_sector = 0;
        self.global_seq = 1;
        self.cached_used = 0;
        self.cached_deleted = 0;
        self.cache = [CacheEntry::EMPTY; CACHE_CAPACITY];
        Ok(())
    }

    /// Check if a path exists.
    pub fn exists(&mut self, path: &str) -> bool {
        self.find_best_sector(path).is_some()
    }

    /// Query exact file payload size in bytes.
    pub fn file_size(&mut self, path: &str) -> Result<usize, FsError> {
        let (_, payload_len, _, _) = self.find_best_sector(path).ok_or(FsError::NotFound)?;
        Ok(payload_len)
    }

    /// Read file content at `path` into `buf`.
    /// Reads up to `buf.len()` bytes.
    /// If `buf.len() >= payload_len`, verifies payload CRC32 and returns `Ok(payload_len)`.
    /// If `buf.len() < payload_len`, reads the first `buf.len()` bytes and returns `Ok(buf.len())`.
    pub fn read_file(&mut self, path: &str, buf: &mut [u8]) -> Result<usize, FsError> {
        let (sector, payload_len, payload_crc, _) =
            self.find_best_sector(path).ok_or(FsError::NotFound)?;

        let to_read = buf.len().min(payload_len);
        if to_read == 0 {
            return Ok(0);
        }

        let data_offset = sector * D::BLOCK_SIZE + HEADER_SIZE;
        self.dev.read(data_offset, &mut buf[..to_read])?;

        // If a full payload was read into buf, verify CRC32 checksum
        if buf.len() >= payload_len {
            let computed_crc = CrcEngine::crc32(&buf[..payload_len]);
            if computed_crc != payload_crc {
                return Err(FsError::CorruptedData);
            }
        }

        Ok(to_read)
    }

    /// Write payload atomically to `path` with wear leveling and power-loss protection.
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        if data.len() > (D::BLOCK_SIZE - HEADER_SIZE) {
            return Err(FsError::CapacityFull);
        }
        if path.len() > 32 {
            return Err(FsError::InvalidPath);
        }

        let mut existing_perm = 0u8;
        if let Some((_, _, _, perm)) = self.find_best_sector(path) {
            existing_perm = perm;
        }

        let target_sector = self.find_free_sector()?;
        let target_offset = target_sector * D::BLOCK_SIZE;

        // Erase target sector before writing
        self.dev.erase_sector(target_offset)?;

        // Prepare Header
        let payload_crc = CrcEngine::crc32(data);
        self.global_seq += 1;

        let mut header_buf = [0u8; HEADER_SIZE];
        write_u16_le(&mut header_buf, 0, MAGIC_KERNFS);
        header_buf[2] = 0x01; // Version 1
        header_buf[3] = 0x7F; // Active / Valid in NOR Flash (bit 7 = 0)
        header_buf[4] = path.len() as u8;
        header_buf[5] = existing_perm;

        // Copy path bytes
        header_buf[6..6 + path.len()].copy_from_slice(path.as_bytes());

        // Copy payload len & seq_num
        write_u32_le(&mut header_buf, 38, data.len() as u32);
        write_u32_le(&mut header_buf, 42, self.global_seq);
        write_u32_le(&mut header_buf, 46, payload_crc);

        // Header CRC32
        let header_crc = CrcEngine::crc32(&header_buf[..50]);
        write_u32_le(&mut header_buf, 50, header_crc);

        // 1. Program payload data first
        self.dev.program(target_offset + HEADER_SIZE, data)?;

        // 2. Program sector header atomically
        self.dev.program(target_offset, &header_buf)?;

        // 3. Invalidate old sectors matching path (if any)
        self.mark_old_sectors_deleted(path, target_sector);

        // 4. Update Cache
        let path_hash = CrcEngine::crc32(path.as_bytes());
        self.cache_insert(
            path_hash,
            target_sector,
            data.len(),
            payload_crc,
            existing_perm,
        );
        self.cached_used = self.cached_used.saturating_add(1);

        Ok(())
    }

    /// Create directory branch (no-op in flat block-structured VFS, kept for trait compatibility).
    pub fn create_dir(&mut self, _path: &str) -> Result<(), FsError> {
        Ok(())
    }

    /// Remove file at `path`.
    pub fn remove(&mut self, path: &str) -> Result<(), FsError> {
        if let Some((sector, _, _, _)) = self.find_best_sector(path) {
            let offset = sector * D::BLOCK_SIZE;
            let flag = [0x00u8]; // Mark Deleted (all bits 0)
            self.dev.program(offset + 3, &flag)?;

            let path_hash = CrcEngine::crc32(path.as_bytes());
            self.cache_remove(path_hash);
            self.cached_used = self.cached_used.saturating_sub(1);
            self.cached_deleted = self.cached_deleted.saturating_add(1);

            Ok(())
        } else {
            Err(FsError::NotFound)
        }
    }

    /// Find sector with highest sequence number matching `path`.
    fn find_best_sector(&mut self, path: &str) -> Option<(usize, usize, u32, u8)> {
        let path_bytes = path.as_bytes();
        let path_hash = CrcEngine::crc32(path_bytes);

        // O(1) Cache fast-path
        for entry in &self.cache {
            if entry.active != 0 && entry.path_hash == path_hash {
                return Some((
                    entry.sector as usize,
                    entry.payload_len as usize,
                    entry.payload_crc,
                    entry.perm,
                ));
            }
        }

        // Cache miss fallback (should only happen if cache > 32 entries)
        let mut best_sector = None;
        let mut highest_seq = 0u32;
        let mut best_payload_len = 0usize;
        let mut best_crc = 0u32;
        let mut best_permission = 0u8;

        let mut header_buf = [0u8; HEADER_SIZE];

        for sector in 0..D::TOTAL_BLOCKS {
            let offset = sector * D::BLOCK_SIZE;
            if self.dev.read(offset, &mut header_buf).is_ok() {
                let magic = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];
                let path_len = header_buf[4] as usize;
                let perm = header_buf[5];

                if magic == MAGIC_KERNFS && (flags == 0x7F || flags == 0x01) {
                    if path_len == path_bytes.len() && &header_buf[6..6 + path_len] == path_bytes {
                        let seq = read_u32_le(&header_buf, 42);
                        if seq > highest_seq {
                            highest_seq = seq;
                            best_sector = Some(sector);
                            best_payload_len = read_u32_le(&header_buf, 38) as usize;
                            best_crc = read_u32_le(&header_buf, 46);
                            best_permission = perm;
                        }
                    }
                }
            }
        }

        // Populate cache on miss
        if let Some(s) = best_sector {
            self.cache_insert(path_hash, s, best_payload_len, best_crc, best_permission);
        }

        best_sector.map(|s| (s, best_payload_len, best_crc, best_permission))
    }

    /// Find next clean/free sector using round-robin wear leveling.
    fn find_free_sector(&mut self) -> Result<usize, FsError> {
        let mut header_buf = [0u8; HEADER_SIZE];

        for _ in 0..D::TOTAL_BLOCKS {
            let sector = self.next_alloc_sector;
            self.next_alloc_sector = (self.next_alloc_sector + 1) & (D::TOTAL_BLOCKS - 1);

            let offset = sector * D::BLOCK_SIZE;
            if self.dev.read(offset, &mut header_buf).is_ok() {
                let magic = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];

                // Unformatted (magic != MAGIC_KERNFS), erased (0xFF), or deleted (0x00) sector is free to use
                if magic != MAGIC_KERNFS || flags == 0xFF || flags == 0x00 {
                    return Ok(sector);
                }
            }
        }

        Err(FsError::CapacityFull)
    }

    /// Mark older versions of file as deleted.
    fn mark_old_sectors_deleted(&mut self, path: &str, current_sector: usize) {
        let mut header_buf = [0u8; HEADER_SIZE];
        let path_bytes = path.as_bytes();

        for sector in 0..D::TOTAL_BLOCKS {
            if sector == current_sector {
                continue;
            }
            let offset = sector * D::BLOCK_SIZE;
            if self.dev.read(offset, &mut header_buf).is_ok() {
                let magic = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];
                let path_len = header_buf[4] as usize;

                if magic == MAGIC_KERNFS && (flags == 0x7F || flags == 0x01) {
                    if path_len == path_bytes.len() && &header_buf[6..6 + path_len] == path_bytes {
                        if self.dev.program(offset + 3, &[0x00u8]).is_ok() {
                            self.cached_used = self.cached_used.saturating_sub(1);
                            self.cached_deleted = self.cached_deleted.saturating_add(1);
                        }
                    }
                }
            }
        }
    }

    pub fn get_permission(&mut self, path: &str) -> Result<u8, FsError> {
        if let Some((_, _, _, perm)) = self.find_best_sector(path) {
            Ok(perm)
        } else {
            Err(FsError::NotFound)
        }
    }

    pub fn set_permission(&mut self, path: &str, perm: u8) -> Result<(), FsError> {
        if let Some((sector, _, _, _)) = self.find_best_sector(path) {
            let offset = sector * D::BLOCK_SIZE;
            self.dev.program(offset + 5, &[perm])?;
            Ok(())
        } else {
            Err(FsError::NotFound)
        }
    }

    /// List files/directories under `dir_path` by performing a single-pass
    /// Returns only immediate children — nested paths are collapsed to the first child component.
    pub fn list_dir<F: FnMut(&str, usize, u8, bool)>(
        &mut self,
        dir_path: &str,
        mut cb: F,
    ) -> Result<usize, FsError> {
        let mut count = 0;
        let mut header_buf = [0u8; HEADER_SIZE];
        let mut end = dir_path.len();
        while end > 0 && dir_path.as_bytes()[end - 1] == b'/' {
            end -= 1;
        }
        let target_dir = &dir_path[..end];

        // Single pass over sectors (no nested find_best_sector loops)
        for sector in 0..D::TOTAL_BLOCKS {
            let offset = sector * D::BLOCK_SIZE;
            if self.dev.read(offset, &mut header_buf).is_ok() {
                let magic = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];
                let path_len = header_buf[4] as usize;
                let perm = header_buf[5];

                if magic == MAGIC_KERNFS && (flags == 0x7F || flags == 0x01) {
                    if path_len > 0
                        && path_len <= 32
                        && header_buf[6..6 + path_len].iter().all(|&b| b < 128)
                    {
                        let full_path =
                            unsafe { core::str::from_utf8_unchecked(&header_buf[6..6 + path_len]) };
                        let payload_len = read_u32_le(&header_buf, 38) as usize;

                        // Determine the child portion relative to target_dir
                        let child_start = if target_dir.is_empty() || target_dir == "/" {
                            // Root listing: child starts after the leading '/'
                            if full_path.as_bytes().first() == Some(&b'/') {
                                1
                            } else {
                                0
                            }
                        } else {
                            let fp_bytes = full_path.as_bytes();
                            let td_bytes = target_dir.as_bytes();
                            if fp_bytes.starts_with(td_bytes)
                                && fp_bytes.get(td_bytes.len()) == Some(&b'/')
                            {
                                td_bytes.len() + 1 // skip past target_dir + '/'
                            } else {
                                continue; // not under this directory
                            }
                        };

                        if child_start >= full_path.len() {
                            continue; // exact match on dir itself, skip
                        }

                        // Extract immediate child: truncate at the next '/' if any
                        let rest = &full_path.as_bytes()[child_start..];
                        let (entry_end, is_dir) =
                            if let Some(slash_pos) = rest.iter().position(|&b| b == b'/') {
                                (child_start + slash_pos, true) // subdirectory — truncate before the slash
                            } else {
                                (full_path.len(), false) // leaf file — use full path
                            };

                        let entry_path = &full_path[..entry_end];
                        cb(entry_path, payload_len, perm, is_dir);
                        count += 1;
                    }
                }
            }
        }

        Ok(count)
    }

    /// Query dynamic filesystem wear leveling and storage statistics in O(1) time without SPI reads.
    pub fn stats(&self) -> FsStats {
        let mut active = 0u16;
        for entry in &self.cache {
            if entry.active != 0 {
                active += 1;
            }
        }
        let total = D::TOTAL_BLOCKS as u16;
        let used = self.cached_used;
        let deleted = self.cached_deleted;
        let free = total.saturating_sub(used.saturating_add(deleted));
        FsStats {
            active_files: active,
            used_sectors: used,
            free_sectors: free,
            deleted_sectors: deleted,
            wear_cycles: self.global_seq,
            total_sectors: total,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FsStats {
    pub active_files: u16,
    pub used_sectors: u16,
    pub free_sectors: u16,
    pub deleted_sectors: u16,
    pub wear_cycles: u32,
    pub total_sectors: u16,
}
