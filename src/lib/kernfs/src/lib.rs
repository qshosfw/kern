//! KernFS: Micro Embedded Filesystem Engine for KERN Microkernel.
//!
//! Provides `#![no_std]`, zero-alloc, 256-byte page-granular CoW flash storage (`KernFS v2`).

#![no_std]

pub mod device;
pub mod format;
pub mod fs;
pub mod gc;

#[cfg(test)]
mod tests;

pub use device::{BlockDevice, FsError};
pub use fs::{FsStats, KernFS};
