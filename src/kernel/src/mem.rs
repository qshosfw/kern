//! Microkernel Memory Management & Zero-Alloc Allocators
//!
//! Enforces zero dynamic heap allocation across the microkernel while
//! providing bounded cell-slabs and transient scratch arenas.

pub mod arena;
pub mod guard;

pub use arena::ScratchArena;
pub use guard::{is_guard_zone_intact, stack_headroom_bytes, stack_paint, stack_watermark};
