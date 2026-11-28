//! Unified Property & Live Telemetry Registry (`0x00..0xFE`).
//!
//! Every scalar setting, live telemetry reading, and hardware control in Kern is
//! declared once in a single `u8` ID space:
//! - `0x00..=0xBF` (`(domain << 5) | offset`): Durable `/pref/*.cfg` properties (`TABLE`)
//!   with inclusive bounds, CRC sealing, and caller-scoped live overrides (`SET` vs `PREF`).
//! - `0xC0..=0xDF` (`live::*`): Live system, power, time, keypad, audio, UI, and process scalars.
//! - `0xE0..=0xFE` (`live::*`): Live transceiver & broadcast FM scalars (`Cap::RadioRx`).

use crate::Cap;

/// One persistent scalar property: identifier and inclusive bounds. 4 bytes in `.rodata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Prop {
    pub id: u8,
    pub min: u8,
    pub max: u8,
    pub step: u8,
}

impl Prop {
    #[inline(always)]
    pub const fn domain(self) -> usize {
        (self.id >> 5) as usize
    }

    #[inline(always)]
    pub const fn offset(self) -> usize {
        (self.id & 0x1F) as usize
    }

    #[inline(always)]
    pub const fn admits(self, val: u8) -> bool {
        val >= self.min && val <= self.max
    }
}

/// Domains an application may control live, and the capability each needs.
pub const CONTROL_CAP: [Cap; 2] = [Cap::DisplayDirect, Cap::AudioOutput];

/// Live overrides one application may hold at a time.
pub const CONTROL_SLOTS: usize = 4;

macro_rules! props {
    (
        durable {
            $( $name:ident = ($dom:literal, $off:literal) $min:literal ..= $max:literal / $step:literal ),* $(,)?
        }
        live {
            $( $lname:ident = $lid:literal ),* $(,)?
        }
    ) => {
        $( pub const $name: u8 = ($dom << 5) | $off; )*

        pub mod live {
            $( pub const $lname: u8 = $lid; )*
        }
        pub use live::*;

        /// Every durable property, ordered by id (compile-time constant for PIC packages).
        pub const DEFS: [Prop; [$($off),*].len()] = [
            $( Prop { id: ($dom << 5) | $off, min: $min, max: $max, step: $step }, )*
        ];

        /// Every durable property, ordered by id.
        pub static TABLE: [Prop; [$($off),*].len()] = DEFS;

        /// `(name, id)` of every durable and live property, for `./kern abi` codegen.
        #[cfg(feature = "describe")]
        pub const NAMES: &[(&str, u8)] = &[
            $( (stringify!($name), ($dom << 5) | $off), )*
            $( (stringify!($lname), $lid), )*
        ];
    };
}

props! {
    durable {
        // Display (domain 0)
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
        // Audio (domain 1)
        VOLUME               = (1, 1)  0..=10  / 1,
        KEY_BEEP             = (1, 2)  0..=6   / 1,
        MIC_GAIN             = (1, 3)  0..=31  / 1,
        ROGER_BEEP           = (1, 4)  0..=12  / 1,
        BEEP_GAIN            = (1, 6)  0..=10  / 1,
        BOOT_JINGLE          = (1, 7)  0..=4   / 1,
        AUDIO_PROFILE        = (1, 8)  0..=4   / 1,
        // Keys (domain 2)
        NAV_LAYOUT           = (2, 1)  0..=1   / 1,
        AUTO_LOCK            = (2, 2)  0..=4   / 1,
        F_KEY_TIMEOUT        = (2, 3)  0..=4   / 1,
        FAST_SCROLL          = (2, 4)  0..=1   / 1,
        HOLD_DELAY           = (2, 5)  0..=3   / 1,
        REPEAT_RATE          = (2, 6)  0..=3   / 1,
        PTT_MODE             = (2, 7)  0..=2   / 1,
        F_DRAWER             = (2, 8)  0..=1   / 1,
        // System (domain 3)
        MODEL                = (3, 1)  0..=2   / 1,
        UART_BAUD            = (3, 2)  0..=1   / 1,
        CPU_CLOCK            = (3, 3)  0..=4   / 1,
        // Battery (domain 4)
        BATTERY_TYPE         = (4, 1)  0..=5   / 1,
        BATTERY_FORMAT       = (4, 2)  0..=8   / 1,
        POWER_PROFILE        = (4, 3)  0..=2   / 1,
        AUTO_POWER_OFF       = (4, 4)  0..=3   / 1,
        CHARGE_DETECT        = (4, 5)  0..=1   / 1,
        // Apps (domain 5)
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
    live {
        // System, Power, Time, Keypad, Audio, UI & Process Scalars (0xC0..=0xDF)
        OS_VERSION           = 0xC0,
        TICKS_MS             = 0xC1,
        CPU_HZ               = 0xC2,
        STACK_FREE           = 0xC3,
        RAND32               = 0xC4,
        BATT_MV              = 0xC5,
        BATT_PCT             = 0xC6,
        BATT_MA              = 0xC7,
        BATT_TYPE            = 0xC8,
        BATT_ADC             = 0xC9,
        MCU_TEMP             = 0xCA,
        CHARGING             = 0xCB,
        BATT_LOW             = 0xCC,
        BATT_CRIT            = 0xCD,
        BL_WAKE              = 0xCE,
        FLASHLIGHT           = 0xCF,
        KEY_F_ACTIVE         = 0xD0,
        KEY_LOCKED           = 0xD1,
        USB_CONNECTED        = 0xD2,
        AUD_PATH             = 0xD3,
        AUD_PLAYING          = 0xD4,
        AUD_MIC_LEVEL        = 0xD5,
        AUD_CAPTURING        = 0xD6,
        MODAL_STATE          = 0xD7,
        MODAL_HOLD_MS        = 0xD8,
        TRAY_ICON            = 0xD9,
        SELF_PID             = 0xDA,
        PROC_COUNT           = 0xDB,
        // Live Radio & Broadcast FM Scalars (0xE0..=0xF8; 0xFF reserved for UI local sentinels)
        RF_FREQ              = 0xE0,
        RF_RSSI              = 0xE1,
        RF_CARRIER           = 0xE2,
        RF_BANDWIDTH         = 0xE3,
        RF_MODULATION        = 0xE4,
        RF_SQUELCH           = 0xE5,
        RF_TX_POWER          = 0xE6,
        RF_AF_MODE           = 0xE7,
        RF_MIC_MUTE          = 0xE8,
        RF_AGC               = 0xE9,
        RF_TX                = 0xEA,
        RF_MONITOR           = 0xEB,
        RF_WATCH_MODE        = 0xEC,
        RF_TAIL              = 0xED,
        RF_UNLOCK_VFO        = 0xEE,
        RF_GLITCH            = 0xEF,
        RF_SNR               = 0xF0,
        RF_NOISE             = 0xF1,
        RF_AFC               = 0xF2,
        RF_AF_FREQ           = 0xF3,
        RF_AF_AMP            = 0xF4,
        RF_CTCSS_SCAN        = 0xF5,
        FM_RSSI              = 0xF6,
        FM_VOLUME            = 0xF7,
        FM_MUTE              = 0xF8,
    }
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

/// Descriptor of durable property `id`, if one is declared.
pub fn find(id: u8) -> Option<&'static Prop> {
    TABLE.iter().find(|p| p.id == id)
}

/// Position-independent property bounds `(min, max, step)` lookup for `.krx` packages.
#[inline(always)]
pub const fn lookup_bounds(id: u8) -> Option<(u8, u8, u8)> {
    let mut i = 0;
    while i < DEFS.len() {
        if DEFS[i].id == id {
            return Some((DEFS[i].min, DEFS[i].max, DEFS[i].step));
        }
        i += 1;
    }
    None
}

/// Canonical NUL-separated choice option lists and timing lookup tables for OS properties.
pub mod options {
    pub const BACKLIGHT_TIME: &str = "Off\05s\010s\030s\01m\0Always On\0";
    pub const SHADING: &str = "Grayscale\0Dither\0Monochrome\0";
    pub const ANIMATION: &str = "Off\0On\0";

    pub const KEY_BEEP: &str = "Off\0Tone\0Click\0Double\0Warning\0Scroll\0Custom\0";
    pub const BOOT_JINGLE: &str = "Off\0Chime\0Nokia\0Chirp\0GameBoy\0";
    pub const AUDIO_PROFILE: &str = "Flat\0Clean\0Mid\0Boost\0Max\0";

    pub const PTT_MODE: &str = "Momentary\0Toggle / Latch\02x Press Latch\0";
    pub const HOLD_DELAY: &str = "250ms\0350ms\0500ms\0750ms\0";
    pub const REPEAT_RATE: &str = "30ms (Turbo)\050ms (Fast)\080ms (Norm)\0120ms (Slow)\0";
    pub const F_KEY_TIMEOUT: &str = "Toggle\01s\02s\03s\05s\0";
    pub const AUTO_LOCK: &str = "Off\015s\030s\01 min\02 min\0";

    pub const BATTERY_K5: &str = "1600 mAh\02200 mAh\03500 mAh\0External\0";
    pub const BATTERY_K1: &str = "1500 mAh\02500 mAh\0External\0";
    pub const BATTERY_FORMAT: &str =
        "NONE\0VOLTAGE\0PERCENT\0ICON\0I+VOLT\0I+PERC\0VOLT\0PERC\0TEMP\0";
    pub const AUTO_POWER_OFF: &str = "Off\015 Mins\030 Mins\01 Hour\0";
    pub const POWER_PROFILE: &str = "Power Save\0Balanced\0Performance\0";

    pub const SPLASH_TIME: &str = "Skip\00.5s\01.0s\02.0s\03.0s\0";
    pub const BOOT_ANIM: &str = "None\0Dither Fill\0Invert Dissolve\0Curtain Wipe\0";

    pub const LAUNCHER_STYLE: &str = "Grid\0List\0";
    pub const STARTUP_APP: &str = "Home\0Radio\0";

    pub const CPU_CLOCK_48: &str = "16 MHz\024 MHz\032 MHz\048 MHz\0";
    pub const CPU_CLOCK_72: &str = "16 MHz\024 MHz\032 MHz\048 MHz\072 MHz\0";
    pub const UART_BAUD: &str = "115200\038400\0";

    pub const HOLD_DELAY_CS: [u8; 4] = [25, 35, 50, 75];
    pub const REPEAT_RATE_MS: [u8; 4] = [30, 50, 80, 120];
    pub const F_KEY_TIMEOUT_S: [u8; 5] = [0, 1, 2, 3, 5];
    pub const AUTO_LOCK_S: [u8; 5] = [0, 15, 30, 60, 120];
    pub const BACKLIGHT_TIMEOUT_S: [u8; 5] = [0, 5, 10, 30, 60];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_packed(s: &str) -> u8 {
        s.as_bytes().iter().filter(|&&b| b == 0).count() as u8
    }

    #[test]
    fn choice_option_lists_match_property_bounds() {
        for (prop_id, packed) in [
            (BACKLIGHT_TIME, options::BACKLIGHT_TIME),
            (SHADING_MODE, options::SHADING),
            (UI_ANIMATIONS, options::ANIMATION),
            (KEY_BEEP, options::KEY_BEEP),
            (BOOT_JINGLE, options::BOOT_JINGLE),
            (AUDIO_PROFILE, options::AUDIO_PROFILE),
            (PTT_MODE, options::PTT_MODE),
            (HOLD_DELAY, options::HOLD_DELAY),
            (REPEAT_RATE, options::REPEAT_RATE),
            (F_KEY_TIMEOUT, options::F_KEY_TIMEOUT),
            (AUTO_LOCK, options::AUTO_LOCK),
            (BATTERY_FORMAT, options::BATTERY_FORMAT),
            (AUTO_POWER_OFF, options::AUTO_POWER_OFF),
            (POWER_PROFILE, options::POWER_PROFILE),
            (BOOT_SPLASH_DURATION, options::SPLASH_TIME),
            (BOOT_SPLASH_ANIM, options::BOOT_ANIM),
            (LAUNCHER_STYLE, options::LAUNCHER_STYLE),
            (CPU_CLOCK, options::CPU_CLOCK_72),
            (UART_BAUD, options::UART_BAUD),
        ] {
            let prop = find(prop_id).expect("declared property");
            assert_eq!(
                count_packed(packed),
                prop.max + 1,
                "option count mismatch for prop {prop_id:#04x}"
            );
        }
        assert_eq!(
            options::HOLD_DELAY_CS.len() as u8,
            find(HOLD_DELAY).unwrap().max + 1
        );
        assert_eq!(
            options::REPEAT_RATE_MS.len() as u8,
            find(REPEAT_RATE).unwrap().max + 1
        );
        assert_eq!(
            options::F_KEY_TIMEOUT_S.len() as u8,
            find(F_KEY_TIMEOUT).unwrap().max + 1
        );
        assert_eq!(
            options::AUTO_LOCK_S.len() as u8,
            find(AUTO_LOCK).unwrap().max + 1
        );
    }
}
