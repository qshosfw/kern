//! Display Domain Settings Declarations

use super::page::{ChoiceId, RangeId, SettingDef, SettingField};

pub static BACKLIGHT_TIME_OPTIONS: [&'static str; 6] =
    ["Off", "5s", "10s", "30s", "1m", "Always On"];

pub static SHADING_OPTIONS: [&'static str; 3] = ["Grayscale", "Dither", "Monochrome"];

pub static ANIMATION_OPTIONS: [&'static str; 2] = ["Off", "On"];

pub static DISPLAY_DEFS: &[SettingDef] = &[
    SettingDef::choice(
        "Backlight Time",
        ChoiceId::BacklightTime,
        SettingField::BACKLIGHT_TIME,
    ),
    SettingDef::range(
        "Max Brightness",
        RangeId::Range1_10,
        SettingField::BACKLIGHT_MAX,
    ),
    SettingDef::range(
        "Min Brightness",
        RangeId::Range0_10,
        SettingField::BACKLIGHT_MIN,
    ),
    SettingDef::range("Contrast", RangeId::Contrast, SettingField::CONTRAST),
    SettingDef::toggle("Invert Pixels", SettingField::INVERT_PIXELS),
    #[cfg(feature = "grayscale")]
    SettingDef::choice(
        "Shading Mode",
        ChoiceId::Shading,
        SettingField::SHADING_MODE,
    ),
    #[cfg(feature = "animations")]
    SettingDef::choice(
        "Animations",
        ChoiceId::Animation,
        SettingField::UI_ANIMATIONS,
    ),
];
