//! Shared Transient App Memory Arena
//!
//! Provides a bounded scratch arena reused across active foreground apps.
//! Since only one application is active at a time, large transient buffers
//! (e.g. scanner hit lists, packet frames, spectrum graphs) share this space.

/// Transient memory arena with bounded capacity.
#[repr(C, align(4))]
pub struct ScratchArena<const BYTES: usize = 512> {
    buffer: [u8; BYTES],
    watermark: usize,
}

impl<const BYTES: usize> Default for ScratchArena<BYTES> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const BYTES: usize> ScratchArena<BYTES> {
    pub const fn new() -> Self {
        Self {
            buffer: [0; BYTES],
            watermark: 0,
        }
    }

    /// Reset the scratch arena watermark back to 0.
    #[inline]
    pub fn reset(&mut self) {
        self.watermark = 0;
    }

    /// Capacity in bytes.
    #[inline]
    pub const fn capacity(&self) -> usize {
        BYTES
    }

    /// Current watermark in bytes.
    #[inline]
    pub const fn used(&self) -> usize {
        self.watermark
    }

    /// Remaining bytes available.
    #[inline]
    pub const fn available(&self) -> usize {
        BYTES.saturating_sub(self.watermark)
    }

    /// Acquire a slice of size `len` aligned to 4 bytes.
    pub fn alloc_slice(&mut self, len: usize) -> Option<&mut [u8]> {
        let aligned_watermark = (self.watermark + 3) & !3;
        if aligned_watermark + len <= BYTES {
            let start = aligned_watermark;
            let end = start + len;
            self.watermark = end;
            Some(&mut self.buffer[start..end])
        } else {
            None
        }
    }
}
