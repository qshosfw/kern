//! Microkernel Executable Application Runtimes & Execution Adapters.
//!
//! Provides decoupled execution adapters for different executable classes:
//! - Class 0 (Native): Zero-overhead raw ARM Thumb-1 machine code.
//! - Class 1 (Paged): Demand-paged segmented memory overlay.
//! - Class 2 (VM): Sandboxed bytecode virtual machine interpreter.
//! - Class 3 (Declare): Declarative lightweight UI & rules walker engine.

pub mod declare;
pub mod loader;
pub mod native_runner;
#[cfg(feature = "app_hibernation")]
pub mod swap;
pub mod vm_runner;

pub use declare::*;
pub use loader::*;
pub use native_runner::*;
#[cfg(feature = "app_hibernation")]
pub use swap::*;
pub use vm_runner::*;
