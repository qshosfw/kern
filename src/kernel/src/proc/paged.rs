//! KRX Demand-Paged Native Subsystem (KRX-ABI 2.5.0 Chapter 7).
//!
//! Implements the normative fixed-window overlay cache (`FixedWindowCache`),
//! `SectionBlockMap` (`0x0005`) parser, LRU victim selection with `.krx.pinned`
//! immunity, and the block-relative shadow return stack (`ShadowReturnStack`)
//! for LR-thunking across overlay evictions on Cortex-M0+ (no MMU).

use super::krx::BlockEntry;

/// Block flag: pinned in cache (never evicted once loaded; e.g., DSP inner loops)
pub const BLOCK_FLAG_PINNED: u8 = 1 << 0;
/// Block flag: real-time bounded latency block
pub const BLOCK_FLAG_RT: u8 = 1 << 1;
/// Block flag: sequential prefetch hint for next block
pub const BLOCK_FLAG_HINT_NEXT: u8 = 1 << 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PagedError {
    InvalidBlockId,
    NoUnpinnedSlotAvailable,
    ShadowStackOverflow,
    ShadowStackUnderflow,
    StorageReadFailed,
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
            let count = (body.len() / BlockEntry::SIZE).min(MAX_BLOCKS);
            for i in 0..count {
                if let Some(entry) = BlockEntry::parse(&body[i * BlockEntry::SIZE..]) {
                    self.blocks[i] = entry;
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

        // 2. Cache miss (Page Fault): find empty slot first
        for s in 0..slot_limit {
            if self.slots[s].resident_block.is_none() {
                self.slots[s] = CacheSlotMeta {
                    resident_block: Some(block_id),
                    pinned: entry.is_pinned(),
                    last_access_tick: self.access_clock,
                };
                self.fault_count = self.fault_count.saturating_add(1);
                return Ok((s, false, entry));
            }
        }

        // 3. Evict least-recently-used unpinned slot
        let mut victim: Option<usize> = None;
        let mut oldest_age: u16 = 0;
        for s in 0..slot_limit {
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

        let victim_idx = victim.ok_or(PagedError::NoUnpinnedSlotAvailable)?;
        self.slots[victim_idx] = CacheSlotMeta {
            resident_block: Some(block_id),
            pinned: entry.is_pinned(),
            last_access_tick: self.access_clock,
        };
        self.fault_count = self.fault_count.saturating_add(1);
        Ok((victim_idx, false, entry))
    }

    /// Byte offset of `slot_idx` relative to the start of the fixed-window cache VMA.
    #[inline(always)]
    pub const fn slot_byte_offset(&self, slot_idx: usize) -> usize {
        slot_idx * (self.page_size as usize)
    }
}

/// Shadow return frame for LR-thunking across paged block calls (KRX-ABI 2.5.0 Section 7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReturnFrame {
    pub caller_block_id: u8,
    pub caller_slot: u8,
    pub return_offset: u16,
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
