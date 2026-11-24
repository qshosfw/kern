//! Shared Transient Scratch & Application Execution Arena.
//!
//! Provides a zero-alloc 8.0 KiB (`8,192 B`) aligned arena in `.uninit` RAM
//! supporting full 8 KiB KRX application images (or 7 KiB P0 root + 1 KiB leaf
//! overlay window at `[7168..8192)`), with transient OS scratch/staging buffers
//! placed in `[7168..8192)` and guarded against overwriting active P0 leaf code.

pub const TRANSIENT_ARENA_WORDS: usize = 2048; // 8.0 KiB aligned arena
pub const TRANSIENT_ARENA_BYTES: usize = TRANSIENT_ARENA_WORDS * 4;
pub const TRANSIENT_SCRATCH_OFFSET: usize = 7168;

#[repr(align(8))]
pub struct AlignedArena(pub [u32; TRANSIENT_ARENA_WORDS]);

#[unsafe(link_section = ".uninit")]
pub static mut TRANSIENT_ARENA: AlignedArena = AlignedArena([0u32; TRANSIENT_ARENA_WORDS]);

#[inline(always)]
pub fn arena_base_ptr() -> *mut u8 {
    core::ptr::addr_of_mut!(TRANSIENT_ARENA) as *mut u8
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
