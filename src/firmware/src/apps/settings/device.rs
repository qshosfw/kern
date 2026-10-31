//! Device Domain Settings Declarations

#[cfg(feature = "boot_assets")]
use super::asset_chooser::AssetDescriptor;
use super::page::{ActionId, MetricId, PageId, SettingDef, SettingField};

#[cfg(feature = "passcode")]
pub static MODEL_OPTIONS: [&'static str; 3] = ["UV-K1", "UV-K5 V3", "Custom..."];
#[cfg(not(feature = "passcode"))]
pub static MODEL_OPTIONS: [&'static str; 2] = ["UV-K1", "UV-K5 V3"];

pub static SPLASH_TIME_OPTIONS: [&'static str; 5] = ["Skip", "0.5s", "1.0s", "2.0s", "3.0s"];
pub static BOOT_ANIM_OPTIONS: [&'static str; 4] =
    ["None", "Dither Fill", "Invert Dissolve", "Curtain Wipe"];

#[cfg(feature = "boot_assets")]
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

#[cfg(feature = "boot_assets")]
pub static SPLASH_PAGE_DEFS: &[SettingDef] = &[
    SettingDef::asset("Image", &SPLASH_ASSET),
    SettingDef::choice(
        "Animation",
        &BOOT_ANIM_OPTIONS,
        SettingField::BOOT_SPLASH_ANIM,
    ),
    SettingDef::choice(
        "Duration",
        &SPLASH_TIME_OPTIONS,
        SettingField::BOOT_SPLASH_DURATION,
    ),
];

#[cfg(not(feature = "boot_assets"))]
pub static SPLASH_PAGE_DEFS: &[SettingDef] = &[
    SettingDef::choice(
        "Animation",
        &BOOT_ANIM_OPTIONS,
        SettingField::BOOT_SPLASH_ANIM,
    ),
    SettingDef::choice(
        "Duration",
        &SPLASH_TIME_OPTIONS,
        SettingField::BOOT_SPLASH_DURATION,
    ),
];

#[cfg(feature = "passcode")]
pub static DEVICE_ROOT: [SettingDef; 5] = [
    SettingDef::action("Model", ActionId::ChooseModel),
    SettingDef::action("Owner", ActionId::EditOwner),
    SettingDef::action("Tag", ActionId::EditTag),
    SettingDef::action("Radio ID", ActionId::EditRadioId),
    SettingDef::submenu("Boot Splash", PageId::BootSplash),
];

#[cfg(not(feature = "passcode"))]
pub static DEVICE_ROOT: [SettingDef; 5] = [
    SettingDef::action("Model", ActionId::ChooseModel),
    SettingDef::metric("Owner", MetricId::DeviceOwner),
    SettingDef::metric("Tag", MetricId::DeviceTag),
    SettingDef::metric("Radio ID", MetricId::DeviceRadioId),
    SettingDef::submenu("Boot Splash", PageId::BootSplash),
];
