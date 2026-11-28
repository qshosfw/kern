//! KRX Demand-Paged Native Subsystem (`P1 v2` Streamed Virtual Code Cache).
//!
//! Implements the unlimited-size `StreamedWindowCache` (`<= 384 KiB`), `SectionBlockMap`
//! (`0x0005`) parser, Clock/LRU victim selection with `.krx.pinned` immunity, and the
//! `PageReturnFrame` shadow return stack (`ShadowReturnStack`) on Cortex-M0+ (no MMU).

use super::image::BlockEntry;

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

        let shift: u8 = if page_size <= 512 {
            9
        } else if page_size <= 1024 {
            10
        } else {
            11
        };
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

#[cfg(test)]
mod tests {
    use super::*;

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
