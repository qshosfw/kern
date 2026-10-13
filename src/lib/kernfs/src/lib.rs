//! KernFS: Micro Embedded Filesystem Engine for KERN Microkernel.
//!
//! Provides ultra-compact (< 5 KB flash footprint), pure Rust `#![no_std]`, zero-alloc,
//! wear-leveled, crash-safe VFS storage.

#![no_std]

pub mod device;
pub mod fs;

pub use device::{BlockDevice, FsError};
pub use fs::{FsStats, KernFS, HEADER_SIZE, MAGIC_KERNFS};
