//! Shared Transient Scratch & Application Execution Arena.
//!
//! Provides a zero-alloc 6.0 KiB (`6,144 B`) aligned arena in `.uninit` RAM
//! supporting full 6 KiB N1 native images (or 5 KiB P0 root + 1 KiB leaf
//! overlay window at `[5120..6144)`), with transient OS scratch/staging buffers
//! placed in `[5120..6144)` and guarded against overwriting active P0 leaf code.
//! Right-sizing from 8 KiB to 6 KiB returns 2.0 KiB of contiguous hardware call
//! stack headroom across the microkernel, UI compositor, KRX apps, and ISRs.

pub const TRANSIENT_ARENA_WORDS: usize = 1536; // 6.0 KiB aligned arena
pub const TRANSIENT_ARENA_BYTES: usize = TRANSIENT_ARENA_WORDS * 4;
pub const TRANSIENT_SCRATCH_OFFSET: usize = 5120;
pub const ZONE_A_MAX_BYTES: usize = 1024; // Zone A ceiling for concurrent resident background tasks

#[repr(align(8))]
pub struct AlignedArena(pub [u32; TRANSIENT_ARENA_WORDS]);

#[unsafe(link_section = ".uninit")]
pub static mut TRANSIENT_ARENA: AlignedArena = AlignedArena([0u32; TRANSIENT_ARENA_WORDS]);

#[inline(never)]
pub fn arena_base_ptr() -> *mut u8 {
    core::ptr::addr_of_mut!(TRANSIENT_ARENA) as *mut u8
}

/// Active Zone A byte reservation (`0` when no resident background daemon/service/module is loaded).
#[inline(always)]
pub fn zone_a_used_bytes() -> usize {
    #[cfg(feature = "krx_native")]
    {
        crate::runtime::background::BgManager::zone_a_used_bytes()
    }
    #[cfg(not(feature = "krx_native"))]
    {
        0
    }
}

/// Base pointer of Zone B (foreground app / P1 demand-paged window), strictly disjoint from Zone A.
#[inline(always)]
pub fn zone_b_base_ptr() -> *mut u8 {
    unsafe { arena_base_ptr().add(zone_a_used_bytes()) }
}

/// Maximum byte span available to Zone B's P1 demand-paged cache without overlapping Zone A or Zone S.
#[inline(always)]
pub fn zone_b_cache_bytes() -> usize {
    TRANSIENT_SCRATCH_OFFSET
        .saturating_sub(zone_a_used_bytes())
        .min(4096)
}

/// Returns `true` while a P0 structured leaf overlay is actively executing from `[7168..8192)`.
/// Any scratch/staging access overlapping the overlay window must be refused while this is `true`.
#[inline(always)]
pub fn is_p0_leaf_dispatching() -> bool {
    #[cfg(feature = "krx_native")]
    {
        crate::runtime::native_runner::is_leaf_dispatching()
    }
    #[cfg(not(feature = "krx_native"))]
    {
        false
    }
}

#[inline(always)]
pub fn transient_arena_bytes_mut() -> &'static mut [u8; TRANSIENT_ARENA_BYTES] {
    unsafe { &mut *(arena_base_ptr() as *mut [u8; TRANSIENT_ARENA_BYTES]) }
}

#[inline(always)]
pub fn scratch_tail_bytes_mut()
-> &'static mut [u8; TRANSIENT_ARENA_BYTES - TRANSIENT_SCRATCH_OFFSET] {
    #[cfg(feature = "krx_native")]
    crate::runtime::native_runner::invalidate_leaf_cache();
    unsafe {
        &mut *(arena_base_ptr().add(TRANSIENT_SCRATCH_OFFSET)
            as *mut [u8; TRANSIENT_ARENA_BYTES - TRANSIENT_SCRATCH_OFFSET])
    }
}

/// Safe fallible staging buffer accessor: returns `None` if a P0 leaf overlay is actively
/// executing from the same tail window, preventing live code corruption.
#[inline(never)]
pub fn try_staging_buf() -> Option<&'static mut [u8; 512]> {
    if is_p0_leaf_dispatching() {
        return None;
    }
    #[cfg(feature = "krx_native")]
    crate::runtime::native_runner::invalidate_leaf_cache();
    Some(unsafe { &mut *(arena_base_ptr().add(TRANSIENT_SCRATCH_OFFSET) as *mut [u8; 512]) })
}

#[inline(never)]
pub fn staging_buf() -> &'static mut [u8; 512] {
    debug_assert!(
        !is_p0_leaf_dispatching(),
        "staging_buf() aliases live P0 leaf overlay while dispatching"
    );
    #[cfg(feature = "krx_native")]
    crate::runtime::native_runner::invalidate_leaf_cache();
    unsafe { &mut *(arena_base_ptr().add(TRANSIENT_SCRATCH_OFFSET) as *mut [u8; 512]) }
}
