//! Declarative Settings Schema: 8 Categories, Submenus & Canonical ABI Choice Pools.
//!
//! Pulls choice option lists and numeric bounds directly from `abi::prop` at compile time
//! into a single zero-relocation `.rodata` pool — single source of truth with firmware.

use kern_sdk::{format_u16_into, icon, nth_packed_slice, prop};

pub const CAT_COUNT: usize = 9;
pub const CAT_LANGUAGE: u8 = 6;
pub const CAT_SECURITY: u8 = 8;

// Submenu / Drilldown Action IDs (1..=9 show animated chevron when selected):
// 1..=6 = view drilldowns, 7..=9 = declarative submenus.
pub const ACT_INSTALLED_APPS: u8 = 1;
pub const ACT_SYS_MODULES: u8 = 2;
pub const ACT_SHORTCUTS: u8 = 3;
pub const ACT_BANDPLANS: u8 = 4;
pub const ACT_SUB_BATT_INFO: u8 = 5;
pub const ACT_SYS_INFO: u8 = 6;
pub const ACT_SUB_SPLASH: u8 = 7;
pub const ACT_SUB_BTN_TWEAKS: u8 = 8;
pub const ACT_SUB_BATT_TWEAKS: u8 = 9;
// Leaf-picker / modal / security Action IDs (11..=28)
pub const ACT_RESET_SETTINGS: u8 = 11;
pub const ACT_REBOOT_RADIO: u8 = 12;
pub const ACT_ASSET_SPLASH: u8 = 13;
pub const ACT_ASSET_ROGER: u8 = 14;
pub const ACT_ASSET_JINGLE: u8 = 15;
pub const ACT_DEV_MODEL: u8 = 17;
pub const ACT_DEV_OWNER: u8 = 18;
pub const ACT_DEV_TAG: u8 = 19;
pub const ACT_DEV_RADIO_ID: u8 = 20;
// Security actions ordered to align toggle bits directly with `SEC_FLAG_*`:
// 21=Passcode, 22=ShowLen (1<<1), 23=Dimmed (1<<2), 24=OnlyDigits (1<<3), 25=MaxTries.
pub const ACT_SEC_PASSCODE: u8 = 21;
pub const ACT_SEC_EXPOSE: u8 = 22;
pub const ACT_SEC_STEALTH: u8 = 23;
pub const ACT_SEC_DIGITS: u8 = 24;
pub const ACT_SEC_MAX_TRIES: u8 = 25;
pub const ACT_SYS_STORAGE: u8 = 26;
pub const ACT_SYS_CALIB: u8 = 27;
pub const ACT_SYS_DFU: u8 = 28;

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
pub const UNIT_MV: u8 = 3;

/// Synthetic property ID for Battery Calib Tweak (`i16`, `-500..=+500 mV`, step 10).
pub const PROP_BATT_TWEAK: u8 = 0xFF;

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
pub const CH_BATT_K5: u8 = 10;
pub const CH_BATT_FMT: u8 = 11;
pub const CH_PWR_PROF: u8 = 12;
pub const CH_AUTO_OFF: u8 = 13;
pub const CH_LAUNCHER: u8 = 14;
pub const CH_STARTUP: u8 = 15;
pub const CH_PAGER: u8 = 16;
pub const CH_CPU_CLK: u8 = 17;
pub const CH_UART_BAUD: u8 = 18;
pub const CH_SPLASH_ANIM: u8 = 19;
pub const CH_SPLASH_DUR: u8 = 20;

const CHOICE_COUNT: usize = 21;

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
    Category { icon_id: icon::DEVICE, start: 25, count: 6, _pad: 0 },
    Category { icon_id: icon::APPS, start: 31, count: 6, _pad: 0 },
    Category { icon_id: icon::GLOBE, start: 0, count: 0, _pad: 0 },
    Category { icon_id: icon::SYSTEM, start: 37, count: 9, _pad: 0 },
    Category { icon_id: icon::SECURITY, start: 46, count: 5, _pad: 0 },
];

// Submenu slice ranges inside `ITEMS`:
pub const SUB_SPLASH_START: u8 = 51;
pub const SUB_SPLASH_COUNT: u8 = 3;
pub const SUB_BTN_TWEAKS_START: u8 = 54;
pub const SUB_BTN_TWEAKS_COUNT: u8 = 2;
pub const SUB_BATT_TWEAKS_START: u8 = 56;
pub const SUB_BATT_TWEAKS_COUNT: u8 = 3;

pub const ITEMS: [Item; 59] = [
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
    // Device (25..31)
    Item::action(ACT_DEV_MODEL, 25),
    Item::action(ACT_DEV_OWNER, 26),
    Item::action(ACT_DEV_TAG, 27),
    Item::action(ACT_DEV_RADIO_ID, 28),
    Item::action(ACT_BANDPLANS, 29),
    Item::action(ACT_SUB_SPLASH, 30),
    // Apps (31..37)
    Item::action(ACT_INSTALLED_APPS, 31),
    Item::choice(prop::LAUNCHER_STYLE, 32, CH_LAUNCHER),
    Item::toggle(prop::SHOW_NUMBERS, 33),
    Item::toggle(prop::SHOW_LABELS, 34),
    Item::choice(prop::STARTUP_APP, 35, CH_STARTUP),
    Item::choice(prop::PAGER_POLICY, 36, CH_PAGER),
    // System (37..46)
    Item::action(ACT_SYS_INFO, 37),
    Item::action(ACT_SYS_STORAGE, 38),
    Item::action(ACT_SYS_CALIB, 39),
    Item::action(ACT_SYS_MODULES, 40),
    Item::action(ACT_SYS_DFU, 41),
    Item::choice(prop::CPU_CLOCK, 42, CH_CPU_CLK),
    Item::choice(prop::UART_BAUD, 43, CH_UART_BAUD),
    Item::action(ACT_RESET_SETTINGS, 44),
    Item::action(ACT_REBOOT_RADIO, 45),
    // Security (46..51)
    Item::action(ACT_SEC_PASSCODE, 46),
    Item::action(ACT_SEC_EXPOSE, 47),
    Item::action(ACT_SEC_DIGITS, 48),
    Item::action(ACT_SEC_STEALTH, 49),
    Item::action(ACT_SEC_MAX_TRIES, 50),
    // Submenu: Boot Splash (51..54)
    Item::action(ACT_ASSET_SPLASH, 51),
    Item::choice(prop::BOOT_SPLASH_ANIM, 52, CH_SPLASH_ANIM),
    Item::choice(prop::BOOT_SPLASH_DURATION, 53, CH_SPLASH_DUR),
    // Submenu: Buttons Tweaks (54..56)
    Item::choice(prop::HOLD_DELAY, 54, CH_HOLD_DELAY),
    Item::choice(prop::REPEAT_RATE, 55, CH_REPEAT),
    // Submenu: Battery Tweaks (56..59)
    Item::num(PROP_BATT_TWEAK, 56, UNIT_MV),
    Item::toggle(prop::CHARGE_DETECT, 57),
    Item::choice(prop::POWER_PROFILE, 58, CH_PWR_PROF),
];

const CAT_NAMES: &[u8] = b"Display\0Audio\0Buttons\0Battery\0Device\0Apps\0Language\0System\0Security\0";

const ITEM_LABELS: &[u8] = concat!(
    // Display (0..7)
    "Backlight Time\0Max Brightness\0Min Brightness\0Contrast\0Invert Pixels\0Shading Mode\0Animations\0",
    // Audio (7..14)
    "AF Gain\0Beep Volume\0Key Beep\0Mic Gain\0Roger Beep\0Boot Jingle\0RX Profile\0",
    // Buttons (14..20)
    "Shortcuts\0PTT Mode\0Auto Lock\0Sticky F-Key\0Fast Scroll\0Tweaks\0",
    // Battery (20..25)
    "Capacity\0Status Bar\0Deep Sleep\0Battery Info\0Tweaks\0",
    // Device (25..31)
    "Model\0Owner\0Tag\0Radio ID\0Bandplans\0Boot Splash\0",
    // Apps (31..37)
    "Installed Apps\0Launcher Style\0Key Badges\0App Titles\0Startup App\0Pager Policy\0",
    // System (37..46)
    "System Info\0Storage\0Calibration\0Services\0DFU Update\0CPU Clock\0UART Baud\0Reset Settings\0Restart Radio\0",
    // Security (46..51)
    "Passcode\0Show Length\0Only Digits\0Dimmed\0Max Tries\0",
    // Submenus (51..59)
    "Image\0Animation\0Duration\0",
    "Hold Delay\0Repeat Rate\0",
    "Calib Tweak\0Charge Detect\0Power Profile\0",
)
.as_bytes();

/// Canonical choice option sources pulled directly from `abi::prop::options`.
const CHOICE_SRCS: [&[u8]; CHOICE_COUNT] = [
    prop::options::BACKLIGHT_TIME.as_bytes(),
    prop::options::SHADING.as_bytes(),
    prop::options::KEY_BEEP.as_bytes(),
    prop::options::AUDIO_PROFILE.as_bytes(),
    prop::options::AUTO_LOCK.as_bytes(),
    prop::options::F_KEY_TIMEOUT.as_bytes(),
    prop::options::HOLD_DELAY.as_bytes(),
    prop::options::REPEAT_RATE.as_bytes(),
    prop::options::PTT_MODE.as_bytes(),
    prop::options::BATTERY_K1.as_bytes(),
    prop::options::BATTERY_K5.as_bytes(),
    prop::options::BATTERY_FORMAT.as_bytes(),
    prop::options::POWER_PROFILE.as_bytes(),
    prop::options::AUTO_POWER_OFF.as_bytes(),
    prop::options::LAUNCHER_STYLE.as_bytes(),
    prop::options::STARTUP_APP.as_bytes(),
    b"Balanced\0Fast\0Max Cache\0",
    prop::options::CPU_CLOCK_72.as_bytes(),
    prop::options::UART_BAUD.as_bytes(),
    prop::options::BOOT_ANIM.as_bytes(),
    prop::options::SPLASH_TIME.as_bytes(),
];

const CHOICE_POOL_LEN: usize = {
    let mut sum = 0usize;
    let mut i = 0usize;
    while i < CHOICE_COUNT {
        sum += CHOICE_SRCS[i].len();
        i += 1;
    }
    sum
};

/// Flat zero-relocation byte pool packed at compile time from `abi::prop::options`.
const CHOICE_POOL: [u8; CHOICE_POOL_LEN] = {
    let mut out = [0u8; CHOICE_POOL_LEN];
    let mut cur = 0usize;
    let mut t = 0usize;
    while t < CHOICE_COUNT {
        let src = CHOICE_SRCS[t];
        let mut j = 0usize;
        while j < src.len() {
            out[cur + j] = src[j];
            j += 1;
        }
        cur += src.len();
        t += 1;
    }
    out
};

const CHOICE_OFFSETS: [u16; CHOICE_COUNT + 1] = {
    let mut out = [0u16; CHOICE_COUNT + 1];
    let mut cur = 0usize;
    let mut t = 0usize;
    while t < CHOICE_COUNT {
        out[t] = cur as u16;
        cur += CHOICE_SRCS[t].len();
        t += 1;
    }
    out[CHOICE_COUNT] = cur as u16;
    out
};

/// Compile-time filtered bounds for `Kind::Num` items only — still sourced 100% from
/// `abi::prop::DEFS` without embedding 37 non-numeric property descriptors in `.rodata`.
const NUM_BOUNDS: [prop::Prop; 6] = {
    let mut out = [prop::Prop { id: 0, min: 0, max: 0, step: 1 }; 6];
    let mut k = 0usize;
    let mut i = 0usize;
    while i < ITEMS.len() {
        if matches!(ITEMS[i].kind, Kind::Num) && ITEMS[i].prop_id != PROP_BATT_TWEAK {
            let Some((min, max, step)) = prop::lookup_bounds(ITEMS[i].prop_id) else {
                panic!("missing Num prop bounds");
            };
            out[k] = prop::Prop { id: ITEMS[i].prop_id, min, max, step };
            k += 1;
        }
        i += 1;
    }
    assert!(k == 6);
    out
};

/// Single non-inlined bounds lookup for numeric items (zero duplication in `.rodata`).
#[inline(never)]
pub fn prop_bounds(id: u8) -> Option<(u8, u8, u8)> {
    let mut i = 0usize;
    while i < NUM_BOUNDS.len() {
        let p = unsafe { *NUM_BOUNDS.get_unchecked(i) };
        if p.id == id {
            return Some((p.min, p.max, p.step));
        }
        i += 1;
    }
    None
}

#[inline(never)]
pub fn choice_options(table: u8) -> &'static [u8] {
    let i = (table as usize).min(CHOICE_COUNT - 1);
    let s = unsafe { *CHOICE_OFFSETS.get_unchecked(i) } as usize;
    let e = unsafe { *CHOICE_OFFSETS.get_unchecked(i + 1) } as usize;
    unsafe { core::slice::from_raw_parts(CHOICE_POOL.as_ptr().add(s), e - s) }
}

/// Resolve `(choice_table, selected_index)` for `item`, switching K1 vs K5 battery options dynamically.
#[inline(never)]
pub fn resolve_choice(meta: u8, raw: u8, is_k1: bool) -> (u8, u8) {
    if meta != CH_BATT_K1 {
        return (meta, raw);
    }
    if is_k1 {
        (CH_BATT_K1, if raw >= 3 { (raw - 3).min(2) } else { raw })
    } else {
        (CH_BATT_K5, if raw <= 2 { raw } else if raw == 5 { 3 } else { 0 })
    }
}

/// Map OptionSheet index back to `prop::BATTERY_TYPE` raw enum for K1 or K5.
#[inline(always)]
pub fn batt_idx_to_type(idx: u8, is_k1: bool) -> u8 {
    if is_k1 {
        if idx < 2 { idx + 3 } else { 5 }
    } else if idx <= 2 {
        idx
    } else {
        5
    }
}

#[inline(never)]
pub fn category(idx: u8) -> Category {
    let i = (idx as usize).min(CAT_COUNT - 1);
    unsafe { *CATEGORIES.get_unchecked(i) }
}

#[inline(never)]
pub fn cat_name(idx: u8) -> &'static [u8] {
    nth_packed_slice(CAT_NAMES, idx)
}

#[inline(never)]
pub fn item_label(idx: u8) -> &'static [u8] {
    nth_packed_slice(ITEM_LABELS, idx)
}

#[inline(never)]
pub fn item_at_slice(start: u8, row: u8) -> Item {
    let idx = (start as usize + row as usize).min(ITEMS.len() - 1);
    unsafe { *ITEMS.get_unchecked(idx) }
}

/// Format an item's current value into `buf` without `core::fmt` or division.
#[inline(never)]
pub fn format_value<'a>(item: Item, val: i16, is_k1: bool, buf: &'a mut [u8; 16]) -> &'a [u8] {
    match item.kind {
        Kind::Choice => {
            let (tbl, eff) = resolve_choice(item.meta, val as u8, is_k1);
            nth_packed_slice(choice_options(tbl), eff)
        }
        Kind::Num => {
            let suffix = nth_packed_slice(b"\0%\0dB\0mV\0", item.meta);
            let neg = val < 0;
            let mag = if neg { (-val) as u16 } else { val as u16 };
            let mut n = format_u16_into(mag, 0, suffix, buf).len();
            if item.meta == UNIT_MV && val != 0 && n < 16 {
                let p = buf.as_mut_ptr();
                let mut i = n;
                while i > 0 {
                    unsafe { *p.add(i) = *p.add(i - 1) };
                    i -= 1;
                }
                unsafe { *p = if neg { b'-' } else { b'+' } };
                n += 1;
            }
            unsafe { core::slice::from_raw_parts(buf.as_ptr(), n) }
        }
        _ => b"",
    }
}
