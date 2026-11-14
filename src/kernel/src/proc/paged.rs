//! KRX Demand-Paged Native Subsystem (KRX-ABI 2.5.0 Chapter 7).
//!
//! Implements the normative fixed-window overlay cache (`FixedWindowCache`),
//! `SectionBlockMap` (`0x0005`) parser, LRU victim selection with `.krx.pinned`
//! immunity, and the block-relative shadow return stack (`ShadowReturnStack`)
//! for LR-thunking across overlay evictions on Cortex-M0+ (no MMU).

use super::krx::{BlockEntry, LeafContext, LeafContinuation, LeafEntry};

/// Block flag: pinned in cache (never evicted once loaded; e.g., DSP inner loops)
pub const BLOCK_FLAG_PINNED: u8 = 1 << 0;
/// Block flag: real-time bounded latency block
pub const BLOCK_FLAG_RT: u8 = 1 << 1;
/// Block flag: sequential prefetch hint for next block
pub const BLOCK_FLAG_HINT_NEXT: u8 = 1 << 2;

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

    /// Parse an 8-byte `BlockEntry` using explicit little-endian byte decoding (ARMv6-M safe).
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

/// Residency metadata for a single fixed-window cache slot in SRAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheSlotMeta {
    pub resident_block: Option<u8>,
    pub pinned: bool,
    pub last_access_tick: u16,
}

impl CacheSlotMeta {
    pub const fn empty() -> Self {
        Self {
            resident_block: None,
            pinned: false,
            last_access_tick: 0,
        }
    }
}

/// Normative Fixed-Window Overlay Cache Controller (KRX-ABI 2.5.0 Section 7.1).
///
/// Manages 1 or 2 fixed-window cache slots (`512 B` or `1024 B` per slot)
/// at a single fixed cache VMA in the application arena.
#[derive(Debug, Clone)]
pub struct FixedWindowCache<const MAX_BLOCKS: usize = 16, const MAX_SLOTS: usize = 4> {
    pub page_size: u16,
    pub active_slots: u8,
    pub block_count: u8,
    pub blocks: [BlockEntry; MAX_BLOCKS],
    pub slots: [CacheSlotMeta; MAX_SLOTS],
    pub access_clock: u16,
    pub fault_count: u16,
    pub hit_count: u16,
}

impl<const MAX_BLOCKS: usize, const MAX_SLOTS: usize> Default
    for FixedWindowCache<MAX_BLOCKS, MAX_SLOTS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_BLOCKS: usize, const MAX_SLOTS: usize> FixedWindowCache<MAX_BLOCKS, MAX_SLOTS> {
    pub const fn new() -> Self {
        Self {
            page_size: 1024,
            active_slots: 2,
            block_count: 0,
            blocks: [const { BlockEntry::empty() }; MAX_BLOCKS],
            slots: [const { CacheSlotMeta::empty() }; MAX_SLOTS],
            access_clock: 0,
            fault_count: 0,
            hit_count: 0,
        }
    }

    pub fn reset(&mut self) {
        self.page_size = 1024;
        self.active_slots = 2;
        self.block_count = 0;
        self.slots = [const { CacheSlotMeta::empty() }; MAX_SLOTS];
        self.access_clock = 0;
        self.fault_count = 0;
        self.hit_count = 0;
    }

    /// Configure the fixed-window cache from `SectionExecution` and optional `SectionBlockMap (0x0005)`.
    /// If no explicit `BlockMap` section is present, partitions the payload into uniform `page_size` blocks.
    pub fn configure(
        &mut self,
        page_size: u16,
        cache_blocks: u8,
        payload_flash_offset: u32,
        payload_size: u32,
        block_map_body: Option<&[u8]>,
    ) {
        self.reset();
        let eff_page = if page_size == 512 || page_size == 1024 {
            page_size
        } else {
            1024
        };
        self.page_size = eff_page;
        self.active_slots = cache_blocks.clamp(1, MAX_SLOTS as u8);

        if let Some(body) = block_map_body {
            for (dst, chunk) in self
                .blocks
                .iter_mut()
                .zip(body.chunks_exact(BlockEntry::SIZE))
            {
                if let Some(entry) = BlockEntry::parse(chunk) {
                    *dst = entry;
                    self.block_count += 1;
                }
            }
        }

        if self.block_count == 0 && payload_size > 0 {
            let step = eff_page as u32;
            let mut rem = payload_size;
            let mut cur_off = payload_flash_offset;
            let mut idx = 0usize;
            while rem > 0 && idx < MAX_BLOCKS {
                let chunk = rem.min(step) as u16;
                self.blocks[idx] = BlockEntry {
                    block_id: idx as u8,
                    flags: 0,
                    size: chunk,
                    flash_offset: cur_off,
                };
                cur_off += chunk as u32;
                rem -= chunk as u32;
                idx += 1;
                self.block_count += 1;
            }
        }
    }

    /// Lookup a block descriptor by `block_id`.
    pub fn get_block(&self, block_id: u8) -> Option<BlockEntry> {
        let count = self.block_count as usize;
        for i in 0..count {
            if self.blocks[i].block_id == block_id {
                return Some(self.blocks[i]);
            }
        }
        None
    }

    /// Resolve a block in the cache or select a victim slot for page-in.
    /// Returns `(slot_index, is_cache_hit, block_entry)`.
    #[inline(never)]
    pub fn resolve_or_select_slot(
        &mut self,
        block_id: u8,
    ) -> Result<(usize, bool, BlockEntry), PagedError> {
        let entry = self.get_block(block_id).ok_or(PagedError::InvalidBlockId)?;

        self.access_clock = self.access_clock.wrapping_add(1);
        let slot_limit = (self.active_slots as usize).min(MAX_SLOTS);

        // 1. Check for cache hit
        for s in 0..slot_limit {
            if self.slots[s].resident_block == Some(block_id) {
                self.slots[s].last_access_tick = self.access_clock;
                self.hit_count = self.hit_count.saturating_add(1);
                return Ok((s, true, entry));
            }
        }

        // 2. Cache miss (Page Fault): prefer an empty slot, else evict LRU unpinned slot
        let mut victim: Option<usize> = None;
        let mut oldest_age: u16 = 0;
        for s in 0..slot_limit {
            if self.slots[s].resident_block.is_none() {
                victim = Some(s);
                break;
            }
            if !self.slots[s].pinned {
                let age = self
                    .access_clock
                    .wrapping_sub(self.slots[s].last_access_tick);
                if victim.is_none() || age >= oldest_age {
                    oldest_age = age;
                    victim = Some(s);
                }
            }
        }

        let slot_idx = victim.ok_or(PagedError::NoUnpinnedSlotAvailable)?;
        self.slots[slot_idx] = CacheSlotMeta {
            resident_block: Some(block_id),
            pinned: entry.is_pinned(),
            last_access_tick: self.access_clock,
        };
        self.fault_count = self.fault_count.saturating_add(1);
        Ok((slot_idx, false, entry))
    }

    /// Byte offset of `slot_idx` relative to the start of the fixed-window cache VMA.
    #[inline(always)]
    pub const fn slot_byte_offset(&self, slot_idx: usize) -> usize {
        if self.page_size == 512 {
            slot_idx << 9
        } else {
            slot_idx << 10
        }
    }
}

/// Shadow return frame for LR-thunking across paged block calls (KRX-ABI 2.5.0 Section 7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(4))]
pub struct ReturnFrame {
    pub caller_block_id: u8,
    pub caller_slot: u8,
    pub return_offset: u16,
}

const _: () = {
    assert!(core::mem::size_of::<ReturnFrame>() == 4);
    assert!(core::mem::align_of::<ReturnFrame>() == 4);
};

impl ReturnFrame {
    pub const fn empty() -> Self {
        Self {
            caller_block_id: 0,
            caller_slot: 0,
            return_offset: 0,
        }
    }
}

/// Bounded zero-allocation Shadow Return Stack for demand-paged return safety.
#[derive(Debug, Clone)]
pub struct ShadowReturnStack<const DEPTH: usize = 8> {
    pub frames: [ReturnFrame; DEPTH],
    pub sp: usize,
}

impl<const DEPTH: usize> Default for ShadowReturnStack<DEPTH> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const DEPTH: usize> ShadowReturnStack<DEPTH> {
    pub const fn new() -> Self {
        Self {
            frames: [const { ReturnFrame::empty() }; DEPTH],
            sp: 0,
        }
    }

    pub fn clear(&mut self) {
        self.sp = 0;
    }

    pub fn push(&mut self, frame: ReturnFrame) -> Result<(), PagedError> {
        if self.sp >= DEPTH {
            return Err(PagedError::ShadowStackOverflow);
        }
        self.frames[self.sp] = frame;
        self.sp += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Result<ReturnFrame, PagedError> {
        if self.sp == 0 {
            return Err(PagedError::ShadowStackUnderflow);
        }
        self.sp -= 1;
        Ok(self.frames[self.sp])
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
pub struct StructuredLeafManager<const MAX_LEAVES: usize = 16> {
    pub overlay_vma: u32,
    pub window_size: u16,
    pub leaves: [LeafEntry; MAX_LEAVES],
    pub leaf_count: u8,
    pub active_leaf: Option<u8>,
    pub is_dispatching: bool,
    pub total_swaps: u16,
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
            leaves: [const { LeafEntry::empty() }; MAX_LEAVES],
            leaf_count: 0,
            active_leaf: None,
            is_dispatching: false,
            total_swaps: 0,
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
                    *dst = entry;
                    self.leaf_count += 1;
                }
            }
        }
    }

    pub fn register_leaf(&mut self, entry: LeafEntry) -> Result<(), PagedError> {
        if (self.leaf_count as usize) < MAX_LEAVES {
            self.leaves[self.leaf_count as usize] = entry;
            self.leaf_count += 1;
            Ok(())
        } else {
            Err(PagedError::InvalidLeafId)
        }
    }

    pub fn find_leaf(&self, leaf_id: u8) -> Option<&LeafEntry> {
        for i in 0..(self.leaf_count as usize) {
            if self.leaves[i].leaf_id == leaf_id {
                return Some(&self.leaves[i]);
            }
        }
        None
    }

    /// Pre-flight check and acquire overlay execution lock for a leaf call.
    /// Returns `(leaf_entry, is_cache_hit)`.
    pub fn begin_leaf_call(&mut self, leaf_id: u8) -> Result<(&LeafEntry, bool), PagedError> {
        if self.is_dispatching {
            return Err(PagedError::ReentrantDispatchDenied);
        }
        let leaf_idx = (0..(self.leaf_count as usize))
            .find(|&i| self.leaves[i].leaf_id == leaf_id)
            .ok_or(PagedError::InvalidLeafId)?;

        let is_hit = self.active_leaf == Some(leaf_id);
        if !is_hit {
            self.active_leaf = Some(leaf_id);
            self.total_swaps = self.total_swaps.saturating_add(1);
        }
        self.is_dispatching = true;
        Ok((&self.leaves[leaf_idx], is_hit))
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
    pub fn leaf_at(&self, idx: usize) -> Option<&LeafEntry> {
        if idx < (self.leaf_count as usize) {
            Some(&self.leaves[idx])
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
            let (leaf_entry, is_hit) = {
                let (entry, hit) = self.begin_leaf_call(current_id)?;
                (*entry, hit)
            };

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
    use crate::proc::FLAG_LEAF_RT;

    #[test]
    fn test_fixed_window_cache_lru_and_pinned() {
        let mut cache = FixedWindowCache::<8, 2>::new();
        cache.configure(512, 2, 128, 2048, None);
        assert_eq!(cache.block_count, 4);

        // Pin block 0
        cache.blocks[0].flags = BLOCK_FLAG_PINNED;

        // Fault in block 0 -> slot 0 (pinned)
        let (s0, hit0, _) = cache.resolve_or_select_slot(0).unwrap();
        assert_eq!((s0, hit0), (0, false));

        // Fault in block 1 -> slot 1 (unpinned)
        let (s1, hit1, _) = cache.resolve_or_select_slot(1).unwrap();
        assert_eq!((s1, hit1), (1, false));

        // Access block 0 again -> cache hit on slot 0
        let (s0_hit, hit0_again, _) = cache.resolve_or_select_slot(0).unwrap();
        assert_eq!((s0_hit, hit0_again), (0, true));

        // Fault in block 2 -> must evict slot 1 because slot 0 is PINNED
        let (s2, hit2, _) = cache.resolve_or_select_slot(2).unwrap();
        assert_eq!((s2, hit2), (1, false));
        assert_eq!(cache.slots[0].resident_block, Some(0));
        assert_eq!(cache.slots[1].resident_block, Some(2));
        assert_eq!(cache.fault_count, 3);
        assert_eq!(cache.hit_count, 1);
    }

    #[test]
    fn test_shadow_return_stack_lr_thunk() {
        let mut stk = ShadowReturnStack::<4>::new();
        let frame = ReturnFrame {
            caller_block_id: 1,
            caller_slot: 0,
            return_offset: 0x0048,
        };
        assert_eq!(stk.push(frame), Ok(()));
        assert_eq!(stk.pop(), Ok(frame));
        assert_eq!(stk.pop(), Err(PagedError::ShadowStackUnderflow));
    }

    #[test]
    fn test_paged_nested_call_and_return_thunk_eviction() {
        let mut cache = FixedWindowCache::<8, 2>::new();
        // 4 blocks of 512B each
        cache.configure(512, 2, 0, 2048, None);
        let mut shadow_stack = ShadowReturnStack::<8>::new();

        // 1. App starts in Block 0 (resident in Slot 0)
        let (s0, _, _) = cache.resolve_or_select_slot(0).unwrap();
        assert_eq!(s0, 0);

        // 2. Block 0 calls function in Block 1 (returns to Block 0 offset 0x0020)
        shadow_stack
            .push(ReturnFrame {
                caller_block_id: 0,
                caller_slot: s0 as u8,
                return_offset: 0x0020,
            })
            .unwrap();
        let (s1, hit1, _) = cache.resolve_or_select_slot(1).unwrap();
        assert_eq!((s1, hit1), (1, false)); // Slot 1 populated

        // 3. Block 1 calls function in Block 2 (returns to Block 1 offset 0x0050)
        shadow_stack
            .push(ReturnFrame {
                caller_block_id: 1,
                caller_slot: s1 as u8,
                return_offset: 0x0050,
            })
            .unwrap();
        // Eviction happens! Block 0 was LRU, so Block 2 evicts Slot 0!
        let (s2, hit2, _) = cache.resolve_or_select_slot(2).unwrap();
        assert_eq!((s2, hit2), (0, false));
        assert_eq!(cache.slots[0].resident_block, Some(2));
        assert_eq!(cache.slots[1].resident_block, Some(1));

        // 4. Block 2 returns: pop shadow stack -> resumes Block 1
        let frame1 = shadow_stack.pop().unwrap();
        assert_eq!(frame1.caller_block_id, 1);
        assert_eq!(frame1.return_offset, 0x0050);
        // Block 1 is STILL resident in Slot 1 -> Cache Hit!
        let (s1_res, hit1_res, _) = cache
            .resolve_or_select_slot(frame1.caller_block_id)
            .unwrap();
        assert_eq!((s1_res, hit1_res), (1, true));

        // 5. Block 1 returns: pop shadow stack -> resumes Block 0
        let frame0 = shadow_stack.pop().unwrap();
        assert_eq!(frame0.caller_block_id, 0);
        assert_eq!(frame0.return_offset, 0x0020);
        // Block 0 was EVICTED! -> Cache Miss, faults back in!
        let (s0_res, hit0_res, _) = cache
            .resolve_or_select_slot(frame0.caller_block_id)
            .unwrap();
        assert_eq!((s0_res, hit0_res), (0, false));
        assert_eq!(cache.slots[0].resident_block, Some(0));
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
        let l1_scratch = leaf1.ram_scratch_bytes;
        assert_eq!(l1_size, 512);
        assert_eq!(l1_flash, 0x2000);
        assert_eq!(l1_entry, 0x0010);
        assert_eq!(l1_scratch, 64);

        let leaf2 = mgr.find_leaf(2).expect("Leaf 2 must be found");
        let l2_size = leaf2.size;
        let l2_flash = leaf2.flash_offset;
        assert_eq!(l2_size, 1024);
        assert_eq!(leaf2.flags, FLAG_LEAF_RT);
        assert_eq!(l2_flash, 0x2400);

        assert!(mgr.find_leaf(3).is_none());
    }
}
