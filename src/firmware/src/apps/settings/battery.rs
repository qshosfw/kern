//! Battery Domain Settings & Submenu Declarations

use super::page::{ChoiceId, MetricId, PageId, SettingDef, SettingField, UNIT_MV, setting_page};

pub use ::abi::prop::options::{
    AUTO_POWER_OFF as AUTO_POWER_OFF_OPTIONS, BATTERY_FORMAT as BATTERY_FORMAT_OPTIONS,
    BATTERY_K1 as BATTERY_K1_OPTIONS, BATTERY_K5 as BATTERY_K5_OPTIONS,
    POWER_PROFILE as POWER_PROFILE_OPTIONS,
};
pub static BATTERY_K5_TYPES: [u8; 4] = [0, 1, 2, 5];
pub static BATTERY_K1_TYPES: [u8; 3] = [3, 4, 5];

#[inline(always)]
pub fn battery_options_and_types(nav_layout: u8) -> (&'static str, &'static [u8]) {
    if nav_layout == 0 {
        (BATTERY_K1_OPTIONS, &BATTERY_K1_TYPES)
    } else {
        (BATTERY_K5_OPTIONS, &BATTERY_K5_TYPES)
    }
}

setting_page!(pub BATTERY_ROOT_PAGE = [
    ("Capacity", SettingDef::battery_type()),
    ("Status Bar", SettingDef::choice(ChoiceId::BatteryFormat, SettingField::BATTERY_FORMAT)),
    ("Deep Sleep", SettingDef::choice(ChoiceId::AutoPowerOff, SettingField::AUTO_POWER_OFF)),
    ("Info", SettingDef::submenu(PageId::BatteryInfo)),
    ("Tweaks", SettingDef::submenu(PageId::BatteryTweaks)),
]);

setting_page!(pub BATTERY_INFO_PAGE = [
    ("Voltage", SettingDef::metric(MetricId::BatteryVoltage)),
    ("Battery", SettingDef::metric(MetricId::BatteryPercentage)),
    ("Status", SettingDef::metric(MetricId::BatteryStatus)),
    ("VDDA Ref", SettingDef::metric(MetricId::BatteryVdda)),
    ("Batt Ref", SettingDef::metric(MetricId::BatteryCalibRef)),
    ("Core Temp", SettingDef::metric(MetricId::BatteryCoreTemp)),
]);

setting_page!(pub BATTERY_TWEAKS_PAGE = [
    ("Calib Tweak", SettingDef::range(UNIT_MV, SettingField::CALIB_TWEAK_MV)),
    ("Charge Detect", SettingDef::toggle(SettingField::CHARGE_DETECT)),
    ("Power Profile", SettingDef::choice(ChoiceId::PowerProfile, SettingField::POWER_PROFILE)),
]);
