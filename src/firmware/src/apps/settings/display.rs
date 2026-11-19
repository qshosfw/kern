//! Display Domain Settings Declarations

use super::page::{ChoiceId, RangeId, SettingDef, SettingField, setting_page};

pub const BACKLIGHT_TIME_OPTIONS: &str = "Off\05s\010s\030s\01m\0Always On\0";
pub const SHADING_OPTIONS: &str = "Grayscale\0Dither\0Monochrome\0";
pub const ANIMATION_OPTIONS: &str = "Off\0On\0";

setting_page!(pub DISPLAY_PAGE = [
    ("Backlight Time", SettingDef::choice(ChoiceId::BacklightTime, SettingField::BACKLIGHT_TIME)),
    ("Max Brightness", SettingDef::range(RangeId::Range1_10, SettingField::BACKLIGHT_MAX)),
    ("Min Brightness", SettingDef::range(RangeId::Range0_10, SettingField::BACKLIGHT_MIN)),
    ("Contrast", SettingDef::range(RangeId::Contrast, SettingField::CONTRAST)),
    ("Invert Pixels", SettingDef::toggle(SettingField::INVERT_PIXELS)),
    #[cfg(feature = "grayscale")]
    ("Shading Mode", SettingDef::choice(ChoiceId::Shading, SettingField::SHADING_MODE)),
    #[cfg(feature = "animations")]
    ("Animations", SettingDef::choice(ChoiceId::Animation, SettingField::UI_ANIMATIONS)),
]);
