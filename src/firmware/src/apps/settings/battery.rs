//! Battery Domain Settings & Submenu Declarations

use super::page::{MetricId, PageId, RANGE_CALIB_TWEAK, SettingDef, SettingField};

pub static BATTERY_K5_OPTIONS: [&'static str; 4] = ["1600 mAh", "2200 mAh", "3500 mAh", "External"];
pub static BATTERY_K5_TYPES: [u8; 4] = [0, 1, 2, 5];

pub static BATTERY_K1_OPTIONS: [&'static str; 3] = ["1500 mAh", "2500 mAh", "External"];
pub static BATTERY_K1_TYPES: [u8; 3] = [3, 4, 5];

pub static BATTERY_FORMAT_OPTIONS: [&'static str; 9] = [
    "NONE", "VOLTAGE", "PERCENT", "ICON", "I+VOLT", "I+PERC", "VOLT", "PERC", "TEMP",
];

pub static AUTO_POWER_OFF_OPTIONS: [&'static str; 4] = ["Off", "15 Mins", "30 Mins", "1 Hour"];

pub static POWER_PROFILE_OPTIONS: [&'static str; 3] = ["Power Save", "Balanced", "Performance"];

pub static CPU_CLOCK_OPTIONS: [&'static str; 2] = ["48 MHz", "72 MHz"];

#[inline(always)]
pub fn battery_options_and_types(nav_layout: u8) -> (&'static [&'static str], &'static [u8]) {
    if nav_layout == 0 {
        (&BATTERY_K1_OPTIONS, &BATTERY_K1_TYPES)
    } else {
        (&BATTERY_K5_OPTIONS, &BATTERY_K5_TYPES)
    }
}

pub static BATTERY_ROOT: [SettingDef; 5] = [
    SettingDef::battery_type("Capacity"),
    SettingDef::choice(
        "Status Bar",
        &BATTERY_FORMAT_OPTIONS,
        SettingField::BATTERY_FORMAT,
    ),
    SettingDef::choice(
        "Deep Sleep",
        &AUTO_POWER_OFF_OPTIONS,
        SettingField::AUTO_POWER_OFF,
    ),
    SettingDef::submenu("Info", PageId::BatteryInfo),
    SettingDef::submenu("Tweaks", PageId::BatteryTweaks),
];

pub static BATTERY_INFO: [SettingDef; 6] = [
    SettingDef::metric("Voltage", MetricId::BatteryVoltage),
    SettingDef::metric("Battery", MetricId::BatteryPercentage),
    SettingDef::metric("Status", MetricId::BatteryStatus),
    SettingDef::metric("VDDA Ref", MetricId::BatteryVdda),
    SettingDef::metric("Batt Ref", MetricId::BatteryCalibRef),
    SettingDef::metric("Core Temp", MetricId::BatteryCoreTemp),
];

pub static BATTERY_TWEAKS: [SettingDef; 4] = [
    SettingDef::range(
        "Calib Tweak",
        &RANGE_CALIB_TWEAK,
        SettingField::CALIB_TWEAK_MV,
    ),
    SettingDef::toggle("Charge Detect", SettingField::CHARGE_DETECT),
    SettingDef::choice(
        "Power Profile",
        &POWER_PROFILE_OPTIONS,
        SettingField::POWER_PROFILE,
    ),
    SettingDef::choice("CPU Clock", &CPU_CLOCK_OPTIONS, SettingField::CPU_CLOCK),
];
