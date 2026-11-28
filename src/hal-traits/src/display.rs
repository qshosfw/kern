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
    /// Inclusive column span `(x0, x1)` (`0..=127`) modified in `page` (`0..8`).
    #[inline(always)]
    fn dirty_span(&self, _page: usize) -> (u8, u8) {
        (0, 127)
    }
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

/// Hardware status LED color state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum StatusLed {
    Off = 0,
    Red = 1,
    Green = 2,
    Orange = 3,
}

/// Time-varying indicator pattern evaluated from monotonic uptime milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedPattern {
    Off,
    Solid(StatusLed),
    BlinkFast(StatusLed),
    BlinkMedium(StatusLed),
    BlinkSlow(StatusLed),
    Pulse(StatusLed),
    Alternate(StatusLed, StatusLed),
}

impl LedPattern {
    #[inline(always)]
    pub fn evaluate(&self, now_ms: u32) -> StatusLed {
        match self {
            Self::Off => StatusLed::Off,
            Self::Solid(c) => *c,
            Self::BlinkFast(c) => {
                if (now_ms / 100) & 1 == 0 {
                    *c
                } else {
                    StatusLed::Off
                }
            }
            Self::BlinkMedium(c) => {
                if (now_ms / 250) & 1 == 0 {
                    *c
                } else {
                    StatusLed::Off
                }
            }
            Self::BlinkSlow(c) => {
                if (now_ms / 500) & 1 == 0 {
                    *c
                } else {
                    StatusLed::Off
                }
            }
            Self::Pulse(c) => {
                if (now_ms % 1000) < 50 {
                    *c
                } else {
                    StatusLed::Off
                }
            }
            Self::Alternate(c1, c2) => {
                if (now_ms / 250) & 1 == 0 {
                    *c1
                } else {
                    *c2
                }
            }
        }
    }
}

/// Board-level backlight LED output driver (monochrome or 3-bit RGB channel mask).
pub trait BacklightOutput {
    fn set_output(&mut self, on: bool, rgb_mask: u8);
}

/// 4-wire SPI + CS + DC/A0 + Delay bus for command/data LCD controllers (ST7565, ST7735, etc.).
pub trait CommandDisplayBus: crate::spi::SpiDevice + crate::time::Delay {
    fn cs_low(&mut self);
    fn cs_high(&mut self);
    fn dc_low(&mut self);
    fn dc_high(&mut self);
}
