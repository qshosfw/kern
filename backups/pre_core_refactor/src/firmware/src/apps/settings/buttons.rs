//! Buttons Domain Settings & Submenu Declarations

use super::options::{
    CHOICE_AUTO_LOCK, CHOICE_F_KEY_TIMEOUT, CHOICE_FAST_SCROLL, CHOICE_HOLD_DELAY, CHOICE_M_LONG,
    CHOICE_PTT_MODE, CHOICE_REPEAT_RATE, CHOICE_SHORTCUT, PAGE_BUTTONS_SHORTCUTS,
    PAGE_BUTTONS_TWEAKS, SettingDef, SettingField, SettingKind,
};
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

pub static FAST_SCROLL_OPTIONS: [&'static str; 2] = ["Off", "Accelerated"];

pub static F_KEY_TIMEOUT_OPTIONS: [&'static str; 5] = ["Toggle", "1s", "2s", "3s", "5s"];

pub static AUTO_LOCK_OPTIONS: [&'static str; 5] = ["Off", "15s", "30s", "1 min", "2 min"];

pub static BUTTONS_ROOT: [SettingDef; 6] = [
    SettingDef {
        title: "PTT Mode",
        kind: SettingKind::Choice(CHOICE_PTT_MODE),
        field: SettingField::PTT_MODE,
    },
    SettingDef {
        title: "Auto Lock",
        kind: SettingKind::Choice(CHOICE_AUTO_LOCK),
        field: SettingField::AUTO_LOCK,
    },
    SettingDef {
        title: "Sticky F-Key",
        kind: SettingKind::Choice(CHOICE_F_KEY_TIMEOUT),
        field: SettingField::F_KEY_TIMEOUT,
    },
    SettingDef {
        title: "Fast Scroll",
        kind: SettingKind::Choice(CHOICE_FAST_SCROLL),
        field: SettingField::FAST_SCROLL,
    },
    SettingDef {
        title: "Tweaks",
        kind: SettingKind::Submenu(PAGE_BUTTONS_TWEAKS),
        field: SettingField::None,
    },
    SettingDef {
        title: "Shortcuts",
        kind: SettingKind::Submenu(PAGE_BUTTONS_SHORTCUTS),
        field: SettingField::None,
    },
];

pub static BUTTONS_TWEAKS: [SettingDef; 2] = [
    SettingDef {
        title: "Hold Delay",
        kind: SettingKind::Choice(CHOICE_HOLD_DELAY),
        field: SettingField::HOLD_DELAY,
    },
    SettingDef {
        title: "Repeat Rate",
        kind: SettingKind::Choice(CHOICE_REPEAT_RATE),
        field: SettingField::REPEAT_RATE,
    },
];

pub static BUTTONS_SHORTCUTS: [SettingDef; 16] = [
    SettingDef {
        title: "Side 1 Short",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::F1_SHORT,
    },
    SettingDef {
        title: "Side 1 Long",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::F1_LONG,
    },
    SettingDef {
        title: "Side 2 Short",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::F2_SHORT,
    },
    SettingDef {
        title: "Side 2 Long",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::F2_LONG,
    },
    SettingDef {
        title: "Menu Long",
        kind: SettingKind::Choice(CHOICE_M_LONG),
        field: SettingField::M_LONG,
    },
    SettingDef {
        title: "F + 0",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::F_0,
    },
    SettingDef {
        title: "F + 1",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(0),
    },
    SettingDef {
        title: "F + 2",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(1),
    },
    SettingDef {
        title: "F + 3",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(2),
    },
    SettingDef {
        title: "F + 4",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(3),
    },
    SettingDef {
        title: "F + 5",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(4),
    },
    SettingDef {
        title: "F + 6",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(5),
    },
    SettingDef {
        title: "F + 7",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(6),
    },
    SettingDef {
        title: "F + 8",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(7),
    },
    SettingDef {
        title: "F + 9",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::f_digit(8),
    },
    SettingDef {
        title: "F + *",
        kind: SettingKind::Choice(CHOICE_SHORTCUT),
        field: SettingField::F_STAR,
    },
];
