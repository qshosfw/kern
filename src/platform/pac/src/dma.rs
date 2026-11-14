//! DMA controller registers for PY32F071.
//!
//! Provides access to the DMA channels and SYSCFG DMA remap.

pub const DMA_BASE: u32 = 0x4002_0000;

/// DMA interrupt status register
pub const DMA_ISR: *const u32 = DMA_BASE as *const u32;
/// DMA interrupt flag clear register
pub const DMA_IFCR: *mut u32 = (DMA_BASE + 0x04) as *mut u32;

// ── DMA Channel Offsets ──────────────────────────────────────────────────

#[inline(always)]
pub const fn ccr(ch: u8) -> *mut u32 {
    (DMA_BASE + 0x08 + ((ch as u32 - 1) * 0x14)) as *mut u32
}

#[inline(always)]
pub const fn cndtr(ch: u8) -> *mut u32 {
    (DMA_BASE + 0x0C + ((ch as u32 - 1) * 0x14)) as *mut u32
}

#[inline(always)]
pub const fn cpar(ch: u8) -> *mut u32 {
    (DMA_BASE + 0x10 + ((ch as u32 - 1) * 0x14)) as *mut u32
}

#[inline(always)]
pub const fn cmar(ch: u8) -> *mut u32 {
    (DMA_BASE + 0x14 + ((ch as u32 - 1) * 0x14)) as *mut u32
}

// ── CCR Bit Definitions ─────────────────────────────────────────────────

pub const CCR_EN: u32 = 1 << 0;
pub const CCR_TCIE: u32 = 1 << 1;
pub const CCR_HTIE: u32 = 1 << 2;
pub const CCR_TEIE: u32 = 1 << 3;
pub const CCR_DIR: u32 = 1 << 4;
pub const CCR_CIRC: u32 = 1 << 5;
pub const CCR_PINC: u32 = 1 << 6;
pub const CCR_MINC: u32 = 1 << 7;
pub const CCR_PSIZE_SHIFT: u32 = 8;
pub const CCR_MSIZE_SHIFT: u32 = 10;
pub const CCR_PL_SHIFT: u32 = 12;
pub const CCR_MEM2MEM: u32 = 1 << 14;

// ── ISR / IFCR Flags ────────────────────────────────────────────────────

#[inline(always)]
pub const fn gif(ch: u8) -> u32 {
    1 << ((ch - 1) * 4)
}
#[inline(always)]
pub const fn tcif(ch: u8) -> u32 {
    1 << (1 + (ch - 1) * 4)
}
#[inline(always)]
pub const fn htif(ch: u8) -> u32 {
    1 << (2 + (ch - 1) * 4)
}
#[inline(always)]
pub const fn teif(ch: u8) -> u32 {
    1 << (3 + (ch - 1) * 4)
}
