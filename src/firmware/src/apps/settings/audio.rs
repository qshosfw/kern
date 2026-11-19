//! Audio Domain Settings Declarations & Hardware Binding

#[cfg(feature = "filesystem")]
use super::asset_chooser::{AssetDescriptor, AssetId};
use super::page::{ChoiceId, RangeId, SettingDef, SettingField, setting_page};

pub const KEY_BEEP_OPTIONS: &str = "Off\0Tone\0Click\0Double\0Warning\0Scroll\0";
pub const BOOT_JINGLE_OPTIONS: &str = "Off\0Chime\0Nokia\0Chirp\0GameBoy\0";
pub const AUDIO_PROFILE_OPTIONS: &str = "Flat\0Clean\0Mid\0Boost\0Max\0";

#[cfg(feature = "filesystem")]
pub static ROGER_ASSET: AssetDescriptor = AssetDescriptor {
    title: "Roger Beep",
    dir: "/user/rogers",
    extensions: ".ton\0.rgr\0",
    none_label: "Off",
    kind: AssetId::Roger,
};

#[cfg(feature = "filesystem")]
pub static JINGLE_ASSET: AssetDescriptor = AssetDescriptor {
    title: "Boot Jingle",
    dir: "/user/jingles",
    extensions: ".ton\0",
    none_label: "Off",
    kind: AssetId::Jingle,
};

setting_page!(pub AUDIO_PAGE = [
    ("AF Gain", SettingDef::range(RangeId::Range0_10, SettingField::VOLUME)),
    ("Beep Vol", SettingDef::range(RangeId::Range0_10, SettingField::BEEP_GAIN)),
    ("Key Beep", SettingDef::choice(ChoiceId::KeyBeep, SettingField::KEY_BEEP)),
    ("Mic Gain", SettingDef::range(RangeId::MicGain, SettingField::MIC_GAIN)),
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
