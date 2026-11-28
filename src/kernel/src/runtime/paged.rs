//! KRX Demand-Paged Native Subsystem (`P1 v2` Streamed Virtual Code Cache & `P0` Leaf Overlays).
//!
//! Implements the unlimited-size `StreamedWindowCache` (`<= 384 KiB`), `SectionBlockMap`
//! (`0x0005`) parser, Clock/LRU victim selection with `.krx.pinned` immunity, and the
//! `PageReturnFrame` shadow return stack (`ShadowReturnStack`) on Cortex-M0+ (no MMU).

use super::image::{BlockEntry, LeafContext, LeafContinuation, LeafEntry};

/// Block flag: pinned in cache (never evicted once loaded; e.g., DSP inner loops)
pub const BLOCK_FLAG_PINNED: u8 = 1 << 0;
/// Block flag: real-time bounded latency block
pub const BLOCK_FLAG_RT: u8 = 1 << 1;
/// Block flag: sequential prefetch hint for next block
pub const BLOCK_FLAG_HINT_NEXT: u8 = 1 << 2;
/// Block flag: backwards-safe in-place LZSS compressed code page (`P1 v2`)
pub const BLOCK_FLAG_LZSS: u8 = 1 << 3;

/// Maximum KernFS v2.1 per-file size (`12 L1 roots * 128 pages * 256 B = 384 KiB`).
pub const MAX_STREAMED_IMAGE_BYTES: u32 = 384 * 1024;
/// Sentinel indicating an empty cache way in `StreamedWindowCache`.
pub const EMPTY_PAGE_ID: u16 = 0xFFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagedError {
    InvalidBlockId,
    InvalidLeafId,
    NoUnpinnedSlotAvailable,
    ShadowStackOverflow,
    ShadowStackUnderflow,
    StorageReadFailed,
    ReentrantDispatchDenied,
    LeafExecutionFailed,
    MaxHopsExceeded,
}

impl BlockEntry {
    pub const SIZE: usize = 8;

    pub const fn empty() -> Self {
        Self {
            block_id: 0,
            flags: 0,
            size: 0,
            flash_offset: 0,
        }
    }

    /// Parse an 8-byte `BlockEntry` from little-endian bytes.
    pub fn parse(buf: &[u8]) -> Option<Self> {
        if buf.len() < Self::SIZE {
            return None;
        }
        Some(Self {
            block_id: buf[0],
            flags: buf[1],
            size: u16::from_le_bytes([buf[2], buf[3]]),
            flash_offset: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
        })
    }

    #[inline(always)]
    pub const fn is_pinned(&self) -> bool {
        (self.flags & BLOCK_FLAG_PINNED) != 0
    }
}

/// 4-byte return frame for `P1 v2` unlimited-size paged calls (`u16` `caller_page_id` up to `768+` pages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(4))]
pub struct PageReturnFrame {
    pub caller_page_id: u16,
    pub return_offset: u16,
}

const _: () = {
    assert!(core::mem::size_of::<PageReturnFrame>() == 4);
    assert!(core::mem::align_of::<PageReturnFrame>() == 4);
};

impl PageReturnFrame {
    pub const fn empty() -> Self {
        Self {
            caller_page_id: 0,
            return_offset: 0,
        }
    }
}

/// 8-byte ($2^3$) cache way metadata for `StreamedWindowCache` (`P1 v2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct CacheWayMeta {
    pub resident_page: u16,
    pub loaded_size: u16,
    pub last_access_tick: u16,
    pub flags: u8,
    pub _pad: u8,
}

const _: () = assert!(core::mem::size_of::<CacheWayMeta>() == 8);

impl CacheWayMeta {
    pub const fn empty() -> Self {
        Self {
            resident_page: EMPTY_PAGE_ID,
            loaded_size: 0,
            last_access_tick: 0,
            flags: 0,
            _pad: 0,
        }
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.resident_page == EMPTY_PAGE_ID
    }

    #[inline(always)]
    pub const fn is_pinned(&self) -> bool {
        (self.flags & BLOCK_FLAG_PINNED) != 0
    }
}

/// Transient page descriptor resolved on a `P1 v2` cache miss (zero SRAM array cost).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct StreamedPageDesc {
    pub flash_offset: u32,
    pub page_id: u16,
    pub raw_size: u16,
    pub exec_size: u16,
    pub flags: u8,
    pub _pad: u8,
}

/// Unlimited-Size `P1 v2` Streamed Virtual Code Cache Controller (up to `384 KiB` / `768` pages).
///
/// Stores metadata ONLY for the `MAX_SLOTS` SRAM cache ways (`8 x 8 B = 64 B`), eliminating
/// fixed `BlockEntry` arrays in RAM so any `.krx` fitting on `KernFS` executes with constant RAM.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct StreamedWindowCache<const MAX_SLOTS: usize = 8> {
    pub code_flash_offset: u32,
    pub code_total_size: u32,
    pub block_map_flash_offset: u32,
    pub page_count: u16,
    pub cache_base_off: u16,
    pub rt_pin_mask: u16,
    pub access_clock: u16,
    pub fault_count: u16,
    pub hit_count: u16,
    pub active_slots: u8,
    pub page_shift: u8,
    pub ways: [CacheWayMeta; MAX_SLOTS],
}

impl<const MAX_SLOTS: usize> Default for StreamedWindowCache<MAX_SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_SLOTS: usize> StreamedWindowCache<MAX_SLOTS> {
    pub const fn new() -> Self {
        Self {
            code_flash_offset: 0,
            code_total_size: 0,
            block_map_flash_offset: 0,
            page_count: 0,
            cache_base_off: 0,
            rt_pin_mask: 0,
            access_clock: 0,
            fault_count: 0,
            hit_count: 0,
            active_slots: 4,
            page_shift: 9,
            ways: [const { CacheWayMeta::empty() }; MAX_SLOTS],
        }
    }

    #[inline(always)]
    pub const fn page_size(&self) -> u16 {
        1u16 << self.page_shift
    }

    /// Byte offset of way `slot_idx` inside Zone B (`cache_base_off + (slot_idx << page_shift)`).
    #[inline(always)]
    pub const fn slot_byte_offset(&self, slot_idx: usize) -> usize {
        (self.cache_base_off as usize) + (slot_idx << self.page_shift)
    }

    pub fn configure(
        &mut self,
        page_size: u16,
        requested_slots: u8,
        avail_cache_bytes: usize,
        cache_base_off: u16,
        rt_pin_mask: u16,
        code_flash_offset: u32,
        code_total_size: u32,
        block_map_flash_offset: u32,
        block_map_count: u16,
    ) {
        self.ways = [const { CacheWayMeta::empty() }; MAX_SLOTS];
        self.access_clock = 0;
        self.fault_count = 0;
        self.hit_count = 0;
        self.cache_base_off = cache_base_off;
        self.rt_pin_mask = rt_pin_mask;
        self.code_flash_offset = code_flash_offset;
        self.code_total_size = code_total_size.min(MAX_STREAMED_IMAGE_BYTES);

        let shift: u8 = if page_size == 512 { 9 } else { 10 };
        self.page_shift = shift;
        let max_by_ram = (avail_cache_bytes >> shift).min(MAX_SLOTS);
        let def_slots = if requested_slots == 0 {
            max_by_ram
        } else {
            (requested_slots as usize).min(max_by_ram)
        };
        self.active_slots = def_slots.max(1).min(MAX_SLOTS) as u8;

        if block_map_flash_offset != 0 && block_map_count > 0 {
            self.block_map_flash_offset = block_map_flash_offset;
            self.page_count = block_map_count;
        } else {
            self.block_map_flash_offset = 0;
            let mask = (1u32 << shift) - 1;
            self.page_count = ((self.code_total_size + mask) >> shift) as u16;
        }
    }

    /// Zero-I/O fast cache-hit probe (`O(active_slots)` over `u16` resident tags).
    #[inline(always)]
    pub fn probe_hit(&mut self, page_id: u16) -> Option<(usize, u16, u8)> {
        let limit = (self.active_slots as usize).min(MAX_SLOTS);
        let mut s = 0;
        while s < limit {
            if self.ways[s].resident_page == page_id {
                self.access_clock = self.access_clock.wrapping_add(1);
                self.ways[s].last_access_tick = self.access_clock;
                self.hit_count = self.hit_count.saturating_add(1);
                return Some((s, self.ways[s].loaded_size, self.ways[s].flags));
            }
            s += 1;
        }
        None
    }

    /// Resolve page descriptor and allocate a cache way on miss (or opportunistic prefetch when `!allow_evict`).
    pub fn alloc_way_for_miss<F>(
        &mut self,
        page_id: u16,
        allow_evict: bool,
        mut read_block_entry: F,
    ) -> Result<(usize, StreamedPageDesc), PagedError>
    where
        F: FnMut(u32) -> Option<BlockEntry>,
    {
        if page_id >= self.page_count {
            return Err(PagedError::InvalidBlockId);
        }
        let limit = (self.active_slots as usize).min(MAX_SLOTS);
        let mut victim = MAX_SLOTS;
        let mut oldest_age: u16 = 0;
        let mut has_empty = false;
        let mut s = 0;
        while s < limit {
            let way = &self.ways[s];
            if way.resident_page == page_id {
                return Err(PagedError::NoUnpinnedSlotAvailable);
            }
            if !has_empty {
                if way.is_empty() {
                    victim = s;
                    has_empty = true;
                } else if allow_evict && !way.is_pinned() {
                    let age = self
                        .access_clock
                        .wrapping_add(1)
                        .wrapping_sub(way.last_access_tick);
                    if victim == MAX_SLOTS || age >= oldest_age {
                        oldest_age = age;
                        victim = s;
                    }
                }
            }
            s += 1;
        }
        if victim >= limit {
            return Err(PagedError::NoUnpinnedSlotAvailable);
        }

        let page_bytes = 1u16 << self.page_shift;
        let desc = if self.block_map_flash_offset == 0 {
            let rel = (page_id as u32) << self.page_shift;
            let raw_sz = self
                .code_total_size
                .saturating_sub(rel)
                .min(page_bytes as u32) as u16;
            let pinned = if page_id < 16 && (self.rt_pin_mask & (1u16 << page_id)) != 0 {
                BLOCK_FLAG_PINNED
            } else {
                0
            };
            StreamedPageDesc {
                flash_offset: self.code_flash_offset + rel,
                page_id,
                raw_size: raw_sz,
                exec_size: raw_sz,
                flags: pinned,
                _pad: 0,
            }
        } else {
            let entry_off = self.block_map_flash_offset + ((page_id as u32) << 3);
            let entry = read_block_entry(entry_off).ok_or(PagedError::StorageReadFailed)?;
            let mut flags = entry.flags;
            if page_id < 16 && (self.rt_pin_mask & (1u16 << page_id)) != 0 {
                flags |= BLOCK_FLAG_PINNED;
            }
            let exec_size = if (flags & BLOCK_FLAG_LZSS) != 0 {
                page_bytes
            } else {
                entry.size.min(page_bytes)
            };
            StreamedPageDesc {
                flash_offset: entry.flash_offset,
                page_id,
                raw_size: entry.size.min(page_bytes),
                exec_size,
                flags,
                _pad: 0,
            }
        };

        if allow_evict {
            self.access_clock = self.access_clock.wrapping_add(1);
            self.fault_count = self.fault_count.saturating_add(1);
        }
        self.ways[victim] = CacheWayMeta {
            resident_page: page_id,
            loaded_size: desc.exec_size,
            last_access_tick: self.access_clock,
            flags: desc.flags,
            _pad: 0,
        };
        Ok((victim, desc))
    }

    #[inline(always)]
    pub fn invalidate_way(&mut self, slot_idx: usize) {
        if slot_idx < MAX_SLOTS {
            self.ways[slot_idx] = CacheWayMeta::empty();
        }
    }
}

/// Bounded zero-allocation Shadow Return Stack for demand-paged return safety.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct ShadowReturnStack<const DEPTH: usize = 8, F: Copy = PageReturnFrame> {
    pub sp: usize,
    pub frames: [F; DEPTH],
}

impl<const DEPTH: usize> Default for ShadowReturnStack<DEPTH, PageReturnFrame> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const DEPTH: usize> ShadowReturnStack<DEPTH, PageReturnFrame> {
    pub const fn new() -> Self {
        Self {
            sp: 0,
            frames: [const { PageReturnFrame::empty() }; DEPTH],
        }
    }

    pub const fn new_paged() -> Self {
        Self::new()
    }
}

impl<const DEPTH: usize, F: Copy> ShadowReturnStack<DEPTH, F> {
    pub fn clear(&mut self) {
        self.sp = 0;
    }

    pub fn push(&mut self, frame: F) -> Result<(), PagedError> {
        if self.sp >= DEPTH {
            return Err(PagedError::ShadowStackOverflow);
        }
        self.frames[self.sp] = frame;
        self.sp += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Result<F, PagedError> {
        if self.sp == 0 {
            return Err(PagedError::ShadowStackUnderflow);
        }
        self.sp -= 1;
        Ok(self.frames[self.sp])
    }
}

/// Compact 8-byte in-RAM descriptor for a P0 leaf overlay (halves `StructuredLeafManager` SRAM).
///
/// Packs `size` (12b, up to 4095 B), `entry_offset` (12b, up to 4095 B), `flags` (8b),
/// `flash_offset` (24b, up to 16 MB), and `leaf_id` (8b) into two 32-bit words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct PackedLeaf {
    word0: u32,
    word1: u32,
}

const _: () = assert!(core::mem::size_of::<PackedLeaf>() == 8);

impl PackedLeaf {
    pub const fn empty() -> Self {
        Self { word0: 0, word1: 0 }
    }

    #[inline(always)]
    pub const fn from_entry(entry: LeafEntry) -> Self {
        let word0 = ((entry.size as u32) & 0x0FFF)
            | (((entry.entry_offset as u32) & 0x0FFF) << 12)
            | ((entry.flags as u32) << 24);
        let word1 = (entry.flash_offset & 0x00FF_FFFF) | ((entry.leaf_id as u32) << 24);
        Self { word0, word1 }
    }

    #[inline(always)]
    pub const fn leaf_id(self) -> u8 {
        (self.word1 >> 24) as u8
    }

    #[inline(always)]
    pub const fn to_entry(self) -> LeafEntry {
        LeafEntry {
            leaf_id: self.leaf_id(),
            flags: (self.word0 >> 24) as u8,
            size: (self.word0 & 0x0FFF) as u16,
            flash_offset: self.word1 & 0x00FF_FFFF,
            entry_offset: ((self.word0 >> 12) & 0x0FFF) as u16,
            ram_scratch_bytes: 0,
            reserved: [0; 4],
        }
    }
}

/// Streaming flash-backed P0 leaf overlay state (24 bytes in RAM, up to 255 leaves).
///
/// Eliminates fixed `MAX_STRUCTURED_LEAVES` SRAM tables: cache hits dispatch directly from
/// `active_entry` in 0 Flash reads, while misses perform an O(1) power-of-two 16-byte lookup
/// at `table_flash_offset + ((leaf_id - 1) << 4)` (with linear fallback for non-sequential IDs).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct StreamingLeafState {
    pub leaf_count: u8,
    pub is_dispatching: bool,
    pub active_leaf: Option<u8>,
    pub window_size: u16,
    pub total_swaps: u16,
    pub overlay_vma: u32,
    pub table_flash_offset: u32,
    pub active_entry: PackedLeaf,
}

impl StreamingLeafState {
    pub const fn new(overlay_vma: u32, window_size: u16) -> Self {
        Self {
            leaf_count: 0,
            is_dispatching: false,
            active_leaf: None,
            window_size,
            total_swaps: 0,
            overlay_vma,
            table_flash_offset: 0,
            active_entry: PackedLeaf::empty(),
        }
    }

    pub fn configure(
        &mut self,
        overlay_vma: u32,
        window_size: u16,
        table_flash_offset: u32,
        leaf_count: u8,
    ) {
        self.overlay_vma = overlay_vma;
        self.window_size = window_size;
        self.table_flash_offset = table_flash_offset;
        self.leaf_count = leaf_count;
        self.active_leaf = None;
        self.is_dispatching = false;
        self.total_swaps = 0;
        self.active_entry = PackedLeaf::empty();
    }

    /// Resolve `leaf_id` using cached `active_entry` on hit, or `read_entry(flash_offset)` on miss.
    /// Returns `(LeafEntry, is_cache_hit)`.
    pub fn begin_leaf_call<F>(
        &mut self,
        leaf_id: u8,
        mut read_entry: F,
    ) -> Result<(LeafEntry, bool), PagedError>
    where
        F: FnMut(u32) -> Option<LeafEntry>,
    {
        if self.is_dispatching {
            return Err(PagedError::ReentrantDispatchDenied);
        }
        if leaf_id == 0 || self.leaf_count == 0 {
            return Err(PagedError::InvalidLeafId);
        }
        if self.active_leaf == Some(leaf_id) {
            self.is_dispatching = true;
            return Ok((self.active_entry.to_entry(), true));
        }
        // O(1) direct slot probe for sequential 1-based leaf IDs (`16 B == 1 << 4`),
        // falling back to linear scan only if a package uses sparse IDs.
        let mut found = None;
        let idx = (leaf_id - 1) as u32;
        if idx < self.leaf_count as u32
            && let Some(e) = read_entry(self.table_flash_offset + (idx << 4))
            && e.leaf_id == leaf_id
        {
            found = Some(e);
        }
        if found.is_none() {
            for i in 0..(self.leaf_count as u32) {
                if i == idx {
                    continue;
                }
                if let Some(e) = read_entry(self.table_flash_offset + (i << 4))
                    && e.leaf_id == leaf_id
                {
                    found = Some(e);
                    break;
                }
            }
        }
        let entry = found.ok_or(PagedError::InvalidLeafId)?;
        self.active_entry = PackedLeaf::from_entry(entry);
        self.active_leaf = Some(leaf_id);
        self.total_swaps = self.total_swaps.saturating_add(1);
        self.is_dispatching = true;
        Ok((entry, false))
    }
}

/// P0 Structured Paged Overlay Manager (Resident Root + Leaf Overlays, §5.1 / KRX 3.2).
///
/// Enforces:
/// 1. Only resident root directly calls an overlay leaf.
/// 2. A leaf runs to completion (run-to-return).
/// 3. Zero direct leaf-to-leaf calls.
/// 4. Leaf requests another leaf via typed `LeafContinuation`.
/// 5. Reentrant overlay dispatch attempts fail deterministically (`ReentrantDispatchDenied`).
#[derive(Debug, Clone)]
#[repr(C)]
pub struct StructuredLeafManager<const MAX_LEAVES: usize = 16> {
    pub leaf_count: u8,
    pub is_dispatching: bool,
    pub active_leaf: Option<u8>,
    pub window_size: u16,
    pub total_swaps: u16,
    pub overlay_vma: u32,
    pub leaves: [PackedLeaf; MAX_LEAVES],
}

impl<const MAX_LEAVES: usize> Default for StructuredLeafManager<MAX_LEAVES> {
    fn default() -> Self {
        Self::new(0, 1024)
    }
}

impl<const MAX_LEAVES: usize> StructuredLeafManager<MAX_LEAVES> {
    pub const fn new(overlay_vma: u32, window_size: u16) -> Self {
        Self {
            overlay_vma,
            window_size,
            total_swaps: 0,
            leaf_count: 0,
            is_dispatching: false,
            active_leaf: None,
            leaves: [const { PackedLeaf::empty() }; MAX_LEAVES],
        }
    }

    pub fn reset(&mut self) {
        self.leaf_count = 0;
        self.active_leaf = None;
        self.is_dispatching = false;
        self.total_swaps = 0;
    }

    /// Configure the structured leaf manager and register leaves from `SectionLeafMap (0x0006)`.
    pub fn configure(&mut self, overlay_vma: u32, window_size: u16, leaf_map_body: Option<&[u8]>) {
        self.reset();
        self.overlay_vma = overlay_vma;
        self.window_size = window_size;
        if let Some(leaf_body) = leaf_map_body {
            for (dst, chunk) in self
                .leaves
                .iter_mut()
                .zip(leaf_body.chunks_exact(LeafEntry::SIZE))
            {
                if let Some(entry) = LeafEntry::parse(chunk) {
                    *dst = PackedLeaf::from_entry(entry);
                    self.leaf_count += 1;
                }
            }
        }
    }

    pub fn register_leaf(&mut self, entry: LeafEntry) -> Result<(), PagedError> {
        if (self.leaf_count as usize) < MAX_LEAVES {
            self.leaves[self.leaf_count as usize] = PackedLeaf::from_entry(entry);
            self.leaf_count += 1;
            Ok(())
        } else {
            Err(PagedError::InvalidLeafId)
        }
    }

    pub fn find_leaf(&self, leaf_id: u8) -> Option<LeafEntry> {
        for i in 0..(self.leaf_count as usize) {
            if self.leaves[i].leaf_id() == leaf_id {
                return Some(self.leaves[i].to_entry());
            }
        }
        None
    }

    /// Pre-flight check and acquire overlay execution lock for a leaf call.
    /// Returns `(leaf_entry, is_cache_hit)`.
    pub fn begin_leaf_call(&mut self, leaf_id: u8) -> Result<(LeafEntry, bool), PagedError> {
        if self.is_dispatching {
            return Err(PagedError::ReentrantDispatchDenied);
        }
        let count = (self.leaf_count as usize).min(MAX_LEAVES);
        let mut found = None;
        for i in 0..count {
            if self.leaves[i].leaf_id() == leaf_id {
                found = Some(i);
                break;
            }
        }
        let leaf_idx = found.ok_or(PagedError::InvalidLeafId)?;

        let is_hit = self.active_leaf == Some(leaf_id);
        if !is_hit {
            self.active_leaf = Some(leaf_id);
            self.total_swaps = self.total_swaps.saturating_add(1);
        }
        self.is_dispatching = true;
        Ok((self.leaves[leaf_idx].to_entry(), is_hit))
    }

    /// Completes the active leaf call and releases the overlay execution lock.
    pub fn end_leaf_call(
        &mut self,
        continuation: LeafContinuation,
    ) -> Result<LeafContinuation, PagedError> {
        self.is_dispatching = false;
        Ok(continuation)
    }

    /// Computes the executable Thumb-1 entry address for a leaf inside the overlay window.
    #[inline(always)]
    pub fn leaf_entry_address(&self, entry: &LeafEntry) -> usize {
        ((self.overlay_vma + entry.entry_offset as u32) | 1) as usize
    }

    #[inline(always)]
    pub const fn active_leaf_id(&self) -> Option<u8> {
        self.active_leaf
    }

    #[inline(always)]
    pub const fn leaf_count(&self) -> usize {
        self.leaf_count as usize
    }

    #[inline(always)]
    pub fn leaf_at(&self, idx: usize) -> Option<LeafEntry> {
        if idx < (self.leaf_count as usize) {
            Some(self.leaves[idx].to_entry())
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn total_swaps(&self) -> u32 {
        self.total_swaps as u32
    }

    /// Dispatches a structured leaf overlay call and processes returned continuations
    /// with trampoline chaining up to `max_hops` to prevent Cortex-M0+ stack recursion.
    pub fn dispatch_leaf_trampoline<F>(
        &mut self,
        initial_leaf_id: u8,
        initial_arg: u32,
        max_hops: u8,
        mut runner: F,
    ) -> Result<u32, PagedError>
    where
        F: FnMut(&LeafEntry, bool, &mut LeafContext) -> Result<u32, PagedError>,
    {
        let mut current_id = initial_leaf_id;
        let mut current_arg = initial_arg;
        let mut hops = 0;

        while hops < max_hops {
            hops += 1;
            let (leaf_entry, is_hit) = self.begin_leaf_call(current_id)?;

            let mut ctx = LeafContext::new(current_id, current_arg);
            let raw_res = runner(&leaf_entry, is_hit, &mut ctx)?;

            // If continuation was not explicitly modified, treat as DONE with raw_res
            let mut cont = ctx.continuation;
            if cont.is_done() && cont.payload == 0 {
                cont.payload = raw_res;
                ctx.continuation = cont;
            }

            let end_cont = self.end_leaf_call(cont)?;
            if end_cont.is_call_leaf() {
                current_id = end_cont.target_or_code;
                current_arg = end_cont.payload;
                continue;
            } else if end_cont.is_error() {
                return Err(PagedError::LeafExecutionFailed);
            } else {
                return Ok(end_cont.payload);
            }
        }

        Err(PagedError::MaxHopsExceeded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::FLAG_LEAF_RT;

    #[test]
    fn test_shadow_return_stack_lr_thunk() {
        let mut stk = ShadowReturnStack::<4>::new();
        let frame = PageReturnFrame {
            caller_page_id: 1,
            return_offset: 0x0048,
        };
        assert_eq!(stk.push(frame), Ok(()));
        assert_eq!(stk.pop(), Ok(frame));
        assert_eq!(stk.pop(), Err(PagedError::ShadowStackUnderflow));
    }

    #[test]
    fn test_paged_nested_call_and_return_thunk_eviction() {
        let mut cache = StreamedWindowCache::<8>::new();
        cache.configure(512, 2, 1024, 0, 0, 0, 2048, 0, 0);
        let mut shadow_stack = ShadowReturnStack::<8>::new();

        let (s0, _) = cache.alloc_way_for_miss(0, true, |_| None).unwrap();
        assert_eq!(s0, 0);

        shadow_stack
            .push(PageReturnFrame {
                caller_page_id: 0,
                return_offset: 0x0020,
            })
            .unwrap();
        let (s1, _) = cache.alloc_way_for_miss(1, true, |_| None).unwrap();
        assert_eq!(s1, 1);

        shadow_stack
            .push(PageReturnFrame {
                caller_page_id: 1,
                return_offset: 0x0050,
            })
            .unwrap();
        let (s2, _) = cache.alloc_way_for_miss(2, true, |_| None).unwrap();
        assert_eq!(s2, 0);
        assert_eq!(cache.ways[0].resident_page, 2);
        assert_eq!(cache.ways[1].resident_page, 1);

        let frame1 = shadow_stack.pop().unwrap();
        assert_eq!(frame1.caller_page_id, 1);
        assert_eq!(frame1.return_offset, 0x0050);
        assert_eq!(cache.probe_hit(frame1.caller_page_id).unwrap().0, 1);

        let frame0 = shadow_stack.pop().unwrap();
        assert_eq!(frame0.caller_page_id, 0);
        assert_eq!(frame0.return_offset, 0x0020);
        assert!(cache.probe_hit(frame0.caller_page_id).is_none());
        let (s0_res, _) = cache
            .alloc_way_for_miss(frame0.caller_page_id, true, |_| None)
            .unwrap();
        assert_eq!(cache.ways[s0_res].resident_page, 0);
    }

    #[test]
    fn test_structured_leaf_manager_dispatch_and_continuations() {
        let mut mgr: StructuredLeafManager<4> = StructuredLeafManager::new(0x2000_1000, 1024);

        let leaf0 = LeafEntry {
            leaf_id: 1,
            flags: 0,
            size: 512,
            flash_offset: 0x1000,
            entry_offset: 0x0010,
            ram_scratch_bytes: 64,
            reserved: [0u8; 4],
        };
        let leaf1 = LeafEntry {
            leaf_id: 2,
            flags: FLAG_LEAF_RT,
            size: 1024,
            flash_offset: 0x1400,
            entry_offset: 0x0020,
            ram_scratch_bytes: 128,
            reserved: [0u8; 4],
        };

        assert!(mgr.register_leaf(leaf0).is_ok());
        assert!(mgr.register_leaf(leaf1).is_ok());

        assert_eq!(mgr.leaf_entry_address(&leaf0), 0x2000_1011);

        // 1. Begin leaf call to leaf 1
        let (entry, hit) = mgr.begin_leaf_call(1).unwrap();
        assert_eq!(entry.leaf_id, 1);
        assert!(!hit); // first call: cache miss / swap

        // 2. Reentrant leaf call while dispatching MUST fail
        assert_eq!(
            mgr.begin_leaf_call(2).unwrap_err(),
            PagedError::ReentrantDispatchDenied
        );

        // 3. Leaf 1 completes and returns CallLeaf continuation to leaf 2
        let cont = mgr
            .end_leaf_call(LeafContinuation::call_leaf(2, 42))
            .unwrap();
        assert!(cont.is_call_leaf());
        assert_eq!(cont.target_or_code, 2);
        assert_eq!(cont.payload, 42);

        // 4. Resident root handles continuation by invoking leaf 2
        let (entry2, hit2) = mgr.begin_leaf_call(2).unwrap();
        assert_eq!(entry2.leaf_id, 2);
        assert!(!hit2); // swap to leaf 2
        assert_eq!(mgr.total_swaps, 2);

        // Leaf 2 finishes with Done(0)
        let cont2 = mgr.end_leaf_call(LeafContinuation::done(0)).unwrap();
        assert!(cont2.is_done());
        assert_eq!(cont2.payload, 0);

        // 5. Subsequent call to leaf 2 is a cache hit!
        let (_, hit2_again) = mgr.begin_leaf_call(2).unwrap();
        assert!(hit2_again);
        assert_eq!(mgr.total_swaps, 2); // no extra swap
        let _ = mgr.end_leaf_call(LeafContinuation::done(1));

        // 6. Test dispatch_leaf_trampoline chaining: Leaf 1 -> Leaf 2 -> Done(150)
        let tramp_res = mgr.dispatch_leaf_trampoline(1, 10, 4, |entry, _hit, ctx| {
            let arg = ctx.arg;
            if entry.leaf_id == 1 {
                ctx.continuation = LeafContinuation::call_leaf(2, arg + 5);
                Ok(0)
            } else if entry.leaf_id == 2 {
                ctx.continuation = LeafContinuation::done(arg * 10);
                Ok(0)
            } else {
                Err(PagedError::InvalidLeafId)
            }
        });
        assert_eq!(tramp_res.unwrap(), 150);

        // 7. Test trampoline max_hops exceeded
        let loop_res = mgr.dispatch_leaf_trampoline(1, 0, 3, |entry, _hit, ctx| {
            // Ping-pong between leaf 1 and leaf 2
            let next = if entry.leaf_id == 1 { 2 } else { 1 };
            ctx.continuation = LeafContinuation::call_leaf(next, ctx.arg + 1);
            Ok(0)
        });
        assert_eq!(loop_res.unwrap_err(), PagedError::MaxHopsExceeded);
    }

    #[test]
    fn test_structured_leaf_manager_configure_from_slice() {
        let mut raw_leaf_map = [0u8; 32];
        // Leaf 1
        raw_leaf_map[0] = 1; // leaf_id
        raw_leaf_map[1] = 0; // flags
        raw_leaf_map[2..4].copy_from_slice(&512u16.to_le_bytes()); // size = 512
        raw_leaf_map[4..8].copy_from_slice(&0x2000u32.to_le_bytes()); // flash_offset
        raw_leaf_map[8..10].copy_from_slice(&0x0010u16.to_le_bytes()); // entry_offset
        raw_leaf_map[10..12].copy_from_slice(&64u16.to_le_bytes()); // ram_scratch

        // Leaf 2
        raw_leaf_map[16] = 2; // leaf_id
        raw_leaf_map[17] = FLAG_LEAF_RT; // flags
        raw_leaf_map[18..20].copy_from_slice(&1024u16.to_le_bytes()); // size = 1024
        raw_leaf_map[20..24].copy_from_slice(&0x2400u32.to_le_bytes()); // flash_offset
        raw_leaf_map[24..26].copy_from_slice(&0x0020u16.to_le_bytes()); // entry_offset
        raw_leaf_map[26..28].copy_from_slice(&128u16.to_le_bytes()); // ram_scratch

        let mut mgr: StructuredLeafManager<4> = StructuredLeafManager::new(0, 0);
        mgr.configure(0x2000_1000, 1024, Some(&raw_leaf_map));

        assert_eq!(mgr.leaf_count(), 2);
        assert_eq!(mgr.overlay_vma, 0x2000_1000);
        assert_eq!(mgr.window_size, 1024);

        let leaf1 = mgr.find_leaf(1).expect("Leaf 1 must be found");
        let l1_size = leaf1.size;
        let l1_flash = leaf1.flash_offset;
        let l1_entry = leaf1.entry_offset;
        assert_eq!(l1_size, 512);
        assert_eq!(l1_flash, 0x2000);
        assert_eq!(l1_entry, 0x0010);

        let leaf2 = mgr.find_leaf(2).expect("Leaf 2 must be found");
        let l2_size = leaf2.size;
        let l2_flash = leaf2.flash_offset;
        assert_eq!(l2_size, 1024);
        assert_eq!(leaf2.flags, FLAG_LEAF_RT);
        assert_eq!(l2_flash, 0x2400);

        assert!(mgr.find_leaf(3).is_none());
    }

    #[test]
    fn streaming_leaf_state_handles_large_leaf_tables_and_dynamic_windows() {
        // Verify StreamingLeafState stays 24 bytes in RAM and dispatches >16 leaves in O(1).
        assert_eq!(core::mem::size_of::<StreamingLeafState>(), 24);

        const NUM_LEAVES: usize = 32;
        let mut raw_table = [0u8; NUM_LEAVES * LeafEntry::SIZE];
        for i in 0..NUM_LEAVES {
            let base = i * LeafEntry::SIZE;
            let leaf_id = (i + 1) as u8;
            let size = (512 + (i % 4) * 512) as u16; // up to 2048 B dynamic window
            let flash_off = 0x1000u32 + (i as u32) * 0x800;
            raw_table[base] = leaf_id;
            raw_table[base + 1] = 0;
            raw_table[base + 2..base + 4].copy_from_slice(&size.to_le_bytes());
            raw_table[base + 4..base + 8].copy_from_slice(&flash_off.to_le_bytes());
            raw_table[base + 8..base + 10].copy_from_slice(&0x0000u16.to_le_bytes());
        }

        let mut state = StreamingLeafState::new(0, 0);
        state.configure(0x2000_2000, 2048, 0x0100, NUM_LEAVES as u8);

        let mut probes = 0u32;
        let read_entry = |flash_off: u32, probes: &mut u32| -> Option<LeafEntry> {
            *probes += 1;
            let rel = flash_off.checked_sub(0x0100)? as usize;
            if rel + LeafEntry::SIZE <= raw_table.len() {
                LeafEntry::parse(&raw_table[rel..rel + LeafEntry::SIZE])
            } else {
                None
            }
        };

        // Call leaf 24 (beyond old 16-leaf table) -> 1 O(1) probe, miss.
        let (e24, hit24) = state
            .begin_leaf_call(24, |off| read_entry(off, &mut probes))
            .unwrap();
        assert!(!hit24);
        assert_eq!(e24.leaf_id, 24);
        assert_eq!(probes, 1);
        assert_eq!(state.total_swaps, 1);

        // Reentrant call while dispatching must be rejected.
        assert_eq!(
            state
                .begin_leaf_call(25, |off| read_entry(off, &mut probes))
                .unwrap_err(),
            PagedError::ReentrantDispatchDenied
        );
        state.is_dispatching = false;

        // Second call to leaf 24 -> cache hit with 0 Flash reads.
        let (e24_hit, hit24_again) = state
            .begin_leaf_call(24, |off| read_entry(off, &mut probes))
            .unwrap();
        assert!(hit24_again);
        assert_eq!(e24_hit.leaf_id, 24);
        assert_eq!(probes, 1);
        assert_eq!(state.total_swaps, 1);
        state.is_dispatching = false;

        // Out-of-range leaf ID fails deterministically.
        assert_eq!(
            state
                .begin_leaf_call(33, |off| read_entry(off, &mut probes))
                .unwrap_err(),
            PagedError::InvalidLeafId
        );
    }

    #[test]
    fn streamed_window_cache_addresses_384kib_768_pages_with_constant_sram() {
        // 8-way cache metadata + header stays <= 96 bytes while addressing 768 x 512 B = 384 KiB!
        assert!(core::mem::size_of::<StreamedWindowCache<8>>() <= 96);

        let mut cache = StreamedWindowCache::<8>::new();
        cache.configure(
            512,
            6,
            3072,
            512,         // 512 B permanent resident prefix (.data/.bss/.got)
            0b0000_0001, // Pin page 0
            0x0200,
            MAX_STREAMED_IMAGE_BYTES,
            0,
            0,
        );
        assert_eq!(cache.page_count, 768);
        assert_eq!(cache.active_slots, 6);
        assert_eq!(cache.slot_byte_offset(0), 512);
        assert_eq!(cache.slot_byte_offset(5), 512 + 5 * 512);

        // Fault page 0 (pinned) and high page 767 (near 384 KiB ceiling, > u8::MAX!)
        let (w0, d0) = cache.alloc_way_for_miss(0, true, |_| None).unwrap();
        assert_eq!(w0, 0);
        assert_eq!(d0.flash_offset, 0x0200);
        assert_ne!(d0.flags & BLOCK_FLAG_PINNED, 0);

        let (w_hi, d_hi) = cache.alloc_way_for_miss(767, true, |_| None).unwrap();
        assert_eq!(w_hi, 1);
        assert_eq!(d_hi.flash_offset, 0x0200 + 767 * 512);
        assert_eq!(d_hi.exec_size, 512);

        // Hit on high page 767 with zero Flash reads
        let (hit_way, loaded_sz, _) = cache.probe_hit(767).expect("767 must hit");
        assert_eq!((hit_way, loaded_sz), (1, 512));

        // Encode & decode Vfp32 for page 767 offset 0x01AC
        let vfp = ::abi::Vfp32::encode_paged(767, 0x01AC);
        assert!(vfp.is_virtual());
        assert_eq!(vfp.page_id(), 767);
        assert_eq!(vfp.offset(), 0x01AC);

        // Fill remaining ways 2..6 and verify page 0 stays pinned on eviction
        for p in [100u16, 200, 300, 400, 500] {
            let _ = cache.alloc_way_for_miss(p, true, |_| None).unwrap();
        }
        assert_eq!(cache.ways[0].resident_page, 0);
        assert!(cache.probe_hit(500).is_some());
    }
}
