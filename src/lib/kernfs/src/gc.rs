//! KernFS v2 Extent Allocation, CoW Partial Updates, Mount, Format & Garbage Collection.

use crate::device::{BlockDevice, FsError};
use crate::format::*;
use crate::fs::{KernFS, MetaQuery};

impl<'a, D: BlockDevice> KernFS<'a, D> {
    pub(crate) fn has_free_room_for_new(&self) -> bool {
        let (free, obs, _) = self.page_counts();
        (free + obs) as usize > RESERVE_BLOCKS * DATA_PAGES_PER_BLOCK + 2
    }

    #[inline(never)]
    pub(crate) fn alloc_page(&mut self, allow_gc: bool) -> Result<u16, FsError> {
        if self.low_power {
            return Err(FsError::LowPower);
        }
        if allow_gc
            && !self.in_gc
            && (self.page_counts().0 as usize) <= RESERVE_BLOCKS * DATA_PAGES_PER_BLOCK
        {
            self.in_gc = true;
            let _ = self.gc_reclaim_one(self.cur_alloc_block as usize);
            self.in_gc = false;
        }
        for _attempt in 0..D::TOTAL_BLOCKS {
            let b = self.cur_alloc_block as usize;
            let p = self.cur_alloc_page as usize;
            if b >= FIRST_DATA_BLOCK
                && b < D::TOTAL_BLOCKS
                && self.block_table[b] != BLOCK_TBL_BAD
                && p >= 1
                && p <= DATA_PAGES_PER_BLOCK
                && (self.block_table[b] >> 4) > 0
            {
                let free = (self.block_table[b] >> 4) - 1;
                let obs = self.block_table[b] & 0x0F;
                self.block_table[b] = (free << 4) | obs;
                self.cur_alloc_page += 1;
                return Ok(make_phys_page(b, p));
            }

            let next_b = self.pick_next_alloc_block();
            if next_b != 0 {
                self.cur_alloc_block = next_b;
                self.cur_alloc_page = 1;
                continue;
            }

            if !allow_gc || self.in_gc {
                return Err(FsError::CapacityFull);
            }
            self.in_gc = true;
            let ok = self.gc_reclaim_one(b);
            self.in_gc = false;
            if !ok? {
                return Err(FsError::CapacityFull);
            }
        }
        Err(FsError::CapacityFull)
    }

    #[inline(never)]
    fn read_block_ec(&mut self, block: usize) -> u32 {
        let mut hdr_uninit = core::mem::MaybeUninit::<BlockHeader>::uninit();
        let hdr_bytes =
            unsafe { core::slice::from_raw_parts_mut(hdr_uninit.as_mut_ptr() as *mut u8, 16) };
        if self.dev.read(block * D::BLOCK_SIZE, hdr_bytes).is_ok() {
            let h = unsafe { hdr_uninit.assume_init() };
            if h.fourcc == FOURCC_BLOCK_V2 && h.kind != BLOCK_KIND_BAD {
                return h.erase_count;
            }
        }
        0
    }

    #[inline(never)]
    fn pick_next_alloc_block(&mut self) -> u16 {
        let mut best_erased = 0usize;
        let mut best_ec = u32::MAX;
        for b in FIRST_DATA_BLOCK..D::TOTAL_BLOCKS {
            if self.block_table[b] != BLOCK_TBL_ERASED {
                continue;
            }
            let ec = self.read_block_ec(b);
            if ec < best_ec {
                best_ec = ec;
                best_erased = b;
            }
        }

        if best_erased != 0 {
            self.block_seq = self.block_seq.wrapping_add(1);
            let bs = self.block_seq.to_le_bytes();
            let meta_bytes = [bs[0], bs[1], BLOCK_KIND_ACTIVE, 0xFF];
            if self
                .dev
                .program(best_erased * D::BLOCK_SIZE + 12, &meta_bytes)
                .is_err()
            {
                self.retire_block(best_erased);
                return 0;
            }
            return best_erased as u16;
        }
        0
    }

    pub(crate) fn retire_block(&mut self, block: usize) {
        if block < FIRST_DATA_BLOCK || block >= D::TOTAL_BLOCKS || block >= MAX_BLOCKS {
            return;
        }
        self.block_table[block] = BLOCK_TBL_BAD;
        let _ = self
            .dev
            .program(block * D::BLOCK_SIZE + 14, &[BLOCK_KIND_BAD]);
    }

    #[inline(never)]
    fn erase_and_init_block(&mut self, block: usize, prev_ec: u32) -> Result<(), FsError> {
        let addr = block * D::BLOCK_SIZE;
        if self.dev.erase_sector(addr).is_err() {
            self.retire_block(block);
            return Err(FsError::IoError);
        }
        let hdr = BlockHeader::new(
            self.format_epoch,
            prev_ec.saturating_add(1),
            0xFFFF,
            BLOCK_KIND_ERASED,
        );
        if self.dev.program(addr, hdr.as_bytes()).is_err() {
            self.retire_block(block);
            return Err(FsError::IoError);
        }
        self.block_table[block & (MAX_BLOCKS - 1)] = BLOCK_TBL_ERASED;
        self.total_erases = self.total_erases.saturating_add(1);
        Ok(())
    }

    /// Reclaim one block via GC (or static wear leveling every 256 erases). Returns `true` if a block was erased.
    #[inline(never)]
    pub(crate) fn gc_reclaim_one(&mut self, avoid_block: usize) -> Result<bool, FsError> {
        let mut best_block = 0usize;
        let mut best_ec = 0u32;
        let mut best_score = 0u32;
        let static_wl = (self.total_erases & 0xFF) == 0 && self.total_erases > 0;

        for b in FIRST_DATA_BLOCK..D::TOTAL_BLOCKS {
            if b == avoid_block {
                continue;
            }
            let v = self.block_table[b];
            if v == BLOCK_TBL_BAD || v == BLOCK_TBL_ERASED {
                continue;
            }
            let obs = (v & 0x0F) as u32;
            let free = (v >> 4) as u32;
            if obs == 0 && !static_wl {
                continue;
            }
            let ec = self.read_block_ec(b);
            let zero_live_bonus = if free + obs == DATA_PAGES_PER_BLOCK as u32 {
                1u32 << 20
            } else {
                0
            };
            let score = if static_wl && obs == 0 && free == 0 {
                1000u32.saturating_sub(ec.min(999))
            } else {
                zero_live_bonus + obs * 1024 + (15 - free) * 16 + 100u32.saturating_sub(ec.min(100))
            };
            if score > best_score {
                best_score = score;
                best_block = b;
                best_ec = ec;
            }
        }

        if best_block == 0 {
            return Ok(false);
        }

        for s in 1..PAGES_PER_BLOCK {
            let phys = make_phys_page(best_block, s);
            self.read_tag(phys)?;
            if self.cur_tag.state == TAG_STATE_COMMITTED {
                self.relocate_if_live(phys)?;
            }
        }

        self.erase_and_init_block(best_block, best_ec)?;
        Ok(true)
    }

    /// Allocates a COW L1 index page with `slot = new_ptr`, returning `(new_l1, prev_ptr)` packed as `u32`.
    /// Arguments are packed into 4 AAPCS register slots to avoid caller stack spills:
    /// - `ino_l2`: `(ino as u32) | ((l2_idx as u32) << 16)`
    /// - `old_slot`: `(old_idx_p as u32) | ((slot as u32) << 16)`
    /// - `new_gc`: `(new_ptr as u32) | ((allow_gc as u32) << 16)`
    #[inline(never)]
    fn cow_set_idx_slot(
        &mut self,
        ino_l2: u32,
        old_slot: u32,
        new_gc: u32,
    ) -> Result<u32, FsError> {
        let new_p = self.alloc_page((new_gc >> 16) != 0)?;
        let old_idx_p = old_slot as u16;
        let slot = (old_slot >> 16) as usize;
        let new_ptr = new_gc as u16;
        self.load_page_or_zero(old_idx_p)?;
        let prev = read_idx_u16(&self.page_buf, slot);
        write_idx_u16(&mut self.page_buf, slot, new_ptr);
        let written =
            self.program_page_buf(new_p, ino_l2 as u16, (ino_l2 >> 16) as u16, TAG_KIND_INDEX1)?;
        Ok((written as u32) | ((prev as u32) << 16))
    }

    #[inline(never)]
    fn relocate_if_live(&mut self, phys: u16) -> Result<(), FsError> {
        let ino = self.cur_tag.ino;
        let page_idx = self.cur_tag.page_idx;
        let kind = self.cur_tag.kind;
        let idx = page_idx as usize;
        let mut inode_uninit = core::mem::MaybeUninit::<Inode>::uninit();
        let inode = unsafe { &mut *inode_uninit.as_mut_ptr() };
        match kind {
            TAG_KIND_META => {
                if idx >= MAX_META_GROUPS || self.meta_pages[idx] != phys {
                    return Ok(());
                }
            }
            TAG_KIND_DATA | TAG_KIND_INDEX1 => {
                if !self.read_inode(ino, Some(inode))? {
                    return Ok(());
                }
                let expected = if kind == TAG_KIND_INDEX1 || inode.depth == DEPTH_DIRECT {
                    inode.l1_page(idx)
                } else if inode.depth == DEPTH_SINGLE_INDIRECT {
                    let p = inode.l1_page(idx >> 7);
                    if p != 0 {
                        self.load_page(p)?;
                        read_idx_u16(&self.page_buf, idx & 0x7F)
                    } else {
                        0
                    }
                } else {
                    0
                };
                if expected != phys {
                    return Ok(());
                }
            }
            _ => return Ok(()),
        }

        let new_p = self.alloc_page(false)?;
        self.load_page(phys)?;
        self.program_page_buf(new_p, ino, page_idx, kind)?;

        if kind == TAG_KIND_META {
            self.meta_pages[idx] = new_p;
            let _ = self.set_tag_state(phys, TAG_STATE_OBSOLETE);
            return Ok(());
        }

        let mut obs = [phys, 0u16];
        if kind == TAG_KIND_INDEX1 || inode.depth == DEPTH_DIRECT {
            inode.set_l1_page(idx, new_p);
        } else {
            let l2_idx = idx >> 7;
            let old_l1 = inode.l1_page(l2_idx);
            let packed = self.cow_set_idx_slot(
                (inode.ino as u32) | ((l2_idx as u32) << 16),
                (old_l1 as u32) | (((idx & 0x7F) as u32) << 16),
                new_p as u32,
            )?;
            inode.set_l1_page(l2_idx, packed as u16);
            obs[1] = old_l1;
        }
        if self.cur_inode.ino == inode.ino {
            self.cur_inode.inline_data = inode.inline_data;
            self.cur_inode.depth = inode.depth;
        }
        self.commit_inode(inode, &obs)
    }

    /// Background idle maintenance hook: reclaims 1 dirty/obsolete block if pre-erased pool < 4.
    pub fn tick_idle(&mut self) -> Result<bool, FsError> {
        if !self.mounted || self.low_power {
            return Ok(false);
        }
        let mut erased_cnt = 0usize;
        for b in FIRST_DATA_BLOCK..D::TOTAL_BLOCKS {
            if self.block_table[b] == BLOCK_TBL_ERASED {
                erased_cnt += 1;
            }
        }
        if erased_cnt < 4 {
            return self.gc_reclaim_one(self.cur_alloc_block as usize);
        }
        Ok(false)
    }

    #[inline(never)]
    pub(crate) fn write_pages_at(
        &mut self,
        offset: usize,
        data: &[u8],
        ring_cap: Option<usize>,
    ) -> Result<(), FsError> {
        let ino = self.cur_inode.ino;
        if self.cur_inode.is_inline() {
            if self.cur_inode.size > 0 {
                let phys = self.alloc_page(true)?;
                let ilen = (self.cur_inode.size as usize).min(MAX_INLINE_LEN);
                self.page_buf.fill(0);
                self.page_buf[..ilen].copy_from_slice(&self.cur_inode.inline_data[..ilen]);
                self.program_page_buf(phys, ino, 0, TAG_KIND_DATA)?;
                self.cur_inode.set_l1_page(0, phys);
            } else {
                self.cur_inode.inline_data = [0; MAX_INLINE_LEN];
            }
            self.cur_inode.flags &= !INODE_FLAG_INLINE;
            self.cur_inode.depth = DEPTH_DIRECT;
        }

        let cap = ring_cap.unwrap_or(0);
        let mut pos = 0usize;
        while pos < data.len() {
            let mut abs_pos = offset + pos;
            if cap != 0 && abs_pos >= cap {
                abs_pos %= cap;
            }
            let lpage = (abs_pos >> 8) as u16;
            if (lpage as usize) >= 8 * INDEX_PTRS_PER_PAGE {
                return Err(FsError::CapacityFull);
            }
            let in_page = abs_pos & 0xFF;
            let chunk = (data.len() - pos).min(PAGE_SIZE - in_page);

            let new_p = self.alloc_page(true)?;
            let old_p = self.resolve_data_page(lpage)?;
            self.load_page_or_zero(old_p)?;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    data.as_ptr().add(pos),
                    self.page_buf.as_mut_ptr().add(in_page),
                    chunk,
                );
            }
            self.program_page_buf(new_p, ino, lpage, TAG_KIND_DATA)?;
            let new_end = (abs_pos + chunk) as u32;
            if cap == 0 {
                if new_end > self.cur_inode.size {
                    self.cur_inode.size = new_end;
                }
            } else {
                self.cur_inode.size = self.cur_inode.size.max(new_end).min(cap as u32);
            }
            self.cow_update_data_ptr(lpage, new_p)?;
            pos += chunk;
        }
        Ok(())
    }

    #[inline(never)]
    pub(crate) fn cow_update_data_ptr(
        &mut self,
        lpage: u16,
        new_data_p: u16,
    ) -> Result<(), FsError> {
        let ino = self.cur_inode.ino;
        let mut obs = [0u16; 2];
        let old_d = self.cur_inode.depth;

        if old_d <= DEPTH_DIRECT && lpage < 8 {
            let old_p = self.cur_inode.l1_page(lpage as usize);
            if old_d == DEPTH_DIRECT && old_p != new_data_p {
                obs[0] = old_p;
            }
            self.cur_inode.depth = DEPTH_DIRECT;
            self.cur_inode.set_l1_page(lpage as usize, new_data_p);
        } else {
            let l2_idx = (lpage >> 7) as usize;
            let l1_idx = (lpage & 0x7F) as usize;
            if l2_idx >= 8 {
                return Err(FsError::CapacityFull);
            }
            if old_d == DEPTH_DIRECT {
                let l1_0 = self.alloc_page(true)?;
                self.page_buf.fill(0);
                for s in 0..8 {
                    write_idx_u16(&mut self.page_buf, s, self.cur_inode.l1_page(s));
                }
                if l2_idx == 0 {
                    write_idx_u16(&mut self.page_buf, l1_idx, new_data_p);
                }
                self.program_page_buf(l1_0, ino, 0, TAG_KIND_INDEX1)?;
                self.cur_inode.inline_data = [0; MAX_INLINE_LEN];
                self.cur_inode.set_l1_page(0, l1_0);
                self.cur_inode.depth = DEPTH_SINGLE_INDIRECT;
                if l2_idx != 0 {
                    // Commit promoted L1 root to flash before a second alloc can trigger GC.
                    self.commit_cur_inode(&[])?;
                }
            }
            if !(old_d == DEPTH_DIRECT && l2_idx == 0) {
                let old_l1 = self.cur_inode.l1_page(l2_idx);
                let packed = self.cow_set_idx_slot(
                    (ino as u32) | ((l2_idx as u32) << 16),
                    (old_l1 as u32) | ((l1_idx as u32) << 16),
                    (new_data_p as u32) | (1 << 16),
                )?;
                let prev = (packed >> 16) as u16;
                obs[0] = old_l1;
                if prev != new_data_p {
                    obs[1] = prev;
                }
                self.cur_inode.set_l1_page(l2_idx, packed as u16);
                self.cur_inode.depth = DEPTH_SINGLE_INDIRECT;
            }
        }

        self.cur_inode.generation = self.global_gen + 1;
        self.commit_cur_inode(&obs)
    }

    #[inline(never)]
    pub(crate) fn mark_tree_obsolete(&mut self, depth: u8, ptrs: &[u8; MAX_INLINE_LEN]) {
        if depth == 0 {
            return;
        }
        for s in 0..8 {
            let p = u16::from_le_bytes([ptrs[s * 2], ptrs[s * 2 + 1]]);
            if p != 0 {
                if depth == DEPTH_SINGLE_INDIRECT && self.load_page(p).is_ok() {
                    for i in 0..INDEX_PTRS_PER_PAGE {
                        let dp = read_idx_u16(&self.page_buf, i);
                        if dp != 0 {
                            let _ = self.set_tag_state(dp, TAG_STATE_OBSOLETE);
                        }
                    }
                }
                let _ = self.set_tag_state(p, TAG_STATE_OBSOLETE);
            }
        }
    }

    /// Metadata-only rename across any directories (zero data page copies).
    #[inline(never)]
    pub fn rename(&mut self, src: &str, dst: &str) -> Result<(), FsError> {
        #[inline(never)]
        fn trim_slashes(s: &str) -> &str {
            let mut b = s.as_bytes();
            while let [b'/', rest @ ..] = b {
                b = rest;
            }
            while let [rest @ .., b'/'] = b {
                b = rest;
            }
            unsafe { core::str::from_utf8_unchecked(b) }
        }
        if src == dst {
            return Ok(());
        }
        self.lookup_inode(src)?;
        let mut src_inode = self.cur_inode;
        let src_t = trim_slashes(src);
        let dst_t = trim_slashes(dst);
        if src_inode.is_dir()
            && !src_t.is_empty()
            && let Some(rest) = dst_t.strip_prefix(src_t)
            && (rest.is_empty() || rest.as_bytes()[0] == b'/')
        {
            return Err(FsError::InvalidPath);
        }
        let dst_existed = self.resolve_path(dst, true)?;
        src_inode.parent_ino = self.cur_inode.parent_ino;
        src_inode.name = self.cur_inode.name;
        src_inode.name_len = self.cur_inode.name_len;
        if dst_existed {
            if self.cur_inode.is_dir() || src_inode.is_dir() {
                return Err(FsError::InvalidPath);
            }
            self.remove(dst)?;
        }

        src_inode.generation = self.global_gen + 1;
        self.commit_inode(&src_inode, &[])?;
        self.hot_ino = 0;
        self.hot_hash = 0;
        Ok(())
    }

    #[inline(never)]
    pub fn remove(&mut self, path: &str) -> Result<(), FsError> {
        self.lookup_inode(path)?;
        let ino = self.cur_inode.ino;
        let is_dir = self.cur_inode.is_dir();
        let depth = if is_dir || self.cur_inode.is_inline() {
            0
        } else {
            self.cur_inode.depth
        };
        let ptrs = self.cur_inode.inline_data;
        if is_dir && self.find_child_inode(ino, b"")? {
            return Err(FsError::AccessDenied);
        }
        self.cur_inode = Inode::EMPTY;
        self.commit_inode_at(ino, None, &[])?;
        if self.hot_ino == ino {
            self.hot_ino = 0;
        }
        self.mark_tree_obsolete(depth, &ptrs);
        if !is_dir {
            self.active_files = self.active_files.saturating_sub(1);
        }
        Ok(())
    }

    #[inline(never)]
    pub fn set_permission(&mut self, path: &str, perm: u8) -> Result<(), FsError> {
        self.lookup_inode(path)?;
        if self.cur_inode.perm == perm {
            return Ok(());
        }
        self.cur_inode.perm = perm;
        self.commit_cur_inode(&[])
    }

    #[inline(never)]
    fn list_dir_dyn(
        &mut self,
        dir_path: &str,
        cb: &mut dyn FnMut(&str, usize, u8, bool),
    ) -> Result<usize, FsError> {
        let dir_ino = if dir_path == "/" {
            if !self.mounted {
                return Err(FsError::NotFound);
            }
            ROOT_INO
        } else {
            match self.lookup_inode(dir_path) {
                Ok(()) if self.cur_inode.is_dir() => self.cur_inode.ino,
                Ok(()) => return Ok(0),
                Err(FsError::NotFound) if self.mounted => return Ok(0),
                Err(e) => return Err(e),
            }
        };
        let count = self.scan_meta(crate::fs::MetaQuery::ListDir(dir_ino), Some(cb))?;
        Ok(count as usize)
    }

    pub fn list_dir<F: FnMut(&str, usize, u8, bool)>(
        &mut self,
        dir_path: &str,
        mut cb: F,
    ) -> Result<usize, FsError> {
        self.list_dir_dyn(dir_path, &mut cb)
    }

    /// Detect whether blocks 0..16 contain any legacy KernFS v1 headers.
    pub fn has_v1_volume(dev: &mut D) -> bool {
        let mut hdr_uninit = core::mem::MaybeUninit::<[u8; 64]>::uninit();
        let hdr = unsafe { &mut *hdr_uninit.as_mut_ptr() };
        let limit = D::TOTAL_BLOCKS.min(16);
        for s in 0..limit {
            if dev.read(s * D::BLOCK_SIZE, hdr).is_ok() && is_valid_v1_header(hdr) {
                return true;
            }
        }
        false
    }

    #[inline(never)]
    fn load_latest_superblock(&mut self) -> bool {
        let mut found = false;
        let mut best_seq = 0u32;
        for s in 0..SUPER_BLOCKS {
            if self
                .dev
                .read(s * D::BLOCK_SIZE, &mut self.page_buf[..32])
                .is_ok()
                && let Some(sb) = SuperblockV2::from_bytes(&self.page_buf[..32])
                && sb.total_blocks as usize == D::TOTAL_BLOCKS
                && (!found || sb.seq >= best_seq)
            {
                best_seq = sb.seq;
                self.format_epoch = sb.format_epoch;
                self.super_seq = sb.seq;
                self.global_gen = sb.global_gen;
                self.block_seq = sb.block_seq;
                found = true;
            }
        }
        found
    }

    /// Format flash into a fresh KernFS v2 volume, preserving block erase counts where possible.
    #[inline(never)]
    pub fn format(&mut self, progress_cb: Option<&mut dyn FnMut(u8)>) -> Result<(), FsError> {
        if self.low_power {
            return Err(FsError::LowPower);
        }
        let had_sb = self.load_latest_superblock();
        self.format_epoch = self.format_epoch.wrapping_add(had_sb as u32).max(1);
        self.super_seq = if had_sb {
            self.super_seq.wrapping_add(1).max(1)
        } else {
            1
        };
        self.global_gen = 1;
        self.block_seq = 1;
        self.reset_tables();

        for b in FIRST_DATA_BLOCK..D::TOTAL_BLOCKS {
            let ec = self.read_block_ec(b);
            let _ = self.erase_and_init_block(b, ec);
        }

        self.cur_alloc_block = 0;
        self.cur_alloc_page = 1;

        for s in 0..SUPER_BLOCKS {
            self.dev.erase_sector(s * D::BLOCK_SIZE)?;
        }
        let mut sb = SuperblockV2 {
            fourcc: FOURCC_SUPER_V2,
            version: 2,
            flags: 0,
            total_blocks: D::TOTAL_BLOCKS as u16,
            format_epoch: self.format_epoch,
            seq: self.super_seq,
            global_gen: self.global_gen,
            next_ino: 2,
            block_seq: self.block_seq,
            root_meta_hint: 0,
            reserved: 0,
            crc32: 0,
        };
        sb.finalize();
        let slot = (self.super_seq as usize) & 1;
        self.dev.program(slot * D::BLOCK_SIZE, sb.to_bytes())?;
        self.mounted = true;
        if let Some(cb) = progress_cb {
            cb(100);
        }
        Ok(())
    }

    #[inline(never)]
    pub fn mount(&mut self) -> Result<(), FsError> {
        if !self.load_latest_superblock() {
            return Err(FsError::NotFound);
        }
        self.reset_tables();

        let mut highest_gen = self.global_gen;
        let mut highest_bseq = self.block_seq;
        let mut active_block_candidate = 0usize;
        let mut active_next_page = 1u8;
        let mut best_active_seq = 0u16;

        for b in FIRST_DATA_BLOCK..D::TOTAL_BLOCKS {
            if self
                .dev
                .read(b * D::BLOCK_SIZE, &mut self.page_buf[..16])
                .is_err()
            {
                self.block_table[b] = BLOCK_TBL_BAD;
                continue;
            }
            let hdr = BlockHeader::from_bytes(&self.page_buf[..16]);
            if hdr.fourcc != FOURCC_BLOCK_V2 || hdr.format_epoch != self.format_epoch {
                self.block_table[b] = DATA_PAGES_PER_BLOCK as u8;
                continue;
            }
            if hdr.kind == BLOCK_KIND_BAD {
                self.block_table[b] = BLOCK_TBL_BAD;
                continue;
            }
            if hdr.kind == BLOCK_KIND_ERASED {
                self.block_table[b] = BLOCK_TBL_ERASED;
                continue;
            }

            highest_bseq = highest_bseq.max(hdr.block_seq);
            let mut last_used = 0u8;
            let mut obs = 0u8;
            for s in 1..PAGES_PER_BLOCK {
                let phys = make_phys_page(b, s);
                if self.read_tag(phys).is_err() {
                    continue;
                }
                let state = self.cur_tag.state;
                if state == TAG_STATE_FREE {
                    continue;
                }
                last_used = s as u8;
                if state == TAG_STATE_COMMITTED {
                    let tag_gen = self.cur_tag.generation;
                    highest_gen = highest_gen.max(tag_gen);
                    if self.cur_tag.kind == TAG_KIND_META {
                        let g = self.cur_tag.page_idx as usize;
                        let prev = if g < MAX_META_GROUPS {
                            self.meta_pages[g]
                        } else {
                            u16::MAX
                        };
                        if g < MAX_META_GROUPS
                            && (prev == 0
                                || self.read_tag(prev).is_err()
                                || self.cur_tag.generation <= tag_gen)
                            && self.load_page(phys).is_ok()
                        {
                            if prev != 0 {
                                let _ = self.set_tag_state(prev, TAG_STATE_OBSOLETE);
                            }
                            self.meta_pages[g] = phys;
                        } else {
                            obs += 1;
                        }
                    }
                } else {
                    obs += 1;
                }
            }
            let tail_free = (DATA_PAGES_PER_BLOCK as u8) - last_used;
            self.block_table[b] = (tail_free << 4) | obs;
            if tail_free > 0 && hdr.block_seq >= best_active_seq {
                best_active_seq = hdr.block_seq;
                active_block_candidate = b;
                active_next_page = last_used + 1;
            }
        }

        self.global_gen = highest_gen.max(1);
        self.block_seq = highest_bseq.max(1);
        self.cur_alloc_block = active_block_candidate as u16;
        self.cur_alloc_page = active_next_page;
        self.active_files = self.scan_meta(MetaQuery::CountFiles, None)?;
        self.mounted = true;
        Ok(())
    }
}
