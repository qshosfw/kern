//! Display Domain Settings Declarations

use super::page::{ChoiceId, SettingDef, SettingField, UNIT_NONE, UNIT_PERCENT, setting_page};

pub use ::abi::prop::options::{
    ANIMATION as ANIMATION_OPTIONS, BACKLIGHT_TIME as BACKLIGHT_TIME_OPTIONS,
    SHADING as SHADING_OPTIONS,
};

setting_page!(pub DISPLAY_PAGE = [
    ("Backlight Time", SettingDef::choice(ChoiceId::BacklightTime, SettingField::BACKLIGHT_TIME)),
    ("Max Brightness", SettingDef::range(UNIT_NONE, SettingField::BACKLIGHT_MAX)),
    ("Min Brightness", SettingDef::range(UNIT_NONE, SettingField::BACKLIGHT_MIN)),
    ("Contrast", SettingDef::range(UNIT_PERCENT, SettingField::CONTRAST)),
    ("Invert Pixels", SettingDef::toggle(SettingField::INVERT_PIXELS)),
    #[cfg(feature = "grayscale")]
    ("Shading Mode", SettingDef::choice(ChoiceId::Shading, SettingField::SHADING_MODE)),
    #[cfg(feature = "animations")]
    ("Animations", SettingDef::choice(ChoiceId::Animation, SettingField::UI_ANIMATIONS)),
]);
