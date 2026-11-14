//! Battery Domain Settings & Submenu Declarations

use super::page::{ChoiceId, MetricId, PageId, RangeId, SettingDef, SettingField};

pub static BATTERY_K5_OPTIONS: &str = "1600 mAh\02200 mAh\03500 mAh\0External\0";
pub static BATTERY_K5_TYPES: [u8; 4] = [0, 1, 2, 5];

pub static BATTERY_K1_OPTIONS: &str = "1500 mAh\02500 mAh\0External\0";
pub static BATTERY_K1_TYPES: [u8; 3] = [3, 4, 5];

pub static BATTERY_FORMAT_OPTIONS: &str =
    "NONE\0VOLTAGE\0PERCENT\0ICON\0I+VOLT\0I+PERC\0VOLT\0PERC\0TEMP\0";

pub static AUTO_POWER_OFF_OPTIONS: &str = "Off\015 Mins\030 Mins\01 Hour\0";

pub static POWER_PROFILE_OPTIONS: &str = "Power Save\0Balanced\0Performance\0";

#[inline(always)]
pub fn battery_options_and_types(nav_layout: u8) -> (&'static str, &'static [u8]) {
    if nav_layout == 0 {
        (BATTERY_K1_OPTIONS, &BATTERY_K1_TYPES)
    } else {
        (BATTERY_K5_OPTIONS, &BATTERY_K5_TYPES)
    }
}

pub static BATTERY_ROOT: [SettingDef; 5] = [
    SettingDef::battery_type("Capacity"),
    SettingDef::choice(
        "Status Bar",
        ChoiceId::BatteryFormat,
        SettingField::BATTERY_FORMAT,
    ),
    SettingDef::choice(
        "Deep Sleep",
        ChoiceId::AutoPowerOff,
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

pub static BATTERY_TWEAKS: [SettingDef; 3] = [
    SettingDef::range(
        "Calib Tweak",
        RangeId::CalibTweak,
        SettingField::CALIB_TWEAK_MV,
    ),
    SettingDef::toggle("Charge Detect", SettingField::CHARGE_DETECT),
    SettingDef::choice(
        "Power Profile",
        ChoiceId::PowerProfile,
        SettingField::POWER_PROFILE,
    ),
];
