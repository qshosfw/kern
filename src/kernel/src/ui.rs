//! Lightweight UI Status Icon & Glyph Descriptors.
//!
//! Pure POD metadata types shared by `kernel::app::StatusBarConfig` and
//! `kernel::proc::StatusTray` without coupling the microkernel to 2D rasterization.

/// Standardized monochromatic icon glyph identifier for UI and status bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Icon {
    Antenna = 0,
    Lock = 1,
    Unlock = 2,
    Speaker = 3,
    Shield = 4,
    Keypad = 5,
    Flashlight = 6,
    Mesh = 7,
    Function = 8,
    Plug = 9,
    SerialActive = 10,
    Bolt = 11,
    Broadcast = 12,
    Storage = 13,
    Scan = 14,
    Beacon = 15,
    Monitor = 16,
    Converter = 17,
    Roger = 18,
    Beep = 19,
    NoListen = 20,
    Moon = 21,
}

impl Icon {
    /// Number of status glyphs (`Antenna..=Moon`).
    pub const COUNT: usize = Icon::Moon as usize + 1;

    #[inline(always)]
    pub const fn width(self) -> u8 {
        match self {
            Icon::Plug | Icon::SerialActive | Icon::Storage | Icon::NoListen => 7,
            Icon::Beacon => 6,
            _ => 5,
        }
    }

    #[inline(always)]
    pub const fn from_u8(val: u8) -> Option<Self> {
        if (val as usize) < Icon::COUNT {
            Some(unsafe { core::mem::transmute::<u8, Self>(val) })
        } else {
            None
        }
    }
}

/// A status bar glyph: either a standardized built-in icon or a custom 5x5 bitmap.
#[repr(C, align(4))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusGlyph {
    Builtin(Icon),
    Custom([u8; 5]),
}

impl StatusGlyph {
    #[inline(always)]
    pub const fn width(&self) -> u8 {
        match self {
            StatusGlyph::Builtin(ic) => ic.width(),
            StatusGlyph::Custom(_) => 5,
        }
    }

    #[inline(always)]
    pub const fn builtin(icon: Icon) -> Self {
        StatusGlyph::Builtin(icon)
    }

    #[inline(always)]
    pub const fn custom(bmp: [u8; 5]) -> Self {
        StatusGlyph::Custom(bmp)
    }

    #[inline(always)]
    pub const fn default_builtin() -> Self {
        StatusGlyph::Builtin(Icon::Antenna)
    }
}

impl From<Icon> for StatusGlyph {
    #[inline(always)]
    fn from(icon: Icon) -> Self {
        StatusGlyph::Builtin(icon)
    }
}
