//! Microkernel Executable Application Runtimes & Execution Adapters.
//!
//! Provides decoupled execution adapters for the four KRX execution classes:
//! - Class 0 (`Native`): Zero-overhead single-base PIC ARM Thumb-1 machine code.
//! - Class 1 (`Paged`): Demand-paged fixed-window overlay cache with LR-thunking.
//! - Class 2 (`VMachine`): Sandboxed, verified, fuel-metered KVM-16 bytecode interpreter.
//! - Class 3 (`Declare`): Declarative 3.0 state & UI projection walker engine.
//! - `syscall`: Modular per-capability VTables, `ServiceRegistry`, and GOT binder.

#[cfg(feature = "krx_declare")]
pub mod declare;
#[cfg(feature = "krx_runtime")]
pub mod loader;
#[cfg(feature = "krx_native")]
pub mod native_runner;
#[cfg(feature = "app_hibernation")]
pub mod swap;
#[cfg(feature = "krx_runtime")]
pub mod syscall;
#[cfg(feature = "krx_vm")]
pub mod vm_runner;

pub use crate::data::arena::{
    TRANSIENT_ARENA_BYTES, TRANSIENT_ARENA_WORDS, arena_base_ptr, transient_arena_bytes_mut,
};

#[cfg(feature = "krx_declare")]
pub use declare::*;
#[cfg(feature = "krx_runtime")]
pub use loader::*;
#[cfg(feature = "krx_native")]
pub use native_runner::*;
#[cfg(feature = "app_hibernation")]
pub use swap::*;
#[cfg(feature = "krx_runtime")]
pub use syscall::*;
#[cfg(feature = "krx_vm")]
pub use vm_runner::*;

#[cfg(not(feature = "krx_runtime"))]
pub fn launch_krx_package(
    _ctx: &mut crate::services::SysContext<'_, '_>,
    _path: &str,
) -> Result<usize, kernel::proc::krx::KrxLaunchError> {
    Err(kernel::proc::krx::KrxLaunchError::VmDisabled)
}
