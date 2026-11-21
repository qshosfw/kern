//! Audio Domain Settings Declarations & Hardware Binding

#[cfg(feature = "filesystem")]
use super::asset_chooser::AssetId;
use super::page::{ChoiceId, SettingDef, SettingField, UNIT_DB, UNIT_NONE, setting_page};

pub use ::abi::prop::options::{
    AUDIO_PROFILE as AUDIO_PROFILE_OPTIONS, BOOT_JINGLE as BOOT_JINGLE_OPTIONS,
    KEY_BEEP as KEY_BEEP_OPTIONS,
};

setting_page!(pub AUDIO_PAGE = [
    ("AF Gain", SettingDef::range(UNIT_NONE, SettingField::VOLUME)),
    ("Beep Vol", SettingDef::range(UNIT_NONE, SettingField::BEEP_GAIN)),
    ("Key Beep", SettingDef::choice(ChoiceId::KeyBeep, SettingField::KEY_BEEP)),
    ("Mic Gain", SettingDef::range(UNIT_DB, SettingField::MIC_GAIN)),
    #[cfg(feature = "filesystem")]
    ("Roger Beep", SettingDef::asset(AssetId::Roger)),
    #[cfg(not(feature = "filesystem"))]
    ("Roger Beep", SettingDef::toggle(SettingField::ROGER_BEEP)),
    #[cfg(feature = "filesystem")]
    ("Boot Jingle", SettingDef::asset(AssetId::Jingle)),
    #[cfg(not(feature = "filesystem"))]
    ("Boot Jingle", SettingDef::choice(ChoiceId::BootJingle, SettingField::BOOT_JINGLE)),
    ("RX Profile", SettingDef::choice(ChoiceId::AudioProfile, SettingField::AUDIO_PROFILE)),
]);
