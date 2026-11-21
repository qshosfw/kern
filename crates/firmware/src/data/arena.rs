//! Shared Transient Scratch & Application Execution Arena.
//!
//! Provides a zero-alloc 8.0 KiB (`8,192 B`) aligned arena in `.uninit` RAM
//! supporting full 8 KiB KRX application images (or 7 KiB P0 root + 1 KiB leaf
//! overlay window at `[7168..8192)`), with transient OS scratch/staging buffers
//! placed in `[6144..8192)`.

pub const TRANSIENT_ARENA_WORDS: usize = 2048; // 8.0 KiB aligned arena
pub const TRANSIENT_ARENA_BYTES: usize = TRANSIENT_ARENA_WORDS * 4;
pub const TRANSIENT_SCRATCH_OFFSET: usize = 6144;

#[repr(align(8))]
pub struct AlignedArena(pub [u32; TRANSIENT_ARENA_WORDS]);

#[unsafe(link_section = ".uninit")]
pub static mut TRANSIENT_ARENA: AlignedArena = AlignedArena([0u32; TRANSIENT_ARENA_WORDS]);

#[inline(always)]
pub fn arena_base_ptr() -> *mut u8 {
    core::ptr::addr_of_mut!(TRANSIENT_ARENA) as *mut u8
}

#[inline(always)]
pub fn transient_arena_bytes_mut() -> &'static mut [u8; TRANSIENT_ARENA_BYTES] {
    unsafe { &mut *(arena_base_ptr() as *mut [u8; TRANSIENT_ARENA_BYTES]) }
}

#[inline(always)]
pub fn scratch_tail_bytes_mut()
-> &'static mut [u8; TRANSIENT_ARENA_BYTES - TRANSIENT_SCRATCH_OFFSET] {
    unsafe {
        &mut *(arena_base_ptr().add(TRANSIENT_SCRATCH_OFFSET)
            as *mut [u8; TRANSIENT_ARENA_BYTES - TRANSIENT_SCRATCH_OFFSET])
    }
}

#[inline(always)]
pub fn staging_buf() -> &'static mut [u8; 512] {
    unsafe { &mut *(arena_base_ptr().add(TRANSIENT_SCRATCH_OFFSET) as *mut [u8; 512]) }
}
