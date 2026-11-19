//! Time source and delay abstractions.

/// A monotonic time source providing microsecond and millisecond counters.
pub trait TimeSource {
    /// Current time in microseconds (wraps at u32::MAX).
    fn now_us(&self) -> u32;
    /// Current time in milliseconds (wraps at u32::MAX).
    fn now_ms(&self) -> u32;
}

/// Blocking microsecond / millisecond delay provider.
pub trait Delay {
    fn delay_us(&mut self, us: u32);
    fn delay_ms(&mut self, ms: u32);
}
