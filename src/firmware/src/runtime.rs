//! Microkernel Executable Application Runtimes & Execution Adapters.
//!
//! Provides decoupled execution adapters for the active KRX execution classes:
//! - Class 0 (`Native`): Zero-overhead single-base PIC ARM Thumb-1 machine code.
//! - Class 1 (`Paged`): Demand-paged fixed-window overlay cache with LR-thunking.
//! - `syscall`: Modular per-capability VTables, `ServiceRegistry`, and GOT binder.

#[cfg(feature = "krx_native")]
pub mod fault;
#[cfg(feature = "krx_native")]
pub mod loader;
#[cfg(feature = "krx_native")]
pub mod native_runner;
#[cfg(feature = "app_hibernation")]
pub mod swap;
#[cfg(feature = "krx_native")]
pub mod syscall;

pub use crate::data::arena::{
    TRANSIENT_ARENA_BYTES, TRANSIENT_ARENA_WORDS, arena_base_ptr, transient_arena_bytes_mut,
};

#[cfg(feature = "krx_native")]
pub use loader::*;
#[cfg(feature = "krx_native")]
pub use native_runner::*;
#[cfg(feature = "app_hibernation")]
pub use swap::*;
#[cfg(feature = "krx_native")]
pub use syscall::*;

/// Read and parse a KRX package header from storage into the top of `TRANSIENT_ARENA`'s scratch
/// tail, returning `(hdr_buf, summary)` without touching P0 root code or allocating stack frames.
#[inline(never)]
pub fn read_krx_summary_in_scratch(
    storage: &mut crate::services::SystemStorage,
    path: &str,
) -> Result<(&'static [u8], &'static kernel::runtime::KrxSummary<'static>), kernel::runtime::KrxLaunchError>
{
    const HDR_RESERVE: usize = kernel::runtime::KrxPreamble::MAX_HEADER_SIZE;
    const SUMMARY_RESERVE: usize =
        (core::mem::size_of::<kernel::runtime::KrxSummary<'static>>() + 7) & !7;
    const _: () = assert!(
        HDR_RESERVE + SUMMARY_RESERVE
            <= crate::data::arena::TRANSIENT_ARENA_BYTES
                - crate::data::arena::TRANSIENT_SCRATCH_OFFSET
    );
    let tail = crate::data::arena::scratch_tail_bytes_mut();
    let tail_len = tail.len();
    let (pre_hdr, hdr_buf) = tail.split_at_mut(tail_len - HDR_RESERVE);
    let summary_slot = unsafe {
        &mut *(pre_hdr.as_mut_ptr().add(pre_hdr.len() - SUMMARY_RESERVE)
            as *mut core::mem::MaybeUninit<kernel::runtime::KrxSummary<'static>>)
    };
    let read_len = storage
        .read_file_at(path, 0, hdr_buf)
        .map_err(|e| match e {
            kernel::service::StorageError::NotFound => {
                kernel::runtime::KrxLaunchError::FileNotFound
            }
            _ => kernel::runtime::KrxLaunchError::ReadFailed,
        })?;
    let hdr: &'static [u8] = unsafe { core::slice::from_raw_parts(hdr_buf.as_ptr(), read_len) };
    let summary = kernel::runtime::KrxPreamble::summarize_into(hdr, summary_slot)?;
    Ok((hdr, summary))
}

#[cfg(not(feature = "krx_native"))]
pub fn launch_krx_package(
    _ctx: &mut crate::services::SysContext<'_, '_>,
    _path: &str,
    _start_arg: u32,
) -> Result<usize, kernel::runtime::KrxLaunchError> {
    Err(kernel::runtime::KrxLaunchError::UnsupportedProfile)
}

#[cfg(not(feature = "krx_native"))]
pub fn show_launch_error_dialog(
    _ctx: &mut crate::services::SysContext<'_, '_>,
    _path: &str,
    _err: kernel::runtime::KrxLaunchError,
) {
}
