//! Lightweight UI Status Icon & Glyph Descriptors.
//!
//! Pure POD metadata types shared by `kernel::app::StatusBarConfig` and
//! `kernel::proc::StatusTray` without coupling the microkernel to 2D rasterization.

/// Standardized monochromatic icon glyph identifier for UI and status bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Icon {
    // ── Primary status icons (ABI IDs 0..=21; have dedicated 5px and 8x8 art) ──
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
    // ── Extended 5px status icons (ABI IDs 0x82..=0x93) ────────────────────────
    Tx = 22,
    Rx = 23,
    DualWatch = 24,
    CrossBand = 25,
    Vox = 26,
    Bluetooth = 27,
    Gps = 28,
    Wifi = 29,
    Alarm = 30,
    Record = 31,
    Headphones = 32,
    Warning = 33,
    Mail = 34,
    Clock = 35,
    Tone = 36,
    Reverse = 37,
    Encrypt = 38,
    Packet = 39,
}

impl Icon {
    /// Number of primary status glyphs with dedicated 8x8 hold-sheet art (`Antenna..=Moon`).
    pub const PRIMARY_COUNT: usize = Icon::Moon as usize + 1;
    /// Total number of built-in status glyphs (`Antenna..=Packet`).
    pub const COUNT: usize = Icon::Packet as usize + 1;
    /// First ABI identifier for extended status glyphs (`0x82`, above `KRX_ICON_CUSTOM_1`).
    pub const ABI_EXT_BASE: u8 = 0x82;

    #[inline(always)]
    pub const fn width(self) -> u8 {
        match self {
            Icon::Plug
            | Icon::SerialActive
            | Icon::Storage
            | Icon::NoListen
            | Icon::DualWatch
            | Icon::CrossBand
            | Icon::Mail
            | Icon::Tone
            | Icon::Encrypt
            | Icon::Packet => 7,
            Icon::Beacon => 6,
            _ => 5,
        }
    }

    /// Decode a contiguous table index `0..Icon::COUNT`.
    #[inline(always)]
    pub const fn from_index(idx: u8) -> Option<Self> {
        if (idx as usize) < Icon::COUNT {
            Some(unsafe { core::mem::transmute::<u8, Self>(idx) })
        } else {
            None
        }
    }

    /// Decode an `abi::icon` identifier (`0..=21` or `0x82..=0x93`) into a status [`Icon`].
    #[inline(always)]
    pub const fn from_u8(abi_id: u8) -> Option<Self> {
        if (abi_id as usize) < Icon::PRIMARY_COUNT {
            Some(unsafe { core::mem::transmute::<u8, Self>(abi_id) })
        } else if abi_id >= Self::ABI_EXT_BASE {
            let idx = (Icon::PRIMARY_COUNT as u8).wrapping_add(abi_id - Self::ABI_EXT_BASE);
            Self::from_index(idx)
        } else {
            None
        }
    }

    /// Return the stable `abi::icon` identifier for this status icon.
    #[inline(always)]
    pub const fn abi_id(self) -> u8 {
        let idx = self as u8;
        if (idx as usize) < Icon::PRIMARY_COUNT {
            idx
        } else {
            Self::ABI_EXT_BASE + (idx - Icon::PRIMARY_COUNT as u8)
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
