//! Watchdog Safety Device Trait.

pub trait WatchdogDevice {
    fn feed(&mut self);
    fn is_watchdog_reset(&self) -> bool;
}
