//! Applications & Launcher Domain Settings Declarations

use super::options::{
    CHOICE_LAUNCHER_STYLE, CHOICE_OFF_ON, CHOICE_STARTUP_APP, CHOICE_VFO_DUAL_WATCH, SettingDef,
    SettingField, SettingKind,
};

pub static LAUNCHER_STYLE_OPTIONS: [&'static str; 2] = ["Grid", "List"];
pub static STARTUP_APP_OPTIONS: [&'static str; 2] = ["Launcher", "Radio"];
pub static VFO_DUAL_WATCH_OPTIONS: [&'static str; 3] = ["Off", "Dual", "Full"];

pub static APPS_DEFS: [SettingDef; 5] = [
    SettingDef {
        title: "Style",
        kind: SettingKind::Choice(CHOICE_LAUNCHER_STYLE),
        field: SettingField::LAUNCHER_STYLE,
    },
    SettingDef {
        title: "Key Badges",
        kind: SettingKind::Choice(CHOICE_OFF_ON),
        field: SettingField::SHOW_NUMBERS,
    },
    SettingDef {
        title: "App Titles",
        kind: SettingKind::Choice(CHOICE_OFF_ON),
        field: SettingField::SHOW_LABELS,
    },
    SettingDef {
        title: "Startup App",
        kind: SettingKind::Choice(CHOICE_STARTUP_APP),
        field: SettingField::STARTUP_APP,
    },
    SettingDef {
        title: "Dual Watch",
        kind: SettingKind::Choice(CHOICE_VFO_DUAL_WATCH),
        field: SettingField::VFO_DUAL_WATCH,
    },
];
