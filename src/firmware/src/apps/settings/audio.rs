//! Audio Domain Settings Declarations & Hardware Binding

#[cfg(feature = "filesystem")]
use super::asset_chooser::AssetId;
use super::page::{ChoiceId, SettingDef, SettingField, UNIT_DB, UNIT_NONE, setting_page};

pub const KEY_BEEP_OPTIONS: &str = "Off\0Tone\0Click\0Double\0Warning\0Scroll\0";
pub const BOOT_JINGLE_OPTIONS: &str = "Off\0Chime\0Nokia\0Chirp\0GameBoy\0";
pub const AUDIO_PROFILE_OPTIONS: &str = "Flat\0Clean\0Mid\0Boost\0Max\0";

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
