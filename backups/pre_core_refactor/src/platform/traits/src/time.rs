//! Time source abstraction.

/// A monotonic time source providing microsecond and millisecond counters.
pub trait TimeSource {
    /// Current time in microseconds (wraps at u32::MAX).
    fn now_us(&self) -> u32;
    /// Current time in milliseconds (wraps at u32::MAX).
    fn now_ms(&self) -> u32;
}
