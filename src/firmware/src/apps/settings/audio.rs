//! Audio Domain Settings Declarations & Hardware Binding

#[cfg(feature = "boot_assets")]
use super::asset_chooser::AssetDescriptor;
#[cfg(feature = "boot_assets")]
use super::page::AssetId;
use super::page::{ChoiceId, RangeId, SettingDef, SettingField};

pub static KEY_BEEP_OPTIONS: &str = "Off\0Tone\0Click\0Double\0Warning\0Scroll\0";

pub static BOOT_JINGLE_OPTIONS: &str = "Off\0Chime\0Nokia\0Chirp\0GameBoy\0";

pub static AUDIO_PROFILE_OPTIONS: &str = "Flat\0Clean\0Mid\0Boost\0Max\0";

#[cfg(feature = "boot_assets")]
pub static ROGER_ASSET: AssetDescriptor = AssetDescriptor {
    title: "Roger Beep",
    dir: "/user/rogers",
    extensions: &[".ton", ".rgr"],
    none_label: "Off",
    get_path: |cfg| (cfg.audio.roger_beep != 0, cfg.audio.roger_path_str()),
    set_path: |cfg, opt| match opt {
        Some(path) => {
            cfg.audio.roger_beep = 1;
            cfg.audio.set_roger_path(path);
        }
        None => cfg.audio.roger_beep = 0,
    },
};

#[cfg(feature = "boot_assets")]
pub static JINGLE_ASSET: AssetDescriptor = AssetDescriptor {
    title: "Boot Jingle",
    dir: "/user/jingles",
    extensions: &[".ton"],
    none_label: "Off",
    get_path: |cfg| (cfg.audio.boot_jingle != 0, cfg.audio.jingle_path_str()),
    set_path: |cfg, opt| match opt {
        Some(path) => {
            cfg.audio.boot_jingle = 1;
            cfg.audio.set_jingle_path(path);
        }
        None => cfg.audio.boot_jingle = 0,
    },
};

pub static AUDIO_DEFS: [SettingDef; 7] = [
    SettingDef::range("AF Gain", RangeId::Range0_10, SettingField::VOLUME),
    SettingDef::range("Beep Vol", RangeId::Range0_10, SettingField::BEEP_GAIN),
    SettingDef::choice("Key Beep", ChoiceId::KeyBeep, SettingField::KEY_BEEP),
    SettingDef::range("Mic Gain", RangeId::MicGain, SettingField::MIC_GAIN),
    #[cfg(feature = "boot_assets")]
    SettingDef::asset("Roger Beep", AssetId::Roger),
    #[cfg(not(feature = "boot_assets"))]
    SettingDef::toggle("Roger Beep", SettingField::ROGER_BEEP),
    #[cfg(feature = "boot_assets")]
    SettingDef::asset("Boot Jingle", AssetId::Jingle),
    #[cfg(not(feature = "boot_assets"))]
    SettingDef::choice(
        "Boot Jingle",
        ChoiceId::BootJingle,
        SettingField::BOOT_JINGLE,
    ),
    SettingDef::choice(
        "RX Profile",
        ChoiceId::AudioProfile,
        SettingField::AUDIO_PROFILE,
    ),
];
