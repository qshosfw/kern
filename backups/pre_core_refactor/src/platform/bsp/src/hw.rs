//! Hardware Abstraction Traits (Zero-cost, lightweight alternative to embedded-hal)

pub use kern_platform_traits::gpio::{InputPin, OutputPin};
pub use kern_platform_traits::radio::RadioBus as Bk4829Bus;
pub use kern_platform_traits::spi::SpiDevice;
