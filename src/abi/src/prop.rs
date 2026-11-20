//! Property registry: every scalar setting the system exposes, declared once.
//!
//! A property id is `(domain << 5) | offset`: the configuration domain and the
//! byte offset inside its CRC-sealed block. Bounds checking, the Settings UI,
//! `Op::PropGet` / `Op::PropSet` and RigWire all read this one table.

use crate::Cap;

/// One scalar property: identifier and inclusive bounds. 4 bytes in `.rodata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Prop {
    pub id: u8,
    pub min: u8,
    pub max: u8,
    pub step: u8,
}

impl Prop {
    /// Configuration domain owning this property.
    #[inline(always)]
    pub const fn domain(self) -> usize {
        (self.id >> 5) as usize
    }

    /// Byte offset inside the domain block.
    #[inline(always)]
    pub const fn offset(self) -> usize {
        (self.id & 0x1F) as usize
    }

    #[inline(always)]
    pub const fn admits(self, val: u8) -> bool {
        val >= self.min && val <= self.max
    }
}

/// Capability an image needs to write a property of each domain.
pub const DOMAIN_CAP: [Cap; 6] = [
    Cap::DisplayDirect,
    Cap::AudioOutput,
    Cap::Keypad,
    Cap::PowerManage,
    Cap::PowerManage,
    Cap::TaskControl,
];

macro_rules! props {
    ( $( $name:ident = ($dom:literal, $off:literal) $min:literal ..= $max:literal / $step:literal ),* $(,)? ) => {
        $( pub const $name: u8 = ($dom << 5) | $off; )*

        /// Every property, ordered by id.
        pub static TABLE: [Prop; [$($off),*].len()] = [
            $( Prop { id: ($dom << 5) | $off, min: $min, max: $max, step: $step }, )*
        ];

        /// `(name, id)` of every property, for host-side generators.
        #[cfg(feature = "describe")]
        pub const NAMES: &[(&str, u8)] = &[ $( (stringify!($name), ($dom << 5) | $off) ),* ];
    };
}

props! {
    // Display
    BACKLIGHT_TIME       = (0, 1)  0..=5   / 1,
    BACKLIGHT_MAX        = (0, 2)  1..=10  / 1,
    BACKLIGHT_MIN        = (0, 3)  0..=10  / 1,
    CONTRAST             = (0, 4)  0..=100 / 5,
    INVERT_PIXELS        = (0, 5)  0..=1   / 1,
    BOOT_SPLASH_ANIM     = (0, 6)  0..=3   / 1,
    BOOT_SPLASH_MODE     = (0, 7)  0..=1   / 1,
    BOOT_SPLASH_DURATION = (0, 8)  0..=4   / 1,
    SHADING_MODE         = (0, 9)  0..=2   / 1,
    UI_ANIMATIONS        = (0, 10) 0..=1   / 1,
    REFRESH_RATE         = (0, 11) 0..=3   / 1,
    // Audio
    VOLUME               = (1, 1)  0..=10  / 1,
    KEY_BEEP             = (1, 2)  0..=6   / 1,
    MIC_GAIN             = (1, 3)  0..=31  / 1,
    ROGER_BEEP           = (1, 4)  0..=12  / 1,
    BEEP_GAIN            = (1, 6)  0..=10  / 1,
    BOOT_JINGLE          = (1, 7)  0..=4   / 1,
    AUDIO_PROFILE        = (1, 8)  0..=4   / 1,
    // Keys
    NAV_LAYOUT           = (2, 1)  0..=1   / 1,
    AUTO_LOCK            = (2, 2)  0..=4   / 1,
    F_KEY_TIMEOUT        = (2, 3)  0..=4   / 1,
    FAST_SCROLL          = (2, 4)  0..=1   / 1,
    HOLD_DELAY           = (2, 5)  0..=3   / 1,
    REPEAT_RATE          = (2, 6)  0..=3   / 1,
    PTT_MODE             = (2, 7)  0..=2   / 1,
    F_DRAWER             = (2, 8)  0..=1   / 1,
    // System
    MODEL                = (3, 1)  0..=2   / 1,
    UART_BAUD            = (3, 2)  0..=1   / 1,
    CPU_CLOCK            = (3, 3)  0..=4   / 1,
    // Battery
    BATTERY_TYPE         = (4, 1)  0..=5   / 1,
    BATTERY_FORMAT       = (4, 2)  0..=8   / 1,
    POWER_PROFILE        = (4, 3)  0..=3   / 1,
    AUTO_POWER_OFF       = (4, 4)  0..=5   / 1,
    CHARGE_DETECT        = (4, 5)  0..=1   / 1,
    // Apps
    LAUNCHER_STYLE       = (5, 1)  0..=1   / 1,
    SHOW_NUMBERS         = (5, 2)  0..=1   / 1,
    SHOW_LABELS          = (5, 3)  0..=1   / 1,
    STARTUP_APP          = (5, 4)  0..=2   / 1,
    PAGER_POLICY         = (5, 6)  0..=2   / 1,
    SWAP_POLICY          = (5, 7)  0..=2   / 1,
    DAEMON_BUDGET_MS     = (5, 12) 2..=16  / 1,
    HOOK_BUDGET_MS       = (5, 13) 2..=32  / 1,
    MAX_STRIKES          = (5, 14) 0..=3   / 1,
    ZONE_B_CACHE_KB      = (5, 15) 1..=4   / 1,
}

const _: () = {
    let mut i = 1;
    while i < TABLE.len() {
        assert!(
            TABLE[i - 1].id < TABLE[i].id,
            "properties must be ordered by id"
        );
        assert!(TABLE[i].min <= TABLE[i].max && TABLE[i].step > 0);
        i += 1;
    }
};

/// Descriptor of property `id`, if one is declared.
#[inline(never)]
pub fn find(id: u8) -> Option<&'static Prop> {
    TABLE.iter().find(|p| p.id == id)
}
