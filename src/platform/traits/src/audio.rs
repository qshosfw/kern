//! Audio hardware abstraction traits.

/// Low-level Audio Power Amplifier (PA) Hardware Control.
pub trait AudioPa {
    fn enable(&mut self);
    fn disable(&mut self);
    fn is_enabled(&self) -> bool;
}
