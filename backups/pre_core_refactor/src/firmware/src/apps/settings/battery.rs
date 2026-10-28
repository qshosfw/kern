//! Battery Domain Settings & Submenu Declarations

use super::options::{
    CHOICE_BATTERY_FORMAT, CHOICE_CPU_CLOCK, CHOICE_DEEP_SLEEP, CHOICE_POWER_PROFILE,
    METRIC_BATT_CALIB_REF, METRIC_BATTERY_PCT, METRIC_BATTERY_STATUS, METRIC_BATTERY_VOLTS,
    METRIC_MCU_TEMP, METRIC_VDDA_REF, PAGE_BATTERY_INFO, PAGE_BATTERY_TWEAKS, SettingDef,
    SettingField, SettingKind,
};

pub static BATTERY_K5_OPTIONS: [&'static str; 4] = ["1600 mAh", "2200 mAh", "3500 mAh", "External"];
pub static BATTERY_K5_TYPES: [u8; 4] = [0, 1, 2, 5];

pub static BATTERY_K1_OPTIONS: [&'static str; 3] = ["1500 mAh", "2500 mAh", "External"];
pub static BATTERY_K1_TYPES: [u8; 3] = [3, 4, 5];

pub static BATTERY_FORMAT_OPTIONS: [&'static str; 9] = [
    "NONE", "VOLTAGE", "PERCENT", "ICON", "I+VOLT", "I+PERC", "VOLT", "PERC", "TEMP",
];

pub static AUTO_POWER_OFF_OPTIONS: [&'static str; 4] = ["Off", "15 Mins", "30 Mins", "1 Hour"];

pub static POWER_PROFILE_OPTIONS: [&'static str; 3] = ["Power Save", "Balanced", "Performance"];

pub static BATTERY_ROOT: [SettingDef; 5] = [
    SettingDef {
        title: "Capacity",
        kind: SettingKind::BatteryType,
        field: SettingField::BATTERY_TYPE,
    },
    SettingDef {
        title: "Status Bar",
        kind: SettingKind::Choice(CHOICE_BATTERY_FORMAT),
        field: SettingField::BATTERY_FORMAT,
    },
    SettingDef {
        title: "Deep Sleep",
        kind: SettingKind::Choice(CHOICE_DEEP_SLEEP),
        field: SettingField::AUTO_POWER_OFF,
    },
    SettingDef {
        title: "Info",
        kind: SettingKind::Submenu(PAGE_BATTERY_INFO),
        field: SettingField::None,
    },
    SettingDef {
        title: "Tweaks",
        kind: SettingKind::Submenu(PAGE_BATTERY_TWEAKS),
        field: SettingField::None,
    },
];

pub static BATTERY_INFO: [SettingDef; 6] = [
    SettingDef {
        title: "Voltage",
        kind: SettingKind::Live(METRIC_BATTERY_VOLTS),
        field: SettingField::None,
    },
    SettingDef {
        title: "Battery",
        kind: SettingKind::Live(METRIC_BATTERY_PCT),
        field: SettingField::None,
    },
    SettingDef {
        title: "Status",
        kind: SettingKind::Live(METRIC_BATTERY_STATUS),
        field: SettingField::None,
    },
    SettingDef {
        title: "VDDA Ref",
        kind: SettingKind::Live(METRIC_VDDA_REF),
        field: SettingField::None,
    },
    SettingDef {
        title: "Batt Ref",
        kind: SettingKind::Live(METRIC_BATT_CALIB_REF),
        field: SettingField::None,
    },
    SettingDef {
        title: "Core Temp",
        kind: SettingKind::Live(METRIC_MCU_TEMP),
        field: SettingField::None,
    },
];

pub static BATTERY_TWEAKS: [SettingDef; 4] = [
    SettingDef {
        title: "Calib Tweak",
        kind: SettingKind::CalibTweak,
        field: SettingField::CALIB_TWEAK_MV,
    },
    SettingDef {
        title: "Charge Detect",
        kind: SettingKind::Toggle,
        field: SettingField::CHARGE_DETECT,
    },
    SettingDef {
        title: "Power Profile",
        kind: SettingKind::Choice(CHOICE_POWER_PROFILE),
        field: SettingField::POWER_PROFILE,
    },
    SettingDef {
        title: "CPU Clock",
        kind: SettingKind::Choice(CHOICE_CPU_CLOCK),
        field: SettingField::CPU_CLOCK,
    },
];
