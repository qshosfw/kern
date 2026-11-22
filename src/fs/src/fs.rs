//! KernFS v2: 256-Byte Page-Granular, CoW, Power-Loss-Safe Flash Filesystem.

use crate::device::{BlockDevice, FsError};
use crate::format::*;
use crypto::crc::CrcEngine;

#[derive(Clone, Copy)]
pub(crate) enum MetaQuery<'a> {
    Child(u16, &'a [u8]),
    FreeSlot,
    ListDir(u16),
    CountFiles,
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

impl FsStats {
    /// Total sectors available for new writes: currently erased plus GC-reclaimable obsolete sectors.
    #[inline(always)]
    pub const fn avail_sectors(&self) -> u16 {
        self.free_sectors.saturating_add(self.deleted_sectors)
    }

    /// Approximate usable payload bytes available for new writes (`15 * 256 = 3,840 B` per sector).
    #[inline(always)]
    pub const fn avail_bytes(&self) -> usize {
        (self.avail_sectors() as usize) * (DATA_PAGES_PER_BLOCK * PAGE_SIZE)
    }
}

#[repr(C)]
pub struct KernFS<'a, D: BlockDevice> {
    // Keep hot 1-byte/2-byte flags and counters within Thumb-1 imm5 offsets (<= 31 / <= 62).
    pub(crate) cur_alloc_page: u8,
    pub(crate) low_power: bool,
    pub(crate) mounted: bool,
    pub(crate) in_gc: bool,
    pub(crate) block_seq: u16,
    pub(crate) cur_alloc_block: u16,
    pub(crate) hot_ino: u16,
    pub(crate) active_files: u16,
    pub dev: &'a mut D,
    pub(crate) format_epoch: u32,
    pub(crate) super_seq: u32,
    pub(crate) global_gen: u32,
    pub(crate) total_erases: u32,
    pub(crate) hot_hash: u32,
    pub(crate) cur_tag: PageTag,
    pub(crate) cur_inode: Inode,
    pub(crate) meta_pages: [u16; MAX_META_GROUPS],
    pub(crate) block_table: [u8; MAX_BLOCKS],
    pub(crate) page_buf: [u8; PAGE_SIZE],
}

impl<'a, D: BlockDevice> KernFS<'a, D> {
    /// Initialize a `KernFS` directly in uninitialized storage to avoid large
    /// by-value stack copy temporaries during firmware startup.
    pub unsafe fn init_in_place(dst: *mut Self, dev: &'a mut D) {
        unsafe {
            core::ptr::write(core::ptr::addr_of_mut!((*dst).cur_alloc_page), 1);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).low_power), false);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).mounted), false);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).in_gc), false);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).block_seq), 1);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).cur_alloc_block), 0);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).hot_ino), 0);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).active_files), 0);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).dev), dev);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).format_epoch), 1);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).super_seq), 1);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).global_gen), 1);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).total_erases), 0);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).hot_hash), 0);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).cur_tag), PageTag::EMPTY);
            core::ptr::write(core::ptr::addr_of_mut!((*dst).cur_inode), Inode::EMPTY);
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*dst).meta_pages) as *mut u8,
                0,
                MAX_META_GROUPS * 2,
            );
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*dst).block_table) as *mut u8,
                BLOCK_TBL_ERASED,
                MAX_BLOCKS,
            );
            core::ptr::write_bytes(
                core::ptr::addr_of_mut!((*dst).page_buf) as *mut u8,
                0,
                PAGE_SIZE,
            );
        }
    }

    pub fn new(dev: &'a mut D) -> Self {
        let mut fs = core::mem::MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_in_place(fs.as_mut_ptr(), dev);
            fs.assume_init()
        }
    }

    #[inline(never)]
    pub(crate) fn reset_tables(&mut self) {
        self.hot_hash = 0;
        self.hot_ino = 0;
        self.active_files = 0;
        self.meta_pages.fill(0);
        self.block_table.fill(BLOCK_TBL_ERASED);
    }

    /// Enable or clear the brown-out guard (blocks erases/writes below safe voltage).
    pub fn set_low_power(&mut self, low_power: bool) {
        self.low_power = low_power;
    }

    #[inline(never)]
    pub(crate) fn page_counts(&self) -> (u16, u16, u16) {
        let mut free_pages = 0u16;
        let mut obs_pages = 0u16;
        let mut bad_blocks = 0u16;
        for b in FIRST_DATA_BLOCK..D::TOTAL_BLOCKS {
            let v = self.block_table[b];
            if v == BLOCK_TBL_BAD {
                bad_blocks += 1;
            } else {
                free_pages += (v >> 4) as u16;
                obs_pages += (v & 0x0F) as u16;
            }
        }
        (free_pages, obs_pages, bad_blocks)
    }

    #[inline(never)]
    pub fn stats(&self) -> FsStats {
        let total = D::TOTAL_BLOCKS as u16;
        let (free_pages, obs_pages, bad_blocks) = self.page_counts();
        let usable_blocks = (D::TOTAL_BLOCKS - FIRST_DATA_BLOCK) as u16 - bad_blocks;
        let free_sectors = (((free_pages as u32) * 4369 + 4368) >> 16) as u16;
        let deleted_sectors = (((obs_pages as u32) * 4369 + 4368) >> 16) as u16;
        let used_sectors =
            usable_blocks.saturating_sub(free_sectors.saturating_add(deleted_sectors));
        FsStats {
            active_files: self.active_files,
            used_sectors,
            free_sectors,
            deleted_sectors,
            wear_cycles: self.total_erases.max(self.global_gen),
            total_sectors: total,
        }
    }

    #[inline(never)]
    pub(crate) fn read_tag(&mut self, phys_page: u16) -> Result<(), FsError> {
        let (block, slot) = split_phys_page(phys_page);
        if block < FIRST_DATA_BLOCK || block >= D::TOTAL_BLOCKS || slot == 0 {
            return Err(FsError::CorruptedData);
        }
        self.dev
            .read(tag_flash_offset(block, slot), self.cur_tag.as_bytes_mut())
    }

    #[inline(never)]
    pub(crate) fn load_page(&mut self, phys_page: u16) -> Result<(), FsError> {
        self.read_tag(phys_page)?;
        self.dev
            .read(page_flash_offset(phys_page), &mut self.page_buf)?;
        if CrcEngine::crc32(&self.page_buf) != self.cur_tag.crc32 {
            return Err(FsError::CorruptedData);
        }
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn load_page_or_zero(&mut self, phys: u16) -> Result<(), FsError> {
        if phys != 0 {
            self.load_page(phys)
        } else {
            self.page_buf.fill(0);
            Ok(())
        }
    }

    #[inline(never)]
    pub(crate) fn program_page_buf(
        &mut self,
        phys: u16,
        ino: u16,
        page_idx: u16,
        kind: u8,
    ) -> Result<u16, FsError> {
        let (block, slot) = split_phys_page(phys);
        self.global_gen = self.global_gen.wrapping_add(1);
        self.cur_tag = PageTag {
            ino,
            page_idx,
            generation: self.global_gen,
            state: TAG_STATE_COMMITTED,
            kind,
            _pad: 0xFFFF,
            crc32: CrcEngine::crc32(&self.page_buf),
        };
        if self
            .dev
            .program(page_flash_offset(phys), &self.page_buf)
            .is_err()
            || self
                .dev
                .program(tag_flash_offset(block, slot), self.cur_tag.as_bytes())
                .is_err()
        {
            self.block_table[block] = BLOCK_TBL_BAD;
            return Err(FsError::IoError);
        }
        Ok(phys)
    }

    #[inline(never)]
    pub(crate) fn set_tag_state(&mut self, phys_page: u16, new_state: u8) -> Result<(), FsError> {
        let (block, slot) = split_phys_page(phys_page);
        if block < FIRST_DATA_BLOCK || block >= D::TOTAL_BLOCKS || slot == 0 {
            return Ok(());
        }
        self.dev
            .program(tag_flash_offset(block, slot) + 8, &[new_state])?;
        if new_state == TAG_STATE_OBSOLETE {
            let v = self.block_table[block];
            if v != BLOCK_TBL_BAD {
                let free = v >> 4;
                let obs = (v & 0x0F).saturating_add(1).min(DATA_PAGES_PER_BLOCK as u8);
                self.block_table[block] = (free << 4) | obs;
            }
        }
        Ok(())
    }

    /// O(1) direct inode read by inode number `ino >= 2`.
    #[inline(never)]
    pub(crate) fn read_inode(
        &mut self,
        ino: u16,
        out: Option<&mut Inode>,
    ) -> Result<bool, FsError> {
        if ino < 2 {
            return Ok(false);
        }
        let g = ((ino - 2) >> 2) as usize;
        let slot = ((ino - 2) & 3) as usize;
        if g >= MAX_META_GROUPS || self.meta_pages[g] == 0 {
            return Ok(false);
        }
        self.load_page(self.meta_pages[g])?;
        let dst = out.unwrap_or(&mut self.cur_inode);
        Inode::read_slot_into(&self.page_buf, slot, dst);
        Ok(dst.ino == ino)
    }

    #[inline(never)]
    pub(crate) fn scan_meta(
        &mut self,
        query: MetaQuery<'_>,
        mut list_cb: Option<&mut dyn FnMut(&str, usize, u8, bool)>,
    ) -> Result<u16, FsError> {
        let mut matched: u16 = 0;
        for g in 0..MAX_META_GROUPS {
            let mp = self.meta_pages[g];
            if mp == 0 {
                if matches!(query, MetaQuery::FreeSlot) {
                    return Ok((g as u16 * 4) + 2);
                }
                continue;
            }
            self.load_page(mp)?;
            for slot in 0..INODES_PER_PAGE {
                let slot_ino = (g as u16 * 4) + (slot as u16) + 2;
                let slot_free = Inode::slot_is_free(&self.page_buf, slot);
                if matches!(query, MetaQuery::FreeSlot) {
                    if slot_free {
                        return Ok(slot_ino);
                    }
                    continue;
                }
                if slot_free {
                    continue;
                }
                Inode::read_slot_into(&self.page_buf, slot, &mut self.cur_inode);
                match query {
                    MetaQuery::Child(parent_ino, name_bytes) => {
                        if self.cur_inode.parent_ino == parent_ino
                            && (name_bytes.is_empty() || self.cur_inode.name_bytes() == name_bytes)
                        {
                            return Ok(slot_ino);
                        }
                    }
                    MetaQuery::ListDir(dir_ino) => {
                        if self.cur_inode.parent_ino == dir_ino {
                            matched += 1;
                            if let Some(cb) = list_cb.as_deref_mut() {
                                cb(
                                    self.cur_inode.name_str(),
                                    self.cur_inode.size as usize,
                                    self.cur_inode.perm,
                                    self.cur_inode.is_dir(),
                                );
                            }
                        }
                    }
                    MetaQuery::CountFiles => {
                        if !self.cur_inode.is_dir() {
                            matched += 1;
                        }
                    }
                    MetaQuery::FreeSlot => {}
                }
            }
        }
        Ok(matched)
    }

    #[inline(always)]
    pub(crate) fn find_child_inode(
        &mut self,
        parent_ino: u16,
        name: &[u8],
    ) -> Result<bool, FsError> {
        Ok(self.scan_meta(MetaQuery::Child(parent_ino, name), None)? != 0)
    }

    #[inline(never)]
    pub(crate) fn alloc_ino(&mut self) -> Result<u16, FsError> {
        let ino = self.scan_meta(MetaQuery::FreeSlot, None)?;
        if ino == 0 {
            Err(FsError::CapacityFull)
        } else {
            Ok(ino)
        }
    }

    #[inline(never)]
    pub(crate) fn init_new_cur_inode(&mut self, parent_ino: u16, name: &[u8]) {
        self.cur_inode = Inode::EMPTY;
        self.cur_inode.parent_ino = parent_ino;
        let l = name.len().min(MAX_NAME_LEN);
        self.cur_inode.name[..l].copy_from_slice(&name[..l]);
        self.cur_inode.name_len = l as u8;
    }

    /// Commits a modified or deleted (`inode.ino == 0`) inode at `target_ino` in a single atomic page write.
    #[inline(never)]
    pub(crate) fn commit_inode_at(
        &mut self,
        target_ino: u16,
        inode: Option<&Inode>,
        extra_obsolete: &[u16],
    ) -> Result<(), FsError> {
        let new_mp = self.alloc_page(false)?;
        let g = (((target_ino - 2) >> 2) as usize) & (MAX_META_GROUPS - 1);
        let slot = ((target_ino - 2) & 3) as usize;
        let old_mp = self.meta_pages[g];
        self.load_page_or_zero(old_mp)?;
        inode
            .unwrap_or(&self.cur_inode)
            .write_slot(&mut self.page_buf, slot);
        self.program_page_buf(new_mp, 0, g as u16, TAG_KIND_META)?;
        self.meta_pages[g] = new_mp;
        if old_mp != 0 {
            let _ = self.set_tag_state(old_mp, TAG_STATE_OBSOLETE);
        }
        for &ep in extra_obsolete {
            if ep != 0 {
                let _ = self.set_tag_state(ep, TAG_STATE_OBSOLETE);
            }
        }
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn commit_inode(
        &mut self,
        inode: &Inode,
        extra_obsolete: &[u16],
    ) -> Result<(), FsError> {
        self.commit_inode_at(inode.ino, Some(inode), extra_obsolete)
    }

    #[inline(always)]
    pub(crate) fn commit_cur_inode(&mut self, extra_obsolete: &[u16]) -> Result<(), FsError> {
        self.commit_inode_at(self.cur_inode.ino, None, extra_obsolete)
    }

    /// Resolves `path` and loads the target into `self.cur_inode` if found (`Ok(true)`).
    /// When not found and `create_dirs == true`, initializes `self.cur_inode` with `parent_ino` and `basename` (`Ok(false)`).
    #[inline(never)]
    pub(crate) fn resolve_path(&mut self, path: &str, create_dirs: bool) -> Result<bool, FsError> {
        let b = path.as_bytes();
        let mut end = b.len();
        if end == 0 || end > 31 || b[0] != b'/' {
            return Err(FsError::InvalidPath);
        }
        for j in 1..end {
            if b[j] == b'.' && b[j - 1] == b'.' {
                return Err(FsError::InvalidPath);
            }
        }
        while end > 1 && b[end - 1] == b'/' {
            end -= 1;
        }
        if end <= 1 {
            return Err(FsError::InvalidPath);
        }
        let mut parent = ROOT_INO;
        let mut i = 1usize;
        loop {
            let start = i;
            while i < end && b[i] != b'/' {
                i += 1;
            }
            let seg = &b[start..i];
            if seg.is_empty() || seg == b"." || seg.len() > MAX_NAME_LEN {
                return Err(FsError::InvalidPath);
            }
            if !self.mounted {
                if i == end {
                    return Err(FsError::NotFound);
                }
                i += 1;
                continue;
            }
            let found = self.find_child_inode(parent, seg)?;
            if i == end {
                if !found {
                    if !create_dirs {
                        return Err(FsError::NotFound);
                    }
                    self.init_new_cur_inode(parent, seg);
                }
                return Ok(found);
            }
            i += 1; // skip '/'
            if found {
                if !self.cur_inode.is_dir() {
                    return Err(FsError::InvalidPath);
                }
                parent = self.cur_inode.ino;
            } else {
                if !create_dirs {
                    return Err(FsError::NotFound);
                }
                let ino = self.alloc_ino()?;
                self.init_new_cur_inode(parent, seg);
                self.cur_inode.ino = ino;
                self.cur_inode.flags = INODE_FLAG_DIR;
                self.commit_cur_inode(&[])?;
                parent = ino;
            }
        }
    }

    #[inline(never)]
    pub(crate) fn lookup_inode(&mut self, path: &str) -> Result<(), FsError> {
        let hash = CrcEngine::crc32(path.as_bytes());
        if self.mounted
            && self.hot_ino >= 2
            && self.hot_hash == hash
            && self.read_inode(self.hot_ino, None).unwrap_or(false)
        {
            return Ok(());
        }
        self.resolve_path(path, false)?;
        self.hot_hash = hash;
        self.hot_ino = self.cur_inode.ino;
        Ok(())
    }

    #[inline(never)]
    pub fn exists(&mut self, path: &str) -> bool {
        self.lookup_inode(path).is_ok()
    }

    #[inline(never)]
    pub fn stat_info(&mut self, path: &str) -> Result<(usize, bool), FsError> {
        self.lookup_inode(path)?;
        Ok((self.cur_inode.size as usize, self.cur_inode.is_dir()))
    }

    #[inline(always)]
    pub fn file_size(&mut self, path: &str) -> Result<usize, FsError> {
        self.stat_info(path).map(|(sz, _)| sz)
    }

    #[inline(never)]
    pub fn get_permission(&mut self, path: &str) -> Result<u8, FsError> {
        self.lookup_inode(path)?;
        Ok(self.cur_inode.perm)
    }

    #[inline(never)]
    pub fn create_dir(&mut self, path: &str) -> Result<(), FsError> {
        if path == "/" {
            return if self.mounted {
                Ok(())
            } else {
                Err(FsError::NotFound)
            };
        }
        if self.resolve_path(path, true)? {
            return if self.cur_inode.is_dir() {
                Ok(())
            } else {
                Err(FsError::InvalidPath)
            };
        }
        self.cur_inode.ino = self.alloc_ino()?;
        self.cur_inode.flags = INODE_FLAG_DIR;
        self.commit_cur_inode(&[])
    }

    #[inline(never)]
    pub(crate) fn resolve_data_page(&mut self, lpage_idx: u16) -> Result<u16, FsError> {
        let depth = self.cur_inode.depth;
        let s = if depth == DEPTH_DIRECT {
            lpage_idx as usize
        } else if depth == DEPTH_SINGLE_INDIRECT {
            (lpage_idx >> 7) as usize
        } else {
            return Ok(0);
        };
        if s >= 8 {
            return Ok(0);
        }
        let mut p = self.cur_inode.l1_page(s);
        if p != 0 && depth == DEPTH_SINGLE_INDIRECT {
            self.load_page(p)?;
            p = read_idx_u16(&self.page_buf, (lpage_idx & 0x7F) as usize);
        }
        Ok(p)
    }

    /// Return `(depth, size)` and the up-to-8 L1 physical page IDs for a non-inline file.
    /// Used by bare-metal SRAM flashers / Stage-2 `kboot` to stream files from SPI flash
    /// without invoking internal-flash filesystem code during erase/program cycles.
    #[inline(never)]
    pub fn file_extents(
        &mut self,
        path: &str,
        out_l1: &mut [u16; 8],
    ) -> Result<(u8, u32), FsError> {
        self.lookup_inode(path)?;
        if self.cur_inode.is_dir() || self.cur_inode.is_inline() {
            return Err(FsError::InvalidPath);
        }
        for i in 0..8 {
            out_l1[i] = self.cur_inode.l1_page(i);
        }
        Ok((self.cur_inode.depth, self.cur_inode.size))
    }

    pub fn read_file(&mut self, path: &str, buf: &mut [u8]) -> Result<usize, FsError> {
        self.read_file_at(path, 0, buf)
    }

    #[inline(never)]
    pub fn read_file_at(
        &mut self,
        path: &str,
        offset: usize,
        buf: &mut [u8],
    ) -> Result<usize, FsError> {
        self.lookup_inode(path)?;
        if self.cur_inode.is_dir() {
            return Err(FsError::InvalidPath);
        }
        let total = self.cur_inode.size as usize;
        if offset >= total || buf.is_empty() {
            return Ok(0);
        }
        let to_read = (total - offset).min(buf.len());
        if self.cur_inode.is_inline() {
            let end = (offset + to_read).min(MAX_INLINE_LEN);
            let n = end.saturating_sub(offset);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.cur_inode.inline_data.as_ptr().add(offset),
                    buf.as_mut_ptr(),
                    n,
                );
            }
            return Ok(n);
        }
        let mut copied = 0usize;
        while copied < to_read {
            let pos = offset + copied;
            let lpage = (pos >> 8) as u16;
            let in_page = pos & 0xFF;
            let take = (to_read - copied).min(PAGE_SIZE - in_page);
            let phys = self.resolve_data_page(lpage)?;
            self.load_page_or_zero(phys)?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.page_buf.as_ptr().add(in_page),
                    buf.as_mut_ptr().add(copied),
                    take,
                );
            }
            copied += take;
        }
        Ok(copied)
    }

    #[inline(never)]
    pub fn write_file(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        if self.low_power {
            return Err(FsError::LowPower);
        }
        let has_existing = self.resolve_path(path, true)?;
        if has_existing && self.cur_inode.is_dir() {
            return Err(FsError::InvalidPath);
        }
        if !has_existing && !self.has_free_room_for_new() {
            return Err(FsError::CapacityFull);
        }

        let mut old_depth = 0u8;
        let mut old_ptrs = core::mem::MaybeUninit::<[u8; MAX_INLINE_LEN]>::uninit();
        if has_existing {
            if !self.cur_inode.is_inline() {
                old_depth = self.cur_inode.depth;
                old_ptrs.write(self.cur_inode.inline_data);
            }
            self.cur_inode.inline_data = [0; MAX_INLINE_LEN];
        } else {
            self.cur_inode.ino = self.alloc_ino()?;
        }
        self.cur_inode.size = data.len() as u32;
        self.cur_inode.generation = self.global_gen + 1;

        if data.len() <= MAX_INLINE_LEN {
            self.cur_inode.flags = INODE_FLAG_INLINE;
            self.cur_inode.depth = DEPTH_NONE_OR_INLINE;
            self.cur_inode.inline_data[..data.len()].copy_from_slice(data);
            self.commit_cur_inode(&[])?;
        } else {
            self.cur_inode.flags = 0;
            self.cur_inode.size = 0;
            self.cur_inode.depth = DEPTH_DIRECT;
            self.write_pages_at(0, data, None)?;
        }
        self.hot_hash = CrcEngine::crc32(path.as_bytes());
        self.hot_ino = self.cur_inode.ino;

        if has_existing {
            if old_depth != 0 {
                // SAFETY: `old_ptrs` is written above whenever `old_depth != 0`.
                self.mark_tree_obsolete(old_depth, unsafe { old_ptrs.assume_init_ref() });
            }
        } else {
            self.active_files = self.active_files.saturating_add(1);
        }
        Ok(())
    }

    pub fn write_at(&mut self, path: &str, offset: usize, data: &[u8]) -> Result<(), FsError> {
        match self.lookup_inode(path) {
            Ok(()) => {}
            Err(FsError::NotFound) if self.mounted => {
                if offset == 0 {
                    return self.write_file(path, data);
                }
                self.write_file(path, &[])?;
            }
            Err(e) => return Err(e),
        }
        if self.cur_inode.is_dir() {
            return Err(FsError::InvalidPath);
        }
        if self.low_power {
            return Err(FsError::LowPower);
        }
        let new_end = offset + data.len();
        let new_size = (self.cur_inode.size as usize).max(new_end);
        if new_size <= MAX_INLINE_LEN && self.cur_inode.is_inline() {
            self.cur_inode.inline_data[offset..offset + data.len()].copy_from_slice(data);
            self.cur_inode.size = new_size as u32;
            self.cur_inode.generation = self.global_gen + 1;
            return self.commit_cur_inode(&[]);
        }
        self.write_pages_at(offset, data, None)
    }

    pub fn append_file(&mut self, path: &str, data: &[u8]) -> Result<(), FsError> {
        let cur_sz = self.file_size(path).unwrap_or(0);
        self.write_at(path, cur_sz, data)
    }

    /// Append fixed or variable records in a bounded cyclic ring buffer (`max_bytes`).
    pub fn write_ring(&mut self, path: &str, max_bytes: usize, data: &[u8]) -> Result<(), FsError> {
        if max_bytes == 0 || data.len() > max_bytes {
            return Err(FsError::CapacityFull);
        }
        let cur_sz = self.file_size(path).unwrap_or(0);
        if cur_sz + data.len() <= max_bytes {
            self.append_file(path, data)
        } else {
            match self.lookup_inode(path) {
                Ok(()) => {}
                Err(FsError::NotFound) if self.mounted => return self.write_file(path, data),
                Err(e) => return Err(e),
            }
            self.cur_inode.flags |= INODE_FLAG_RING;
            self.write_pages_at(0, data, Some(max_bytes))
        }
    }

    /// Unmount the filesystem and perform a full raw `0xFF` erase of the underlying flash device.
    pub fn wipe_raw(&mut self) -> Result<(), FsError> {
        self.mounted = false;
        self.dev.chip_erase()
    }
}
