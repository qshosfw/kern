//! Microkernel Executable Application Runtimes & Execution Adapters.
//!
//! Provides decoupled execution adapters for the four KRX execution classes:
//! - Class 0 (`Native`): Zero-overhead single-base PIC ARM Thumb-1 machine code.
//! - Class 1 (`Paged`): Demand-paged fixed-window overlay cache with LR-thunking.
//! - Class 2 (`VMachine`): Sandboxed, verified, fuel-metered KVM-16 bytecode interpreter.
//! - Class 3 (`Declare`): Declarative 3.0 state & UI projection walker engine.
//! - `abi_bridge`: Modular per-capability VTables, `ServiceRegistry`, and GOT binder.

pub mod abi_bridge;
pub mod declare;
pub mod loader;
pub mod native_runner;
#[cfg(feature = "app_hibernation")]
pub mod swap;
pub mod vm_runner;

pub use abi_bridge::*;
pub use declare::*;
pub use loader::*;
pub use native_runner::*;
#[cfg(feature = "app_hibernation")]
pub use swap::*;
pub use vm_runner::*;
