#![no_std]

pub mod bytes;
pub mod codec;
pub mod dsp;
pub mod format;
pub mod gsm7;
pub mod math;
pub mod mbuf;
pub mod path;
pub mod serde;
pub mod units;

pub use format::*;
pub use math::*;

/// Define a named `.rodata` byte array without emitting an 8-byte fat-pointer static.
#[macro_export]
macro_rules! static_bytes {
    ($vis:vis static $name:ident = $lit:literal;) => {
        $vis static $name: [u8; $lit.len()] = *$lit;
    };
}
