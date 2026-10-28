//! Display backend abstraction.

/// A monochrome display backend.
pub trait DisplayBackend {
    /// Display width in pixels.
    fn width(&self) -> u8;
    /// Display height in pixels.
    fn height(&self) -> u8;
    /// Set a single pixel. `on` = true for black/filled.
    fn set_pixel(&mut self, x: u8, y: u8, on: bool);
    /// Flush dirty regions to the physical display.
    fn flush(&mut self);
    /// Set display contrast (0-255, hardware-dependent mapping).
    fn set_contrast(&mut self, level: u8);
    /// Set display inversion.
    fn set_inverted(&mut self, inverted: bool);
}
