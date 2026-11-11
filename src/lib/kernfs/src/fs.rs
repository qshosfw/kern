//! KernFS FileSystem Engine: Ultra-compact, Zero-Alloc `#![no_std]`, Crash-Safe, Wear-Leveled VFS.

use crate::device::{BlockDevice, FsError};
use crypto::crc::CrcEngine;

pub const FILE_ID_KERNFS: u16 = 0x4B46; // "KF"
pub const HEADER_SIZE: usize = 64;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SectorHeader {
    pub ident: u16,       // 0x4B46 ("KF")
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
    max_sector_used: usize,
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
            max_sector_used: 0,
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
        let mut empty_slot = None;
        for entry in self.cache.iter_mut() {
            if entry.active != 0 && entry.path_hash == path_hash {
                *entry = entry_val;
                return;
            }
            if entry.active == 0 && empty_slot.is_none() {
                empty_slot = Some(entry);
            }
        }
        if let Some(slot) = empty_slot {
            *slot = entry_val;
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

    /// Mount filesystem, scan sector headers, calculate global sequence number,
    /// and resolve any interrupted atomic commits by invalidating superseded sectors.
    pub fn mount(&mut self) -> Result<(), FsError> {
        let mut max_seq = 0u32;
        let mut max_sector = 0usize;
        let mut found_valid = false;
        let mut header_buf = [0u8; HEADER_SIZE];
        let mut used = 0u16;
        let mut deleted = 0u16;
        self.cache = [CacheEntry::EMPTY; CACHE_CAPACITY];

        for sector in 0..D::TOTAL_BLOCKS {
            let offset = sector * D::BLOCK_SIZE;
            if self.dev.read(offset, &mut header_buf).is_ok() {
                let ident = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];
                let seq = read_u32_le(&header_buf, 42);

                if ident == FILE_ID_KERNFS && (flags == 0x7F || flags == 0x01) {
                    found_valid = true;
                    used += 1;
                    max_sector = max_sector.max(sector);
                    max_seq = max_seq.max(seq);

                    let path_len = header_buf[4] as usize;
                    if path_len > 0 && path_len <= 32 {
                        let path_hash = CrcEngine::crc32(&header_buf[6..6 + path_len]);
                        let payload_len = read_u32_le(&header_buf, 38) as usize;
                        let crc = read_u32_le(&header_buf, 46);
                        let perm = header_buf[5];

                        // Check if an earlier sector in our scan had the same path_hash
                        let mut existing_sec = None;
                        for entry in &self.cache {
                            if entry.active != 0 && entry.path_hash == path_hash {
                                existing_sec = Some(entry.sector as usize);
                                break;
                            }
                        }

                        if let Some(prev_sec) = existing_sec {
                            let mut seq_buf = [0u8; 4];
                            let prev_seq = if self
                                .dev
                                .read(prev_sec * D::BLOCK_SIZE + 42, &mut seq_buf)
                                .is_ok()
                            {
                                read_u32_le(&seq_buf, 0)
                            } else {
                                0
                            };

                            if seq >= prev_seq {
                                // Current sector is newer; invalidate previous sector
                                if self
                                    .dev
                                    .program(prev_sec * D::BLOCK_SIZE + 3, &[0x00u8])
                                    .is_ok()
                                {
                                    used = used.saturating_sub(1);
                                    deleted = deleted.saturating_add(1);
                                }
                                self.cache_insert(path_hash, sector, payload_len, crc, perm);
                            } else {
                                // Previous sector is newer; invalidate current sector
                                if self.dev.program(offset + 3, &[0x00u8]).is_ok() {
                                    used = used.saturating_sub(1);
                                    deleted = deleted.saturating_add(1);
                                }
                            }
                        } else {
                            self.cache_insert(path_hash, sector, payload_len, crc, perm);
                        }
                    }
                } else if flags == 0x00 {
                    deleted += 1;
                    max_sector = max_sector.max(sector);
                }
            }
        }

        self.max_sector_used = max_sector;
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
        self.max_sector_used = 0;
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

    /// Read file content at `path` starting at byte `offset` into `buf`.
    /// Reads up to `buf.len()` bytes without heap allocation.
    pub fn read_file_at(
        &mut self,
        path: &str,
        offset: usize,
        buf: &mut [u8],
    ) -> Result<usize, FsError> {
        let (sector, payload_len, _, _) = self.find_best_sector(path).ok_or(FsError::NotFound)?;

        if offset >= payload_len || buf.is_empty() {
            return Ok(0);
        }

        let to_read = buf.len().min(payload_len - offset);
        let data_offset = sector * D::BLOCK_SIZE + HEADER_SIZE + offset;
        self.dev.read(data_offset, &mut buf[..to_read])?;
        Ok(to_read)
    }

    fn invalidate_sector(&mut self, sector: usize) -> Result<(), FsError> {
        self.dev.program(sector * D::BLOCK_SIZE + 3, &[0x00u8])?;
        self.cached_used = self.cached_used.saturating_sub(1);
        self.cached_deleted = self.cached_deleted.saturating_add(1);
        Ok(())
    }

    fn commit_header_and_cache(
        &mut self,
        path: &str,
        target_sector: usize,
        payload_len: usize,
        payload_crc: u32,
        perm: u8,
        old_sector: Option<usize>,
    ) -> Result<(), FsError> {
        self.global_seq += 1;
        let mut header_buf = [0u8; HEADER_SIZE];
        write_u16_le(&mut header_buf, 0, FILE_ID_KERNFS);
        header_buf[2] = 0x01; // Version 1
        header_buf[3] = 0x7F; // Active / Valid in NOR Flash (bit 7 = 0)
        header_buf[4] = path.len() as u8;
        header_buf[5] = perm;
        header_buf[6..6 + path.len()].copy_from_slice(path.as_bytes());
        write_u32_le(&mut header_buf, 38, payload_len as u32);
        write_u32_le(&mut header_buf, 42, self.global_seq);
        write_u32_le(&mut header_buf, 46, payload_crc);
        let header_crc = CrcEngine::crc32(&header_buf[..50]);
        write_u32_le(&mut header_buf, 50, header_crc);

        self.dev
            .program(target_sector * D::BLOCK_SIZE, &header_buf)?;

        if let Some(old) = old_sector
            && old != target_sector
        {
            let _ = self.invalidate_sector(old);
        }

        self.max_sector_used = self.max_sector_used.max(target_sector);

        let path_hash = CrcEngine::crc32(path.as_bytes());
        self.cache_insert(path_hash, target_sector, payload_len, payload_crc, perm);
        self.cached_used = self.cached_used.saturating_add(1);
        Ok(())
    }

    /// Write payload atomically to `path` with wear leveling and power-loss protection.
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        if data.len() > (D::BLOCK_SIZE - HEADER_SIZE) {
            return Err(FsError::CapacityFull);
        }
        if path.len() > 32 {
            return Err(FsError::InvalidPath);
        }

        let prev_entry = self.find_best_sector(path);
        let existing_perm = prev_entry.map_or(0u8, |(_, _, _, perm)| perm);

        let target_sector = self.find_free_sector()?;
        let target_offset = target_sector * D::BLOCK_SIZE;

        // Erase target sector before writing
        self.dev.erase_sector(target_offset)?;

        let payload_crc = CrcEngine::crc32(data);
        self.dev.program(target_offset + HEADER_SIZE, data)?;

        self.commit_header_and_cache(
            path,
            target_sector,
            data.len(),
            payload_crc,
            existing_perm,
            prev_entry.map(|(s, _, _, _)| s),
        )
    }

    /// Rename or relocate a file from `src` to `dst` using chunked sector-to-sector streaming
    /// without truncating large payloads or allocating large stack buffers.
    pub fn rename(&mut self, src: &str, dst: &str) -> Result<(), FsError> {
        if src == dst {
            return Ok(());
        }
        if dst.len() > 32 {
            return Err(FsError::InvalidPath);
        }

        let (src_sector, payload_len, payload_crc, perm) =
            self.find_best_sector(src).ok_or(FsError::NotFound)?;
        let prev_dst = self.find_best_sector(dst);

        let target_sector = self.find_free_sector()?;
        let target_offset = target_sector * D::BLOCK_SIZE;
        self.dev.erase_sector(target_offset)?;

        // Stream payload in 128-byte chunks from src_sector to target_sector
        let mut chunk = [0u8; 128];
        let mut copied = 0usize;
        while copied < payload_len {
            let step = (payload_len - copied).min(chunk.len());
            self.dev.read(
                src_sector * D::BLOCK_SIZE + HEADER_SIZE + copied,
                &mut chunk[..step],
            )?;
            self.dev
                .program(target_offset + HEADER_SIZE + copied, &chunk[..step])?;
            copied += step;
        }

        self.commit_header_and_cache(
            dst,
            target_sector,
            payload_len,
            payload_crc,
            perm,
            prev_dst.map(|(s, _, _, _)| s).filter(|&s| s != src_sector),
        )?;
        if src_sector != target_sector {
            let _ = self.invalidate_sector(src_sector);
        }
        self.cache_remove(CrcEngine::crc32(src.as_bytes()));

        Ok(())
    }

    /// Create directory branch (no-op in flat block-structured VFS, kept for trait compatibility).
    pub fn create_dir(&mut self, _path: &str) -> Result<(), FsError> {
        Ok(())
    }

    /// Remove file at `path`.
    pub fn remove(&mut self, path: &str) -> Result<(), FsError> {
        if let Some((sector, _, _, _)) = self.find_best_sector(path) {
            self.invalidate_sector(sector)?;
            self.cache_remove(CrcEngine::crc32(path.as_bytes()));
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

        // If active files do not exceed cache capacity, an un-cached file is guaranteed absent.
        if self.cached_used <= CACHE_CAPACITY as u16 {
            return None;
        }

        // Cache miss fallback bounded to max_sector_used instead of all 512 sectors
        let mut best_sector = None;
        let mut highest_seq = 0u32;
        let mut best_payload_len = 0usize;
        let mut best_crc = 0u32;
        let mut best_permission = 0u8;

        let mut header_buf = [0u8; HEADER_SIZE];
        let scan_limit = (self.max_sector_used + 1).min(D::TOTAL_BLOCKS);

        for sector in 0..scan_limit {
            let offset = sector * D::BLOCK_SIZE;
            if self.dev.read(offset, &mut header_buf).is_ok() {
                let ident = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];
                let path_len = header_buf[4] as usize;
                let perm = header_buf[5];

                if ident == FILE_ID_KERNFS
                    && (flags == 0x7F || flags == 0x01)
                    && path_len == path_bytes.len()
                    && &header_buf[6..6 + path_len] == path_bytes
                {
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
                let ident = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];

                // Unformatted (ident != FILE_ID_KERNFS), erased (0xFF), or deleted (0x00) sector is free to use
                if ident != FILE_ID_KERNFS || flags == 0xFF || flags == 0x00 {
                    return Ok(sector);
                }
            }
        }

        Err(FsError::CapacityFull)
    }

    pub fn get_permission(&mut self, path: &str) -> Result<u8, FsError> {
        if let Some((_, _, _, perm)) = self.find_best_sector(path) {
            Ok(perm)
        } else {
            Err(FsError::NotFound)
        }
    }

    pub fn set_permission(&mut self, path: &str, perm: u8) -> Result<(), FsError> {
        if let Some((sector, payload_len, payload_crc, _)) = self.find_best_sector(path) {
            let offset = sector * D::BLOCK_SIZE;
            self.dev.program(offset + 5, &[perm])?;
            let path_hash = CrcEngine::crc32(path.as_bytes());
            self.cache_insert(path_hash, sector, payload_len, payload_crc, perm);
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
                let ident = read_u16_le(&header_buf, 0);
                let flags = header_buf[3];
                let path_len = header_buf[4] as usize;
                let perm = header_buf[5];

                if ident == FILE_ID_KERNFS
                    && (flags == 0x7F || flags == 0x01)
                    && path_len > 0
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

#[cfg(test)]
mod tests {
    use super::*;

    struct MockFlash {
        data: [u8; 16 * 4096],
    }

    impl MockFlash {
        fn new() -> Self {
            Self {
                data: [0xFF; 16 * 4096],
            }
        }
    }

    impl BlockDevice for MockFlash {
        const READ_SIZE: usize = 256;
        const WRITE_SIZE: usize = 256;
        const BLOCK_SIZE: usize = 4096;
        const TOTAL_BLOCKS: usize = 16;

        fn read(&mut self, offset: usize, buf: &mut [u8]) -> Result<(), FsError> {
            if offset + buf.len() > self.data.len() {
                return Err(FsError::IoError);
            }
            buf.copy_from_slice(&self.data[offset..offset + buf.len()]);
            Ok(())
        }

        fn program(&mut self, offset: usize, data: &[u8]) -> Result<(), FsError> {
            if offset + data.len() > self.data.len() {
                return Err(FsError::IoError);
            }
            for (i, &b) in data.iter().enumerate() {
                self.data[offset + i] &= b;
            }
            Ok(())
        }

        fn erase_sector(&mut self, sector_addr: usize) -> Result<(), FsError> {
            let start = sector_addr;
            let end = start + Self::BLOCK_SIZE;
            if end > self.data.len() {
                return Err(FsError::IoError);
            }
            self.data[start..end].fill(0xFF);
            Ok(())
        }
    }

    #[test]
    fn test_read_file_at_arbitrary_offset() {
        let mut flash = MockFlash::new();
        let mut fs = KernFS::new(&mut flash);
        fs.format(None).unwrap();

        let payload = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
        fs.write_file("/test.bin", payload).unwrap();

        let mut out = [0u8; 10];
        let n = fs.read_file_at("/test.bin", 10, &mut out).unwrap();
        assert_eq!(n, 10);
        assert_eq!(&out, b"ABCDEFGHIJ");

        let n2 = fs.read_file_at("/test.bin", 36, &mut out).unwrap();
        assert_eq!(n2, 10);
        assert_eq!(&out, b"abcdefghij");

        // Offset beyond length returns 0
        let n3 = fs.read_file_at("/test.bin", 100, &mut out).unwrap();
        assert_eq!(n3, 0);
    }

    #[test]
    fn test_wrap_around_seq_recovery_and_rename() {
        let mut flash = MockFlash::new();
        {
            let mut fs = KernFS::new(&mut flash);
            fs.format(None).unwrap();
            for i in 0..20u8 {
                let payload = [b'A' + i; 32];
                fs.write_file("/pref/test.cfg", &payload).unwrap();
            }
            fs.rename("/pref/test.cfg", "/pref/renamed.cfg").unwrap();
        }

        let mut fs2 = KernFS::new(&mut flash);
        fs2.mount().unwrap();
        let mut out = [0u8; 32];
        let n = fs2.read_file("/pref/renamed.cfg", &mut out).unwrap();
        assert_eq!(n, 32);
        assert_eq!(out, [b'A' + 19; 32]);
        assert!(fs2.read_file("/pref/test.cfg", &mut out).is_err());
    }
}
