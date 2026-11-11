//! Device Domain Settings Declarations

#[cfg(feature = "filesystem")]
use super::asset_chooser::AssetDescriptor;
#[cfg(feature = "filesystem")]
use super::page::AssetId;
#[cfg(not(feature = "text_input"))]
use super::page::MetricId;
use super::page::{ActionId, ChoiceId, PageId, SettingDef, SettingField};

#[cfg(feature = "text_input")]
pub static MODEL_OPTIONS: [&'static str; 3] = ["UV-K1", "UV-K5 V3", "Custom..."];
#[cfg(not(feature = "text_input"))]
pub static MODEL_OPTIONS: [&'static str; 2] = ["UV-K1", "UV-K5 V3"];

pub static SPLASH_TIME_OPTIONS: &str = "Skip\00.5s\01.0s\02.0s\03.0s\0";
pub static BOOT_ANIM_OPTIONS: &str = "None\0Dither Fill\0Invert Dissolve\0Curtain Wipe\0";

#[cfg(feature = "filesystem")]
pub static SPLASH_ASSET: AssetDescriptor = AssetDescriptor {
    title: "Boot Splash",
    dir: "/user/splash",
    extensions: &[".rle", ".img", ".bin"],
    none_label: "None",
    get_path: |cfg| {
        (
            cfg.display.boot_splash_mode != 0,
            cfg.display.splash_path_str(),
        )
    },
    set_path: |cfg, opt| match opt {
        Some(path) => {
            cfg.display.boot_splash_mode = 1;
            cfg.display.set_splash_path(path);
        }
        None => cfg.display.boot_splash_mode = 0,
    },
};

#[cfg(feature = "filesystem")]
pub static SPLASH_PAGE_DEFS: &[SettingDef] = &[
    SettingDef::asset("Image", AssetId::Splash),
    SettingDef::choice(
        "Animation",
        ChoiceId::SplashAnim,
        SettingField::BOOT_SPLASH_ANIM,
    ),
    SettingDef::choice(
        "Duration",
        ChoiceId::SplashDuration,
        SettingField::BOOT_SPLASH_DURATION,
    ),
];

#[cfg(not(feature = "filesystem"))]
pub static SPLASH_PAGE_DEFS: &[SettingDef] = &[
    SettingDef::choice(
        "Animation",
        ChoiceId::SplashAnim,
        SettingField::BOOT_SPLASH_ANIM,
    ),
    SettingDef::choice(
        "Duration",
        ChoiceId::SplashDuration,
        SettingField::BOOT_SPLASH_DURATION,
    ),
];

#[cfg(feature = "text_input")]
pub static DEVICE_ROOT: [SettingDef; 5] = [
    SettingDef::action("Model", ActionId::ChooseModel),
    SettingDef::action("Owner", ActionId::EditOwner),
    SettingDef::action("Tag", ActionId::EditTag),
    SettingDef::action("Radio ID", ActionId::EditRadioId),
    SettingDef::submenu("Boot Splash", PageId::BootSplash),
];

#[cfg(not(feature = "text_input"))]
pub static DEVICE_ROOT: [SettingDef; 5] = [
    SettingDef::action("Model", ActionId::ChooseModel),
    SettingDef::metric("Owner", MetricId::DeviceOwner),
    SettingDef::metric("Tag", MetricId::DeviceTag),
    SettingDef::metric("Radio ID", MetricId::DeviceRadioId),
    SettingDef::submenu("Boot Splash", PageId::BootSplash),
];
