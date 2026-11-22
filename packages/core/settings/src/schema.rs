//! Declarative Settings Schema: 7 Full Categories, Submenus, Full Labels & Native OptionSheets.
//!
//! Uses full, unshortened option labels matching (and improving on) resident Settings,
//! backed by canonical `abi::prop::options` NUL-separated pools for native `OptionSheet` modals.

use kern_sdk::{format_u16_into, icon, nth_packed_slice, prop};

pub const CAT_COUNT: usize = 7;

// Submenu / Drilldown Action IDs (1..=9 show animated chevron when selected)
pub const ACT_INSTALLED_APPS: u8 = 1;
pub const ACT_SYS_INFO: u8 = 2;
pub const ACT_SYS_MODULES: u8 = 3;
pub const ACT_SUB_SPLASH: u8 = 4;
pub const ACT_SUB_BTN_TWEAKS: u8 = 5;
pub const ACT_SUB_BATT_INFO: u8 = 6;
pub const ACT_SUB_BATT_TWEAKS: u8 = 7;
pub const ACT_SHORTCUTS: u8 = 8;
pub const ACT_BANDPLANS: u8 = 9;
pub const ACT_STORAGE: u8 = 10;
// Leaf-picker / modal Action IDs (11..=21)
pub const ACT_RESET_SETTINGS: u8 = 11;
pub const ACT_REBOOT_RADIO: u8 = 12;
pub const ACT_DFU_MODE: u8 = 13;
pub const ACT_ASSET_SPLASH: u8 = 14;
pub const ACT_ASSET_ROGER: u8 = 15;
pub const ACT_ASSET_JINGLE: u8 = 16;
pub const ACT_ASSET_LANG: u8 = 17;
pub const ACT_DEV_MODEL: u8 = 18;
pub const ACT_DEV_OWNER: u8 = 19;
pub const ACT_DEV_TAG: u8 = 20;
pub const ACT_DEV_RADIO_ID: u8 = 21;

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
pub const CH_AUDIO_PROF: u8 = 3;
pub const CH_AUTO_LOCK: u8 = 4;
pub const CH_F_TIMEOUT: u8 = 5;
pub const CH_HOLD_DELAY: u8 = 6;
pub const CH_REPEAT: u8 = 7;
pub const CH_PTT_MODE: u8 = 8;
pub const CH_BATT_K1: u8 = 9;
pub const CH_BATT_FMT: u8 = 10;
pub const CH_PWR_PROF: u8 = 11;
pub const CH_AUTO_OFF: u8 = 12;
pub const CH_LAUNCHER: u8 = 13;
pub const CH_STARTUP: u8 = 14;
pub const CH_PAGER: u8 = 15;
pub const CH_CPU_CLK: u8 = 16;
pub const CH_UART_BAUD: u8 = 17;
pub const CH_SPLASH_ANIM: u8 = 18;
pub const CH_SPLASH_DUR: u8 = 19;

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

/// Top-level category descriptor (`align(4)`).
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
    Category { icon_id: icon::BUTTONS, start: 14, count: 6, _pad: 0 },
    Category { icon_id: icon::BATTERY, start: 20, count: 5, _pad: 0 },
    Category { icon_id: icon::DEVICE, start: 25, count: 7, _pad: 0 },
    Category { icon_id: icon::APPS, start: 32, count: 6, _pad: 0 },
    Category { icon_id: icon::SYSTEM, start: 38, count: 8, _pad: 0 },
];

// Submenu slice ranges inside `ITEMS`:
pub const SUB_SPLASH_START: u8 = 46;
pub const SUB_SPLASH_COUNT: u8 = 3;
pub const SUB_BTN_TWEAKS_START: u8 = 49;
pub const SUB_BTN_TWEAKS_COUNT: u8 = 2;
pub const SUB_BATT_TWEAKS_START: u8 = 51;
pub const SUB_BATT_TWEAKS_COUNT: u8 = 2;

pub const ITEMS: [Item; 53] = [
    // Display (0..7)
    Item::choice(prop::BACKLIGHT_TIME, 0, CH_BL_TIME),
    Item::num(prop::BACKLIGHT_MAX, 1, UNIT_RAW),
    Item::num(prop::BACKLIGHT_MIN, 2, UNIT_RAW),
    Item::num(prop::CONTRAST, 3, UNIT_PCT),
    Item::toggle(prop::INVERT_PIXELS, 4),
    Item::choice(prop::SHADING_MODE, 5, CH_SHADING),
    Item::toggle(prop::UI_ANIMATIONS, 6),
    // Audio (7..14)
    Item::num(prop::VOLUME, 7, UNIT_RAW),
    Item::num(prop::BEEP_GAIN, 8, UNIT_RAW),
    Item::choice(prop::KEY_BEEP, 9, CH_KEY_BEEP),
    Item::num(prop::MIC_GAIN, 10, UNIT_DB),
    Item::action(ACT_ASSET_ROGER, 11),
    Item::action(ACT_ASSET_JINGLE, 12),
    Item::choice(prop::AUDIO_PROFILE, 13, CH_AUDIO_PROF),
    // Buttons (14..20)
    Item::action(ACT_SHORTCUTS, 14),
    Item::choice(prop::PTT_MODE, 15, CH_PTT_MODE),
    Item::choice(prop::AUTO_LOCK, 16, CH_AUTO_LOCK),
    Item::choice(prop::F_KEY_TIMEOUT, 17, CH_F_TIMEOUT),
    Item::toggle(prop::FAST_SCROLL, 18),
    Item::action(ACT_SUB_BTN_TWEAKS, 19),
    // Battery (20..25)
    Item::choice(prop::BATTERY_TYPE, 20, CH_BATT_K1),
    Item::choice(prop::BATTERY_FORMAT, 21, CH_BATT_FMT),
    Item::choice(prop::AUTO_POWER_OFF, 22, CH_AUTO_OFF),
    Item::action(ACT_SUB_BATT_INFO, 23),
    Item::action(ACT_SUB_BATT_TWEAKS, 24),
    // Device (25..32)
    Item::action(ACT_DEV_MODEL, 25),
    Item::action(ACT_DEV_OWNER, 26),
    Item::action(ACT_DEV_TAG, 27),
    Item::action(ACT_DEV_RADIO_ID, 28),
    Item::action(ACT_ASSET_LANG, 29),
    Item::action(ACT_BANDPLANS, 30),
    Item::action(ACT_SUB_SPLASH, 31),
    // Apps (32..38)
    Item::action(ACT_INSTALLED_APPS, 32),
    Item::choice(prop::LAUNCHER_STYLE, 33, CH_LAUNCHER),
    Item::toggle(prop::SHOW_NUMBERS, 34),
    Item::toggle(prop::SHOW_LABELS, 35),
    Item::choice(prop::STARTUP_APP, 36, CH_STARTUP),
    Item::choice(prop::PAGER_POLICY, 37, CH_PAGER),
    // System (38..46)
    Item::action(ACT_SYS_INFO, 38),
    Item::action(ACT_STORAGE, 39),
    Item::action(ACT_SYS_MODULES, 40),
    Item::choice(prop::CPU_CLOCK, 41, CH_CPU_CLK),
    Item::choice(prop::UART_BAUD, 42, CH_UART_BAUD),
    Item::action(ACT_DFU_MODE, 43),
    Item::action(ACT_RESET_SETTINGS, 44),
    Item::action(ACT_REBOOT_RADIO, 45),
    // Submenu: Boot Splash (46..49)
    Item::action(ACT_ASSET_SPLASH, 46),
    Item::choice(prop::BOOT_SPLASH_ANIM, 47, CH_SPLASH_ANIM),
    Item::choice(prop::BOOT_SPLASH_DURATION, 48, CH_SPLASH_DUR),
    // Submenu: Buttons Tweaks (49..51)
    Item::choice(prop::HOLD_DELAY, 49, CH_HOLD_DELAY),
    Item::choice(prop::REPEAT_RATE, 50, CH_REPEAT),
    // Submenu: Battery Tweaks (51..53)
    Item::toggle(prop::CHARGE_DETECT, 51),
    Item::choice(prop::POWER_PROFILE, 52, CH_PWR_PROF),
];

const CAT_NAMES: &[u8] = b"Display\0Audio\0Buttons\0Battery\0Device\0Apps\0System\0";

const ITEM_LABELS: &[u8] = concat!(
    // Display (0..7)
    "Backlight Time\0Max Brightness\0Min Brightness\0Contrast\0Invert Pixels\0Shading Mode\0Animations\0",
    // Audio (7..14)
    "AF Gain\0Beep Volume\0Key Beep\0Mic Gain\0Roger Beep\0Boot Jingle\0RX Profile\0",
    // Buttons (14..20)
    "Shortcuts\0PTT Mode\0Auto Lock\0Sticky F-Key\0Fast Scroll\0Tweaks\0",
    // Battery (20..25)
    "Capacity\0Status Bar\0Deep Sleep\0Battery Info\0Tweaks\0",
    // Device (25..32)
    "Model\0Owner\0Tag\0Radio ID\0Language\0Bandplans\0Boot Splash\0",
    // Apps (32..38)
    "Installed Apps\0Launcher Style\0Key Badges\0App Titles\0Startup App\0Pager Policy\0",
    // System (38..46)
    "System Info\0Storage\0Modules\0CPU Clock\0UART Baud\0DFU Bootloader\0Reset Settings\0Restart Radio\0",
    // Submenus (44..51)
    "Image\0Animation\0Duration\0",
    "Hold Delay\0Repeat Rate\0",
    "Charge Detect\0Power Profile\0",
)
.as_bytes();

/// Single packed choice option pool; tables are separated by `0x1F` (Unit Separator)
/// so offsets are computed at compile time with zero manual byte-counting drift.
const CHOICE_POOL: &[u8] = concat!(
    "Off\05s\010s\030s\01m\0Always On\0\x1f",
    "Grayscale\0Dither\0Monochrome\0\x1f",
    "Off\0Tone\0Click\0Double\0Warning\0Scroll\0Custom\0\x1f",
    "Flat\0Clean\0Mid\0Boost\0Max\0\x1f",
    "Off\015s\030s\01 min\02 min\0\x1f",
    "Toggle\01s\02s\03s\05s\0\x1f",
    "250ms\0350ms\0500ms\0750ms\0\x1f",
    "30ms (Turbo)\050ms (Fast)\080ms (Norm)\0120ms (Slow)\0\x1f",
    "Momentary\0Toggle / Latch\02x Press Latch\0\x1f",
    "1500 mAh\02500 mAh\0External\0\x1f",
    "None\0Voltage\0Percent\0Icon\0I+Volt\0I+Perc\0Volt\0Perc\0Temp\0\x1f",
    "Power Save\0Balanced\0Performance\0\x1f",
    "Off\015 Mins\030 Mins\01 Hour\0\x1f",
    "Grid\0List\0\x1f",
    "Home\0Radio\0\x1f",
    "Balanced\0Fast\0Max Cache\0\x1f",
    "16 MHz\024 MHz\032 MHz\048 MHz\072 MHz\0\x1f",
    "115200\038400\0\x1f",
    "None\0Dither Fill\0Invert Dissolve\0Curtain Wipe\0\x1f",
    "Skip\00.5s\01.0s\02.0s\03.0s\0\x1f",
)
.as_bytes();

const CHOICE_OFFSETS: [u16; 21] = {
    let mut out = [0u16; 21];
    let mut t = 1usize;
    let mut i = 0usize;
    while i < CHOICE_POOL.len() && t <= 20 {
        if CHOICE_POOL[i] == 0x1F {
            out[t] = (i + 1) as u16;
            t += 1;
        }
        i += 1;
    }
    out
};

#[inline(never)]
pub fn choice_options(table: u8) -> &'static [u8] {
    let i = (table as usize).min(19);
    let s = unsafe { *CHOICE_OFFSETS.get_unchecked(i) } as usize;
    let e = (unsafe { *CHOICE_OFFSETS.get_unchecked(i + 1) } as usize).saturating_sub(1);
    unsafe { core::slice::from_raw_parts(CHOICE_POOL.as_ptr().add(s), e - s) }
}

/// Map `prop::BATTERY_TYPE` raw enum (3, 4, 5 on K1) to OptionSheet index (0, 1, 2).
#[inline(always)]
pub fn batt_type_to_idx(raw: u8) -> u8 {
    match raw {
        3 => 0,
        4 => 1,
        5 => 2,
        _ => raw.min(2),
    }
}

/// Map K1 OptionSheet index (0, 1, 2) back to `prop::BATTERY_TYPE` (3, 4, 5).
#[inline(always)]
pub fn batt_idx_to_type(idx: u8) -> u8 {
    match idx {
        0 => 3,
        1 => 4,
        _ => 5,
    }
}

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
pub fn item_at_slice(start: u8, row: u8) -> Item {
    let idx = (start as usize + row as usize).min(ITEMS.len() - 1);
    unsafe { *ITEMS.get_unchecked(idx) }
}

/// Format an item's current value into `buf` without `core::fmt` or division.
#[inline(never)]
pub fn format_value<'a>(item: Item, val: u8, buf: &'a mut [u8; 16]) -> &'a [u8] {
    match item.kind {
        Kind::Toggle => b"",
        Kind::Choice => {
            let eff = if item.meta == CH_BATT_K1 {
                batt_type_to_idx(val)
            } else {
                val
            };
            let s = nth_packed_slice(choice_options(item.meta), eff);
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
        Kind::Action => b"",
    }
}
