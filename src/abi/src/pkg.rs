//! Package manifest identifiers: built-in icon glyphs and declared-action scopes.
//!
//! A package names these in its `krx.toml`; `./kern abi` mirrors them into
//! `packages/sdk/krx_ops.h` as `KRX_ICON_*` and `KRX_SCOPE_*`.

/// Declares `u8` identifiers plus their names for host-side generators.
macro_rules! ids {
    ( $( $(#[$doc:meta])* $name:ident = $val:literal ),* $(,)? ) => {
        $( $(#[$doc])* pub const $name: u8 = $val; )*

        /// `(name, value)` of every identifier in this namespace.
        #[cfg(feature = "describe")]
        pub const NAMES: &[(&str, u8)] = &[ $( (stringify!($name), $val) ),* ];
    };
}

/// Built-in monochrome glyphs addressable by `icon_id`.
pub mod icon {
    ids! {
        ANTENNA = 0,
        LOCK = 1,
        UNLOCK = 2,
        SPEAKER = 3,
        SHIELD = 4,
        KEYPAD = 5,
        FLASHLIGHT = 6,
        MESH = 7,
        FUNCTION = 8,
        PLUG = 9,
        SERIAL_ACTIVE = 10,
        BOLT = 11,
        BROADCAST = 12,
        STORAGE = 13,
        SCAN = 14,
        BEACON = 15,
        MONITOR = 16,
        CONVERTER = 17,
        ROGER = 18,
        BEEP = 19,
        NO_LISTEN = 20,
        MOON = 21,
        FOLDER = 22,
        CH = 23,
        VFO = 24,
        APP_RADIO = 25,
        APP_SETTINGS = 26,
        APP_EXPLORER = 27,
        APP_TORCH = 28,
        APP_DIAG = 29,
        APP_ABOUT = 30,
        CHECKBOX_OFF = 31,
        CHECKBOX_ON = 32,
        RADIO_OFF = 33,
        RADIO_ON = 34,
        ARROW_RIGHT = 35,
        ARROW_LEFT = 36,
        SELECT_ARROWS = 37,
        GEAR = 38,
        FILE_DOC = 39,
        FILE_FOLDER = 40,
        DISPLAY = 41,
        BUTTONS = 42,
        AUDIO = 43,
        BATTERY = 44,
        DEVICE = 45,
        APPS = 46,
        SECURITY = 47,
        SYSTEM = 48,
        UPLOAD = 49,
        HOME = 50,
    }

    /// First 8x8 system glyph; identifiers below it are 5x5 status glyphs.
    pub const SYSTEM_BASE: u8 = FOLDER;
    /// Number of 8x8 system glyphs (`SYSTEM_BASE..=HOME`).
    pub const SYSTEM_COUNT: usize = (HOME - SYSTEM_BASE) as usize + 1;
}

/// Where a declared action (`SECTION_ACTIONS`) is surfaced by the OS.
pub mod scope {
    ids! {
        /// Bindable to a programmable key.
        SHORTCUT = 1,
        /// Listed in the `[F]` drawer.
        DRAWER = 2,
        /// Opens as a page from Settings -> System.
        SYSTEM_PAGE = 4,
        /// Runs in place as a Settings -> System maintenance button.
        SYSTEM_ACTION = 8,
    }
}
