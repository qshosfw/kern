#![cfg_attr(test, allow(dead_code))]
//! ARM Cortex-M0+ Caller Stack Unwinder.
//!
//! Instruction decoding and fault-cause elaboration are offloaded to `/apps/panic.mod`.

use py32_hal as hal;

/// Scan active stack window (`[thread_sp .. 0x2000_4000]`) for valid Thumb return addresses,
/// writing up to 4 unique return addresses into `out` and returning the number written.
#[inline(never)]
pub fn unwind_caller_stack(
    ef_valid: bool,
    thread_sp: u32,
    pc: u32,
    lr: u32,
    out: &mut [u32],
) -> u8 {
    let mut unwind_count = 0u8;

    let (start_sp, max_words) = if ef_valid
        && (thread_sp & 3) == 0
        && thread_sp >= 0x2000_0000
        && thread_sp <= (0x2000_4000 - 32)
    {
        (
            thread_sp,
            (((0x2000_4000 - thread_sp) >> 2) as usize).min(128),
        )
    } else {
        (0x2000_0000, 128)
    };

    let sp_slice = unsafe { core::slice::from_raw_parts(start_sp as *const u32, max_words) };
    for &w in sp_slice {
        let in_flash = w >= hal::flash::FIRMWARE_MIN_ADDR && w < hal::flash::FLASH_END_ADDR;
        let in_sram = w >= 0x2000_0000 && w < 0x2000_4000;
        if (in_flash || in_sram)
            && (w & 1) != 0
            && w != pc
            && w != lr
            && (unwind_count == 0 || out[unwind_count as usize - 1] != w)
            && (unwind_count as usize) < out.len()
        {
            out[unwind_count as usize] = w;
            unwind_count += 1;
        }
    }

    unwind_count
}
