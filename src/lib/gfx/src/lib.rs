#![no_std]

pub mod bmp;
pub mod canvas;
pub mod draw;
pub mod fonts;
pub mod gui;
pub mod icons;
pub mod text;
pub mod widgets;

pub use canvas::{Canvas, Color, LCD_HEIGHT, LCD_WIDTH};
pub use kernel::ui::{Icon, StatusGlyph};

#[derive(Debug, Clone, Copy)]
pub struct GfxGlyph {
    pub bitmap_offset: u16,
    pub dimensions: u8,
    pub metrics: u8,
}

impl GfxGlyph {
    pub const fn new(
        bitmap_offset: u16,
        width: u8,
        height: u8,
        x_advance: u8,
        x_offset: i8,
        y_offset: i8,
    ) -> Self {
        let dimensions = (width << 4) | (height & 0x0F);
        let xa_bits = x_advance.saturating_sub(1) & 0x07;
        let xo_bits = (((x_offset + 1) as u8) & 0x03) << 3;
        let yo_bits = (((-y_offset) as u8) & 0x07) << 5;
        Self {
            bitmap_offset,
            dimensions,
            metrics: xa_bits | xo_bits | yo_bits,
        }
    }

    #[inline(always)]
    pub const fn width(&self) -> u8 {
        self.dimensions >> 4
    }

    #[inline(always)]
    pub const fn height(&self) -> u8 {
        self.dimensions & 0x0F
    }

    #[inline(always)]
    pub const fn x_advance(&self) -> u8 {
        (self.metrics & 0x07) + 1
    }

    #[inline(always)]
    pub const fn x_offset(&self) -> i8 {
        (((self.metrics >> 3) & 0x03) as i8) - 1
    }

    #[inline(always)]
    pub const fn y_offset(&self) -> i8 {
        -(((self.metrics >> 5) & 0x07) as i8)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GfxFont {
    pub bitmap: &'static [u8],
    pub glyphs: &'static [GfxGlyph],
    pub first: u8,
    pub last: u8,
    pub y_advance: u8,
}
