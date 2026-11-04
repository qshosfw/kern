//! Shared Transient Scratch Memory Arena.
//!
//! Provides a zero-alloc 4 KiB aligned scratch buffer in `.uninit` RAM
//! for transient operations: file streaming, boot splash staging, calibration,
//! and native package execution.

pub const TRANSIENT_ARENA_WORDS: usize = 2048;
pub const TRANSIENT_ARENA_BYTES: usize = TRANSIENT_ARENA_WORDS * 4;
pub const TRANSIENT_SCRATCH_OFFSET: usize = 2048;

#[repr(align(4))]
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
pub fn staging_buf() -> &'static mut [u8; 512] {
    unsafe { &mut *(arena_base_ptr() as *mut [u8; 512]) }
}
