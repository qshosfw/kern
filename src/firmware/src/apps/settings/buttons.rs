//! Buttons Domain Settings & Submenu Declarations

use super::page::{ChoiceId, PageId, SettingDef, SettingField};

pub static PTT_MODE_OPTIONS: &str = "Momentary\0Toggle / Latch\02x Press Latch\0";

pub static HOLD_DELAY_OPTIONS: &str = "250ms\0350ms\0500ms\0750ms\0";

pub static REPEAT_RATE_OPTIONS: &str = "30ms (Turbo)\050ms (Fast)\080ms (Norm)\0120ms (Slow)\0";

pub static F_KEY_TIMEOUT_OPTIONS: &str = "Toggle\01s\02s\03s\05s\0";

pub static AUTO_LOCK_OPTIONS: &str = "Off\015s\030s\01 min\02 min\0";

pub static BUTTONS_ROOT: [SettingDef; 6] = [
    SettingDef::choice("PTT Mode", ChoiceId::PttMode, SettingField::PTT_MODE),
    SettingDef::choice("Auto Lock", ChoiceId::AutoLock, SettingField::AUTO_LOCK),
    SettingDef::choice(
        "Sticky F-Key",
        ChoiceId::FKeyTimeout,
        SettingField::F_KEY_TIMEOUT,
    ),
    SettingDef::toggle("Fast Scroll", SettingField::FAST_SCROLL),
    SettingDef::submenu("Tweaks", PageId::ButtonsTweaks),
    SettingDef::submenu("Shortcuts", PageId::ButtonsShortcuts),
];

pub static BUTTONS_TWEAKS: [SettingDef; 2] = [
    SettingDef::choice("Hold Delay", ChoiceId::HoldDelay, SettingField::HOLD_DELAY),
    SettingDef::choice(
        "Repeat Rate",
        ChoiceId::RepeatRate,
        SettingField::REPEAT_RATE,
    ),
];

pub static BUTTONS_SHORTCUTS: [SettingDef; 16] = [
    SettingDef::shortcut("Side 1 Short", SettingField::F1_SHORT),
    SettingDef::shortcut("Side 1 Long", SettingField::F1_LONG),
    SettingDef::shortcut("Side 2 Short", SettingField::F2_SHORT),
    SettingDef::shortcut("Side 2 Long", SettingField::F2_LONG),
    SettingDef::shortcut("Menu Long", SettingField::M_LONG),
    SettingDef::shortcut("F + 0", SettingField::F_0),
    SettingDef::shortcut("F + 1", SettingField::f_digit(0)),
    SettingDef::shortcut("F + 2", SettingField::f_digit(1)),
    SettingDef::shortcut("F + 3", SettingField::f_digit(2)),
    SettingDef::shortcut("F + 4", SettingField::f_digit(3)),
    SettingDef::shortcut("F + 5", SettingField::f_digit(4)),
    SettingDef::shortcut("F + 6", SettingField::f_digit(5)),
    SettingDef::shortcut("F + 7", SettingField::f_digit(6)),
    SettingDef::shortcut("F + 8", SettingField::f_digit(7)),
    SettingDef::shortcut("F + 9", SettingField::f_digit(8)),
    SettingDef::shortcut("F + *", SettingField::F_STAR),
];
