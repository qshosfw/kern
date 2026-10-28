//! GPIO pin abstraction traits.

/// A digital output pin.
pub trait OutputPin {
    fn set_high(&mut self);
    fn set_low(&mut self);
    fn toggle(&mut self);
}

/// A digital input pin.
pub trait InputPin {
    fn is_high(&self) -> bool;
    fn is_low(&self) -> bool;
}
