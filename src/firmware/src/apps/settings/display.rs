//! Display Domain Settings Declarations

#[cfg(feature = "grayscale")]
use super::options::CHOICE_SHADING_MODE;
#[cfg(feature = "animations")]
use super::options::CHOICE_UI_ANIMATIONS;
use super::options::{
    CHOICE_BACKLIGHT_TIME, RANGE_0_10, RANGE_BACKLIGHT_MAX, RANGE_CONTRAST, SettingDef,
    SettingField, SettingKind,
};

pub static BACKLIGHT_TIME_OPTIONS: [&'static str; 6] =
    ["Off", "5s", "10s", "30s", "1m", "Always On"];

pub static BACKLIGHT_TR_OPTIONS: [&'static str; 4] = ["Off", "RX Only", "TX Only", "Both RX+TX"];

pub static SHADING_OPTIONS: [&'static str; 3] = ["Grayscale", "Dither", "Monochrome"];

pub static ANIMATION_OPTIONS: [&'static str; 2] = ["Off", "On"];

pub static DISPLAY_DEFS: &[SettingDef] = &[
    SettingDef {
        title: "Backlight Time",
        kind: SettingKind::Choice(CHOICE_BACKLIGHT_TIME),
        field: SettingField::BACKLIGHT_TIME,
    },
    SettingDef {
        title: "Max Brightness",
        kind: SettingKind::Range(RANGE_BACKLIGHT_MAX),
        field: SettingField::BACKLIGHT_MAX,
    },
    SettingDef {
        title: "Min Brightness",
        kind: SettingKind::Range(RANGE_0_10),
        field: SettingField::BACKLIGHT_MIN,
    },
    SettingDef {
        title: "Contrast",
        kind: SettingKind::Range(RANGE_CONTRAST),
        field: SettingField::CONTRAST,
    },
    SettingDef {
        title: "Invert Pixels",
        kind: SettingKind::Toggle,
        field: SettingField::INVERT_PIXELS,
    },
    #[cfg(feature = "grayscale")]
    SettingDef {
        title: "Shading Mode",
        kind: SettingKind::Choice(CHOICE_SHADING_MODE),
        field: SettingField::SHADING_MODE,
    },
    #[cfg(feature = "animations")]
    SettingDef {
        title: "Animations",
        kind: SettingKind::Choice(CHOICE_UI_ANIMATIONS),
        field: SettingField::UI_ANIMATIONS,
    },
];
