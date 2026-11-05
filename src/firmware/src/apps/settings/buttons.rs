//! Buttons Domain Settings & Submenu Declarations

use super::page::{PageId, SettingDef, SettingField};
use crate::apps::Shortcut;

pub static SHORTCUT_OPTIONS: [&'static str; 13] = [
    Shortcut::None.name(),
    Shortcut::Flashlight.name(),
    Shortcut::Monitor.name(),
    Shortcut::Scanner.name(),
    Shortcut::Explorer.name(),
    Shortcut::Settings.name(),
    Shortcut::LockKeypad.name(),
    Shortcut::ToggleVfo.name(),
    Shortcut::ToggleTxPower.name(),
    Shortcut::MuteAudio.name(),
    Shortcut::FmRadio.name(),
    Shortcut::TaskManager.name(),
    Shortcut::Fault.name(),
];

pub static PTT_MODE_OPTIONS: [&'static str; 3] = ["Momentary", "Toggle / Latch", "2x Press Latch"];

pub static M_LONG_OPTIONS: [&'static str; 6] = [
    Shortcut::None.name(),
    Shortcut::LockKeypad.name(),
    Shortcut::Settings.name(),
    Shortcut::Monitor.name(),
    Shortcut::Flashlight.name(),
    Shortcut::TaskManager.name(),
];

pub static HOLD_DELAY_OPTIONS: [&'static str; 4] = ["250ms", "350ms", "500ms", "750ms"];

pub static REPEAT_RATE_OPTIONS: [&'static str; 4] =
    ["30ms (Turbo)", "50ms (Fast)", "80ms (Norm)", "120ms (Slow)"];

pub static F_KEY_TIMEOUT_OPTIONS: [&'static str; 5] = ["Toggle", "1s", "2s", "3s", "5s"];

pub static AUTO_LOCK_OPTIONS: [&'static str; 5] = ["Off", "15s", "30s", "1 min", "2 min"];

pub static BUTTONS_ROOT: [SettingDef; 6] = [
    SettingDef::choice("PTT Mode", &PTT_MODE_OPTIONS, SettingField::PTT_MODE),
    SettingDef::choice("Auto Lock", &AUTO_LOCK_OPTIONS, SettingField::AUTO_LOCK),
    SettingDef::choice(
        "Sticky F-Key",
        &F_KEY_TIMEOUT_OPTIONS,
        SettingField::F_KEY_TIMEOUT,
    ),
    SettingDef::toggle("Fast Scroll", SettingField::FAST_SCROLL),
    SettingDef::submenu("Tweaks", PageId::ButtonsTweaks),
    SettingDef::submenu("Shortcuts", PageId::ButtonsShortcuts),
];

pub static BUTTONS_TWEAKS: [SettingDef; 2] = [
    SettingDef::choice("Hold Delay", &HOLD_DELAY_OPTIONS, SettingField::HOLD_DELAY),
    SettingDef::choice(
        "Repeat Rate",
        &REPEAT_RATE_OPTIONS,
        SettingField::REPEAT_RATE,
    ),
];

pub static BUTTONS_SHORTCUTS: [SettingDef; 16] = [
    SettingDef::shortcut("Side 1 Short", SettingField::F1_SHORT),
    SettingDef::shortcut("Side 1 Long", SettingField::F1_LONG),
    SettingDef::shortcut("Side 2 Short", SettingField::F2_SHORT),
    SettingDef::shortcut("Side 2 Long", SettingField::F2_LONG),
    SettingDef::choice("Menu Long", &M_LONG_OPTIONS, SettingField::M_LONG),
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
