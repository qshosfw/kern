//! Package manifest identifiers: built-in icon glyphs and declared-action scopes.
//!
//! A package names these in its `krx.toml`; `./kern abi` mirrors them into
//! `sdk/c/krx_ops.h` as `KRX_ICON_*` and `KRX_SCOPE_*`.

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
        SLEEP = 51,
        BEACON_TOWER = 52,
        FILE_BIN = 53,
        FILE_CONTACTS = 54,
        FILE_BANDPLAN = 55,
        FILE_FM = 56,
        FILE_IMAGE = 57,
        FILE_ZONES = 58,
        FILE_HITS = 59,
        FILE_TONES = 60,
        FILE_CALIB = 61,
        VOLUME_MUTE = 62,
        VOLUME_LOW = 63,
        VOLUME_HIGH = 64,
        LEVELS = 65,
        SIGNAL = 66,
        MIC = 67,
        HEADPHONES = 68,
        MESSAGE = 69,
        CHAT = 70,
        BULLETIN = 71,
        CELL_BROADCAST = 72,
        TOWER = 73,
        GPS = 74,
        SATELLITE = 75,
        BLUETOOTH = 76,
        CLOCK = 77,
        BELL = 78,
        STAR = 79,
        KEY = 80,
        TRASH = 81,
        SEARCH = 82,
        EDIT = 83,
        DOWNLOAD = 84,
        SYNC = 85,
        POWER = 86,
        CHECK = 87,
        CROSS = 88,
        WARNING = 89,
        PLAY = 90,
        PAUSE = 91,
        STOP = 92,
        RECORD = 93,
        BRIGHTNESS = 94,
        CONTRAST = 95,
        FONT = 96,
        FILE_LOG = 97,
        FILE_KEY = 98,
        FILE_FONT = 99,
        FILE_SCRIPT = 100,
        FILE_UNKNOWN = 101,
        FILE_MSG = 102,
        FILE_CFG = 103,
        FILE_FW = 104,
        FILE_AUDIO = 105,
        FILE_TABLE = 106,
        FILE_CERT = 107,
        FILE_ARCHIVE = 108,
        FILE_MAP = 109,
        FILE_TEXT = 110,
        USB = 111,
        SERIAL = 112,
        SPECTRUM = 113,
        MORSE = 114,
        REPEATER = 115,
        THERMOMETER = 116,
        COMPASS = 117,
        MEMORY = 118,
        WIFI = 119,
        BATTERY_LOW = 120,
        BATTERY_CHG = 121,
        LOCK_OPEN = 122,
        EYE = 123,
        TERMINAL = 124,
        BOOKMARK = 125,
        FOLDER_UP = 126,
        MISSING = 127,
        // 0x80 (128) = KRX_ICON_CUSTOM_0, 0x81 (129) = KRX_ICON_CUSTOM_1
        STATUS_TX = 130,
        STATUS_RX = 131,
        STATUS_DUAL_WATCH = 132,
        STATUS_CROSS_BAND = 133,
        STATUS_VOX = 134,
        STATUS_BLUETOOTH = 135,
        STATUS_GPS = 136,
        STATUS_WIFI = 137,
        STATUS_ALARM = 138,
        STATUS_RECORD = 139,
        STATUS_HEADPHONES = 140,
        STATUS_WARNING = 141,
        STATUS_MAIL = 142,
        STATUS_CLOCK = 143,
        STATUS_TONE = 144,
        STATUS_REVERSE = 145,
        STATUS_ENCRYPT = 146,
        STATUS_PACKET = 147,
    }

    /// First 8x8 system glyph; identifiers below it are primary 5px status glyphs.
    pub const SYSTEM_BASE: u8 = FOLDER;
    /// Number of 8x8 system glyphs (`SYSTEM_BASE..=MISSING`).
    pub const SYSTEM_COUNT: usize = (MISSING - SYSTEM_BASE) as usize + 1;
    /// First extended 5px status glyph (placed above `KRX_ICON_CUSTOM_1 = 0x81`).
    pub const STATUS_EXT_BASE: u8 = STATUS_TX;
    /// Number of extended 5px status glyphs (`STATUS_TX..=STATUS_PACKET`).
    pub const STATUS_EXT_COUNT: usize = (STATUS_PACKET - STATUS_TX) as usize + 1;

    const _: () = assert!(
        (SYSTEM_BASE as usize + SYSTEM_COUNT) <= 0x80,
        "built-in system icon identifiers must stay below KRX_ICON_CUSTOM_0 (0x80)"
    );
    const _: () = assert!(
        STATUS_EXT_BASE == 0x82 && (STATUS_EXT_BASE as usize + STATUS_EXT_COUNT) < 0xFF,
        "extended status icon identifiers must sit in 0x82..0xFF"
    );
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
