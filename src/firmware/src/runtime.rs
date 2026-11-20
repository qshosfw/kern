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

/// Read and parse a KRX package header from storage into `out` (single shared read + summary path).
#[inline(never)]
pub fn read_krx_summary<'a, 'b>(
    storage: &mut crate::services::SystemStorage,
    path: &str,
    hdr_buf: &'a mut [u8],
    out: &'b mut core::mem::MaybeUninit<kernel::runtime::KrxSummary<'a>>,
) -> Result<&'b mut kernel::runtime::KrxSummary<'a>, kernel::runtime::KrxLaunchError> {
    let read_len = storage
        .read_file_at(path, 0, hdr_buf)
        .map_err(|e| match e {
            kernel::service::StorageError::NotFound => {
                kernel::runtime::KrxLaunchError::FileNotFound
            }
            _ => kernel::runtime::KrxLaunchError::ReadFailed,
        })?;
    kernel::runtime::KrxPreamble::summarize_into(&hdr_buf[..read_len], out)
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
