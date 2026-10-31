//! Display Domain Settings Declarations

use super::page::{RANGE_0_10, RANGE_1_10, RANGE_CONTRAST, SettingDef, SettingField};

pub static BACKLIGHT_TIME_OPTIONS: [&'static str; 6] =
    ["Off", "5s", "10s", "30s", "1m", "Always On"];

pub static BACKLIGHT_TR_OPTIONS: [&'static str; 4] = ["Off", "RX Only", "TX Only", "Both RX+TX"];

pub static SHADING_OPTIONS: [&'static str; 3] = ["Grayscale", "Dither", "Monochrome"];

pub static ANIMATION_OPTIONS: [&'static str; 2] = ["Off", "On"];

pub static DISPLAY_DEFS: &[SettingDef] = &[
    SettingDef::choice(
        "Backlight Time",
        &BACKLIGHT_TIME_OPTIONS,
        SettingField::BACKLIGHT_TIME,
    ),
    SettingDef::range("Max Brightness", &RANGE_1_10, SettingField::BACKLIGHT_MAX),
    SettingDef::range("Min Brightness", &RANGE_0_10, SettingField::BACKLIGHT_MIN),
    SettingDef::range("Contrast", &RANGE_CONTRAST, SettingField::CONTRAST),
    SettingDef::toggle("Invert Pixels", SettingField::INVERT_PIXELS),
    #[cfg(feature = "grayscale")]
    SettingDef::choice("Shading Mode", &SHADING_OPTIONS, SettingField::SHADING_MODE),
    #[cfg(feature = "animations")]
    SettingDef::choice(
        "Animations",
        &ANIMATION_OPTIONS,
        SettingField::UI_ANIMATIONS,
    ),
];
