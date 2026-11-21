//! Cell-Slab Bitmap Allocator (KRX Map A Specification)
//!
//! Provides an exact $O(1)$ power-of-two geometry cell allocator (16B cells)
//! backed by a multi-word occupancy bitmap. Zero heap, strictly `#![no_std]`.
//! Standard Map A geometry: 400 cells = 6.4 KB with 13 u32 bitmap words.

pub const SLAB_IDENT: [u8; 4] = [0xC0, 0xDE, 0x04, 0xFB];

/// 16-byte fixed-size slab cell
#[repr(C, align(4))]
#[derive(Debug, Clone, Copy)]
pub struct SlabCell {
    pub data: [u8; 16],
}

impl SlabCell {
    pub const fn empty() -> Self {
        Self { data: [0; 16] }
    }
}

/// Static slab allocator backed by a multi-word bitmap (defaulting to Map A: 400 cells, 13 words).
pub struct CellSlab<const N: usize = 400, const WORDS: usize = 13> {
    pub ident: [u8; 4],
    pub bitmap: [u32; WORDS],
    pub cells: [SlabCell; N],
}

impl<const N: usize, const WORDS: usize> Default for CellSlab<N, WORDS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize, const WORDS: usize> CellSlab<N, WORDS> {
    pub const fn new() -> Self {
        const {
            assert!(
                N <= WORDS * 32,
                "CellSlab capacity N exceeds bitmap word capacity"
            )
        };
        Self {
            ident: SLAB_IDENT,
            bitmap: [0; WORDS],
            cells: [const { SlabCell::empty() }; N],
        }
    }

    /// Verifies that the slab header format identifier matches KRX SLAB_IDENT.
    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        self.ident == SLAB_IDENT
    }

    /// Checks if a cell at `idx` is currently allocated.
    #[inline]
    pub fn is_allocated(&self, idx: usize) -> bool {
        if idx >= N {
            return false;
        }
        let word_idx = idx >> 5;
        let bit_idx = idx & 31;
        if word_idx < WORDS {
            (self.bitmap[word_idx] & (1 << bit_idx)) != 0
        } else {
            false
        }
    }

    /// Allocate a single cell, returning its index.
    pub fn alloc(&mut self) -> Option<usize> {
        self.alloc_cell()
    }

    /// Allocate a single cell in O(1) time.
    pub fn alloc_cell(&mut self) -> Option<usize> {
        for (w_idx, word) in self.bitmap.iter_mut().enumerate() {
            if *word != 0xFFFF_FFFF {
                let free_bit = (!*word).trailing_zeros() as usize;
                let cell_idx = (w_idx << 5) | free_bit;
                if cell_idx < N {
                    *word |= 1 << free_bit;
                    return Some(cell_idx);
                }
            }
        }
        None
    }

    /// Allocate `count` contiguous cells.
    pub fn alloc_contiguous(&mut self, count: usize) -> Option<usize> {
        if count == 0 || count > N {
            return None;
        }
        if count == 1 {
            return self.alloc_cell();
        }

        let mut start_idx = 0;
        let mut run_len = 0;

        for idx in 0..N {
            if !self.is_allocated(idx) {
                if run_len == 0 {
                    start_idx = idx;
                }
                run_len += 1;
                if run_len == count {
                    // Mark all cells in this range as allocated
                    for i in start_idx..start_idx + count {
                        let word_idx = i >> 5;
                        let bit_idx = i & 31;
                        self.bitmap[word_idx] |= 1 << bit_idx;
                    }
                    return Some(start_idx);
                }
            } else {
                run_len = 0;
            }
        }

        None
    }

    /// Free a previously allocated cell by index.
    pub fn free(&mut self, idx: usize) -> bool {
        self.free_cell(idx)
    }

    /// Free a single cell by index.
    pub fn free_cell(&mut self, idx: usize) -> bool {
        if idx < N && self.is_allocated(idx) {
            let word_idx = idx >> 5;
            let bit_idx = idx & 31;
            self.bitmap[word_idx] &= !(1 << bit_idx);
            self.cells[idx] = SlabCell::empty();
            true
        } else {
            false
        }
    }

    /// Free `count` contiguous cells starting at `start_idx`.
    pub fn free_contiguous(&mut self, start_idx: usize, count: usize) -> bool {
        let Some(end) = start_idx.checked_add(count) else {
            return false;
        };
        if end > N {
            return false;
        }
        let mut all_freed = true;
        for i in start_idx..end {
            if !self.free_cell(i) {
                all_freed = false;
            }
        }
        all_freed
    }

    /// Get mutable reference to cell.
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut [u8; 16]> {
        if self.is_allocated(idx) {
            Some(&mut self.cells[idx].data)
        } else {
            None
        }
    }

    /// Get reference to cell.
    pub fn get(&self, idx: usize) -> Option<&[u8; 16]> {
        if self.is_allocated(idx) {
            Some(&self.cells[idx].data)
        } else {
            None
        }
    }

    /// Total count of allocated cells.
    pub fn allocated_count(&self) -> usize {
        let mut count = 0;
        for &w in self.bitmap.iter() {
            count += w.count_ones() as usize;
        }
        count
    }

    /// Total count of free cells available.
    pub fn free_count(&self) -> usize {
        N.saturating_sub(self.allocated_count())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_contiguous_rejects_overflow_and_oob() {
        let mut slab: CellSlab<8, 1> = CellSlab::new();
        let start = slab.alloc_contiguous(4).unwrap();
        assert_eq!(start, 0);
        assert!(!slab.free_contiguous(usize::MAX, 2));
        assert!(!slab.free_contiguous(6, 4));
        assert_eq!(slab.allocated_count(), 4);
        assert!(slab.free_contiguous(start, 4));
        assert_eq!(slab.allocated_count(), 0);
    }
}
