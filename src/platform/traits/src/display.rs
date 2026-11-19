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

/// 8-page × 128-column monochrome framebuffer contract for display drivers.
///
/// Decouples hardware display drivers (e.g. ST7565) from concrete 2D graphics libraries (`gfx`).
pub trait PagedFramebuffer {
    /// Bitmask of modified 8-pixel-tall pages (`bit 0` = page 0 .. `bit 7` = page 7).
    fn dirty_mask(&self) -> u8;
    /// Clear all pending dirty flags after flushing to the panel.
    fn clear_dirty(&mut self);
    /// Raw 128-byte page slice for `page` (`0..8`).
    fn page_bytes(&self, page: usize) -> &[u8; 128];
    /// Optional temporal FRC subframe byte (defaults to 1-bit monochrome page byte).
    #[inline(always)]
    fn subframe_byte(&self, page: usize, col: usize, _subframe: u8) -> u8 {
        self.page_bytes(page)[col]
    }
    /// Optional ordered-dither byte (defaults to 1-bit monochrome page byte).
    #[inline(always)]
    fn dither_byte(&self, page: usize, col: usize) -> u8 {
        self.page_bytes(page)[col]
    }
}
