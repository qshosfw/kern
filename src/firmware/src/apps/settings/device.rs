//! Device Domain Settings Declarations

#[cfg(feature = "filesystem")]
use super::asset_chooser::{AssetDescriptor, AssetId};
#[cfg(not(feature = "text_input"))]
use super::page::MetricId;
use super::page::{ActionId, ChoiceId, PageId, SettingDef, SettingField, setting_page};

#[cfg(feature = "text_input")]
pub static MODEL_OPTIONS: &str = "UV-K1\0UV-K5 V3\0Custom...\0";
#[cfg(not(feature = "text_input"))]
pub static MODEL_OPTIONS: &str = "UV-K1\0UV-K5 V3\0";

pub const SPLASH_TIME_OPTIONS: &str = "Skip\00.5s\01.0s\02.0s\03.0s\0";
pub const BOOT_ANIM_OPTIONS: &str = "None\0Dither Fill\0Invert Dissolve\0Curtain Wipe\0";

#[cfg(feature = "filesystem")]
pub static SPLASH_ASSET: AssetDescriptor = AssetDescriptor {
    title: "Boot Splash",
    dir: "/user/splash",
    extensions: ".rle\0.img\0.bin\0",
    none_label: "None",
    kind: AssetId::Splash,
};

setting_page!(pub SPLASH_PAGE = [
    #[cfg(feature = "filesystem")]
    ("Image", SettingDef::asset(AssetId::Splash)),
    ("Animation", SettingDef::choice(ChoiceId::SplashAnim, SettingField::BOOT_SPLASH_ANIM)),
    ("Duration", SettingDef::choice(ChoiceId::SplashDuration, SettingField::BOOT_SPLASH_DURATION)),
]);

setting_page!(pub DEVICE_ROOT_PAGE = [
    ("Model", SettingDef::action(ActionId::ChooseModel)),
    #[cfg(feature = "text_input")]
    ("Owner", SettingDef::action(ActionId::EditOwner)),
    #[cfg(not(feature = "text_input"))]
    ("Owner", SettingDef::metric(MetricId::DeviceOwner)),
    #[cfg(feature = "text_input")]
    ("Tag", SettingDef::action(ActionId::EditTag)),
    #[cfg(not(feature = "text_input"))]
    ("Tag", SettingDef::metric(MetricId::DeviceTag)),
    #[cfg(feature = "text_input")]
    ("Radio ID", SettingDef::action(ActionId::EditRadioId)),
    #[cfg(not(feature = "text_input"))]
    ("Radio ID", SettingDef::metric(MetricId::DeviceRadioId)),
    ("Boot Splash", SettingDef::submenu(PageId::BootSplash)),
]);
