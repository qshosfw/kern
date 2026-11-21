//! Device Domain Settings Declarations

#[cfg(feature = "filesystem")]
use super::asset_chooser::AssetId;
#[cfg(not(feature = "text_input"))]
use super::page::MetricId;
use super::page::{ActionId, ChoiceId, PageId, SettingDef, SettingField, setting_page};

#[cfg(feature = "text_input")]
pub const MODEL_OPTIONS: &str = "UV-K1\0UV-K5 V3\0Custom...\0";
#[cfg(not(feature = "text_input"))]
pub const MODEL_OPTIONS: &str = "UV-K1\0UV-K5 V3\0";

pub use ::abi::prop::options::{BOOT_ANIM as BOOT_ANIM_OPTIONS, SPLASH_TIME as SPLASH_TIME_OPTIONS};

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
