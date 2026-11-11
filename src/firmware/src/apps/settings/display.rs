//! Display Domain Settings Declarations

use super::page::{ChoiceId, RangeId, SettingDef, SettingField};

pub static BACKLIGHT_TIME_OPTIONS: &str = "Off\05s\010s\030s\01m\0Always On\0";

pub static SHADING_OPTIONS: &str = "Grayscale\0Dither\0Monochrome\0";

pub static ANIMATION_OPTIONS: &str = "Off\0On\0";

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
