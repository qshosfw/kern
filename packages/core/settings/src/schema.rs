//! Declarative Settings Schema: Categories, Properties, Choices, and Value Formatters.
//!
//! Compact 4-byte `align(4)` descriptors + packed NUL-separated string pools:
//! zero heap, zero `core::fmt`, zero GOT relocations, zero jump-table bloat.

use kern_sdk::{format_u16_into, icon, nth_packed_slice, prop};

pub const CAT_COUNT: usize = 6;

pub const ACT_INSTALLED_APPS: u8 = 1;
pub const ACT_DEVICE_INFO: u8 = 2;
pub const ACT_RESET_SETTINGS: u8 = 3;
pub const ACT_REBOOT_RADIO: u8 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    Toggle = 0,
    Num = 1,
    Choice = 2,
    Action = 3,
}

pub const UNIT_RAW: u8 = 0;
pub const UNIT_PCT: u8 = 1;
pub const UNIT_DB: u8 = 2;

pub const CH_BL_TIME: u8 = 0;
pub const CH_SHADING: u8 = 1;
pub const CH_KEY_BEEP: u8 = 2;
pub const CH_BOOT_JINGLE: u8 = 3;
pub const CH_AUDIO_PROF: u8 = 4;
pub const CH_NAV_LAYOUT: u8 = 5;
pub const CH_AUTO_LOCK: u8 = 6;
pub const CH_F_TIMEOUT: u8 = 7;
pub const CH_HOLD_DELAY: u8 = 8;
pub const CH_REPEAT: u8 = 9;
pub const CH_PTT_MODE: u8 = 10;
pub const CH_BATT_TYPE: u8 = 11;
pub const CH_BATT_FMT: u8 = 12;
pub const CH_PWR_PROF: u8 = 13;
pub const CH_AUTO_OFF: u8 = 14;
pub const CH_LAUNCHER: u8 = 15;
pub const CH_STARTUP: u8 = 16;
pub const CH_PAGER: u8 = 17;
pub const CH_CPU_CLK: u8 = 18;
pub const CH_UART_BAUD: u8 = 19;

/// Compact 4-byte setting descriptor (`align(4)` avoids Cortex-M0+ `__aeabi_uread4`).
#[derive(Clone, Copy)]
#[repr(C, align(4))]
pub struct Item {
    pub kind: Kind,
    pub prop_id: u8,
    pub label_idx: u8,
    pub meta: u8,
}

impl Item {
    pub const fn toggle(prop_id: u8, label_idx: u8) -> Self {
        Self { kind: Kind::Toggle, prop_id, label_idx, meta: 0 }
    }
    pub const fn num(prop_id: u8, label_idx: u8, unit: u8) -> Self {
        Self { kind: Kind::Num, prop_id, label_idx, meta: unit }
    }
    pub const fn choice(prop_id: u8, label_idx: u8, table: u8) -> Self {
        Self { kind: Kind::Choice, prop_id, label_idx, meta: table }
    }
    pub const fn action(action_id: u8, label_idx: u8) -> Self {
        Self { kind: Kind::Action, prop_id: 0, label_idx, meta: action_id }
    }
}

/// Top-level category descriptor (4 bytes, `align(4)` avoids `__aeabi_uread4`).
#[derive(Clone, Copy)]
#[repr(C, align(4))]
pub struct Category {
    pub icon_id: u8,
    pub start: u8,
    pub count: u8,
    pub _pad: u8,
}

pub const CATEGORIES: [Category; CAT_COUNT] = [
    Category { icon_id: icon::DISPLAY, start: 0, count: 7, _pad: 0 },
    Category { icon_id: icon::AUDIO, start: 7, count: 7, _pad: 0 },
    Category { icon_id: icon::KEYPAD, start: 14, count: 8, _pad: 0 },
    Category { icon_id: icon::BATTERY, start: 22, count: 5, _pad: 0 },
    Category { icon_id: icon::APPS, start: 27, count: 6, _pad: 0 },
    Category { icon_id: icon::SYSTEM, start: 33, count: 5, _pad: 0 },
];

pub const ITEMS: [Item; 38] = [
    // Display (0..7)
    Item::choice(prop::BACKLIGHT_TIME, 0, CH_BL_TIME),
    Item::num(prop::BACKLIGHT_MAX, 1, UNIT_RAW),
    Item::num(prop::BACKLIGHT_MIN, 2, UNIT_RAW),
    Item::num(prop::CONTRAST, 3, UNIT_PCT),
    Item::toggle(prop::INVERT_PIXELS, 4),
    Item::toggle(prop::UI_ANIMATIONS, 5),
    Item::choice(prop::SHADING_MODE, 6, CH_SHADING),
    // Audio (7..14)
    Item::num(prop::VOLUME, 7, UNIT_RAW),
    Item::choice(prop::KEY_BEEP, 8, CH_KEY_BEEP),
    Item::num(prop::BEEP_GAIN, 9, UNIT_RAW),
    Item::num(prop::MIC_GAIN, 10, UNIT_DB),
    Item::num(prop::ROGER_BEEP, 11, UNIT_RAW),
    Item::choice(prop::AUDIO_PROFILE, 12, CH_AUDIO_PROF),
    Item::choice(prop::BOOT_JINGLE, 13, CH_BOOT_JINGLE),
    // Keys (14..22)
    Item::choice(prop::NAV_LAYOUT, 14, CH_NAV_LAYOUT),
    Item::choice(prop::AUTO_LOCK, 15, CH_AUTO_LOCK),
    Item::choice(prop::F_KEY_TIMEOUT, 16, CH_F_TIMEOUT),
    Item::toggle(prop::FAST_SCROLL, 17),
    Item::choice(prop::HOLD_DELAY, 18, CH_HOLD_DELAY),
    Item::choice(prop::REPEAT_RATE, 19, CH_REPEAT),
    Item::choice(prop::PTT_MODE, 20, CH_PTT_MODE),
    Item::toggle(prop::F_DRAWER, 21),
    // Battery (22..27)
    Item::choice(prop::BATTERY_TYPE, 22, CH_BATT_TYPE),
    Item::choice(prop::BATTERY_FORMAT, 23, CH_BATT_FMT),
    Item::choice(prop::POWER_PROFILE, 24, CH_PWR_PROF),
    Item::choice(prop::AUTO_POWER_OFF, 25, CH_AUTO_OFF),
    Item::toggle(prop::CHARGE_DETECT, 26),
    // Apps (27..33)
    Item::action(ACT_INSTALLED_APPS, 27),
    Item::choice(prop::LAUNCHER_STYLE, 28, CH_LAUNCHER),
    Item::toggle(prop::SHOW_NUMBERS, 29),
    Item::toggle(prop::SHOW_LABELS, 30),
    Item::choice(prop::STARTUP_APP, 31, CH_STARTUP),
    Item::choice(prop::PAGER_POLICY, 32, CH_PAGER),
    // System (33..38)
    Item::action(ACT_DEVICE_INFO, 33),
    Item::choice(prop::CPU_CLOCK, 34, CH_CPU_CLK),
    Item::choice(prop::UART_BAUD, 35, CH_UART_BAUD),
    Item::action(ACT_RESET_SETTINGS, 36),
    Item::action(ACT_REBOOT_RADIO, 37),
];

const CAT_NAMES: &[u8] = b"Display\0Audio\0Keys\0Battery\0Apps\0System\0";

const ITEM_LABELS: &[u8] = concat!(
    "Backlight\0Max Bright\0Min Bright\0Contrast\0Invert LCD\0Animations\0Shading\0",
    "AF Volume\0Key Beep\0Beep Gain\0Mic Gain\0Roger Beep\0Audio EQ\0Boot Chime\0",
    "Nav Layout\0Auto Lock\0F Timeout\0Fast Scroll\0Hold Delay\0Repeat Rate\0PTT Mode\0F Drawer\0",
    "Cell Type\0Bar Format\0Power Mode\0Auto Off\0Chg Detect\0",
    "Installed\0Home Style\0Numbers\0Labels\0Startup\0Pager\0",
    "Device Info\0CPU Clock\0UART Baud\0Reset All\0Restart\0",
)
.as_bytes();

/// Single packed choice pool indexed by `CHOICE_BASES[table] + val` (0 jump-table code).
const CHOICE_BASES: [u8; 20] = [
    0,  // CH_BL_TIME (6)
    6,  // CH_SHADING (3)
    9,  // CH_KEY_BEEP (7)
    16, // CH_BOOT_JINGLE (5)
    21, // CH_AUDIO_PROF (5)
    26, // CH_NAV_LAYOUT (2)
    28, // CH_AUTO_LOCK (5)
    33, // CH_F_TIMEOUT (5)
    38, // CH_HOLD_DELAY (4)
    42, // CH_REPEAT (4)
    46, // CH_PTT_MODE (3)
    49, // CH_BATT_TYPE (3)
    52, // CH_BATT_FMT (9)
    61, // CH_PWR_PROF (3)
    64, // CH_AUTO_OFF (4)
    68, // CH_LAUNCHER (2)
    70, // CH_STARTUP (2)
    72, // CH_PAGER (3)
    75, // CH_CPU_CLK (5)
    80, // CH_UART_BAUD (2)
];

const CHOICE_POOL: &[u8] = concat!(
    "Off\05s\010s\030s\01m\0Always\0",
    "Gray\0Dither\0Mono\0",
    "Off\0Tone\0Click\0Double\0Warn\0Scroll\0Custom\0",
    "Off\0Chime\0Nokia\0Chirp\0GameBoy\0",
    "Flat\0Clean\0Mid\0Boost\0Max\0",
    "Std\0Alt\0",
    "Off\015s\030s\01m\02m\0",
    "Toggle\01s\02s\03s\05s\0",
    "250ms\0350ms\0500ms\0750ms\0",
    "30ms\050ms\080ms\0120ms\0",
    "Moment\0Latch\02xLatch\0",
    "1500mAh\02500mAh\0Ext\0",
    "None\0Volt\0Pct\0Icon\0I+V\0I+%\0V2\0%2\0Temp\0",
    "Save\0Bal\0Perf\0",
    "Off\015m\030m\01h\0",
    "Grid\0List\0",
    "Home\0Radio\0",
    "Bal\0Fast\0Max\0",
    "16M\024M\032M\048M\072M\0",
    "115k\038k\0",
)
.as_bytes();

#[inline(always)]
pub fn category(idx: u8) -> Category {
    let i = (idx as usize).min(CAT_COUNT - 1);
    unsafe { *CATEGORIES.get_unchecked(i) }
}

#[inline(always)]
pub fn cat_name(idx: u8) -> &'static [u8] {
    nth_packed_slice(CAT_NAMES, idx)
}

#[inline(always)]
pub fn item_label(idx: u8) -> &'static [u8] {
    nth_packed_slice(ITEM_LABELS, idx)
}

#[inline(always)]
pub fn item_at(cat_idx: u8, row: u8) -> Item {
    let c = category(cat_idx);
    let idx = (c.start as usize + row as usize).min(ITEMS.len() - 1);
    unsafe { *ITEMS.get_unchecked(idx) }
}

/// Format an item's current value into `buf` without `core::fmt` or division.
#[inline(never)]
pub fn format_value<'a>(item: Item, val: u8, buf: &'a mut [u8; 16]) -> &'a [u8] {
    match item.kind {
        Kind::Toggle => {
            if val != 0 {
                b"On"
            } else {
                b"Off"
            }
        }
        Kind::Choice => {
            let base = unsafe { *CHOICE_BASES.get_unchecked((item.meta as usize).min(19)) };
            let s = nth_packed_slice(CHOICE_POOL, base.saturating_add(val));
            if s.is_empty() {
                format_u16_into(val as u16, 0, b"", buf)
            } else {
                s
            }
        }
        Kind::Num => {
            let suffix: &[u8] = match item.meta {
                UNIT_PCT => b"%",
                UNIT_DB => b"dB",
                _ => b"",
            };
            format_u16_into(val as u16, 0, suffix, buf)
        }
        Kind::Action => match item.meta {
            ACT_INSTALLED_APPS | ACT_DEVICE_INFO => b">",
            _ => b"",
        },
    }
}
