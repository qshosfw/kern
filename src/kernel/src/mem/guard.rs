//! Memory Integrity, Stack Painting & Runtime Watermarking (KRX-ABI 2.5.0 Section 13.1).
//!
//! Provides deterministic stack canary checking, high-water mark computation,
//! and 128-byte guard zone overflow detection to contain faults on Cortex-M0+.

pub const STACK_PAINT_SENTINEL: u32 = 0xDEAD_BEEF;
pub const STACK_GUARD_ZONE_BYTES: usize = 128;

/// Paint the unused stack region between `stack_base` and current stack pointer.
///
/// # Safety
/// Caller must ensure `stack_base` and `paint_top` are valid 4-byte aligned pointers
/// within the configured SRAM stack region, and that active stack frames are above `paint_top`.
#[inline(never)]
pub unsafe fn stack_paint(stack_base: *mut u32, paint_top: *mut u32) {
    let mut curr = stack_base;
    while curr < paint_top {
        unsafe {
            curr.write_volatile(STACK_PAINT_SENTINEL);
            curr = curr.add(1);
        }
    }
}

/// Compute the peak stack usage in bytes by scanning from base upwards (KRX Listing 13.1).
///
/// Returns the number of bytes that have been overwritten from the stack top down.
///
/// # Safety
/// Caller must ensure `stack_base` and `stack_top` bound a valid memory region.
pub unsafe fn stack_watermark(stack_base: *const u32, stack_top: *const u32) -> usize {
    let total_words = (stack_top as usize).saturating_sub(stack_base as usize) / 4;
    let mut unused_words = 0usize;
    unsafe {
        let mut curr = stack_base;
        while curr < stack_top {
            if curr.read_volatile() == STACK_PAINT_SENTINEL {
                unused_words += 1;
            } else {
                break;
            }
            curr = curr.add(1);
        }
    }
    (total_words.saturating_sub(unused_words)) * 4
}

/// Query remaining stack headroom in bytes before reaching `stack_base`.
///
/// # Safety
/// Caller must pass valid stack boundaries.
pub unsafe fn stack_headroom_bytes(stack_base: *const u32, stack_top: *const u32) -> usize {
    let total_bytes = (stack_top as usize).saturating_sub(stack_base as usize);
    let used_bytes = unsafe { stack_watermark(stack_base, stack_top) };
    total_bytes.saturating_sub(used_bytes)
}

/// Verify that stack headroom has not breached the 128-byte KRX Guard Zone.
///
/// # Safety
/// Caller must pass valid stack boundaries.
pub unsafe fn is_guard_zone_intact(stack_base: *const u32, stack_top: *const u32) -> bool {
    unsafe { stack_headroom_bytes(stack_base, stack_top) >= STACK_GUARD_ZONE_BYTES }
}
