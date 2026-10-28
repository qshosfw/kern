//! Audio Domain Settings Declarations & Hardware Binding

#[cfg(feature = "boot_assets")]
use super::options::{ACTION_JINGLE_CHOOSER, ACTION_ROGER_CHOOSER};
use super::options::{
    CHOICE_AUDIO_PROFILE, CHOICE_KEY_BEEP, RANGE_0_10, RANGE_MIC_GAIN, SettingDef, SettingField,
    SettingKind,
};
#[cfg(not(feature = "boot_assets"))]
use super::options::{CHOICE_BOOT_JINGLE, CHOICE_OFF_ON};
use crate::SysContext;
#[cfg(feature = "boot_assets")]
use gfx::gui::modules::menu::OptionSheet;

pub static KEY_BEEP_OPTIONS: [&'static str; 6] =
    ["Off", "Tone", "Click", "Double", "Warning", "Scroll"];

pub static BOOT_JINGLE_OPTIONS: [&'static str; 5] = ["Off", "Chime", "Nokia", "Chirp", "GameBoy"];

pub static AUDIO_PROFILE_OPTIONS: [&'static str; 5] = ["Flat", "Clean", "Mid", "Boost", "Max"];

pub struct AudioDomain;

pub static AUDIO_DEFS: [SettingDef; 7] = [
    SettingDef {
        title: "AF Gain",
        kind: SettingKind::Range(RANGE_0_10),
        field: SettingField::VOLUME,
    },
    SettingDef {
        title: "Beep Vol",
        kind: SettingKind::Range(RANGE_0_10),
        field: SettingField::BEEP_GAIN,
    },
    SettingDef {
        title: "Key Beep",
        kind: SettingKind::Choice(CHOICE_KEY_BEEP),
        field: SettingField::KEY_BEEP,
    },
    SettingDef {
        title: "Mic Gain",
        kind: SettingKind::Range(RANGE_MIC_GAIN),
        field: SettingField::MIC_GAIN,
    },
    #[cfg(feature = "boot_assets")]
    SettingDef {
        title: "Roger Beep",
        kind: SettingKind::Action(ACTION_ROGER_CHOOSER),
        field: SettingField::None,
    },
    #[cfg(not(feature = "boot_assets"))]
    SettingDef {
        title: "Roger Beep",
        kind: SettingKind::Choice(CHOICE_OFF_ON),
        field: SettingField::ROGER_BEEP,
    },
    #[cfg(feature = "boot_assets")]
    SettingDef {
        title: "Boot Jingle",
        kind: SettingKind::Action(ACTION_JINGLE_CHOOSER),
        field: SettingField::None,
    },
    #[cfg(not(feature = "boot_assets"))]
    SettingDef {
        title: "Boot Jingle",
        kind: SettingKind::Choice(CHOICE_BOOT_JINGLE),
        field: SettingField::BOOT_JINGLE,
    },
    SettingDef {
        title: "RX Profile",
        kind: SettingKind::Choice(CHOICE_AUDIO_PROFILE),
        field: SettingField::AUDIO_PROFILE,
    },
];

impl AudioDomain {
    #[cfg(feature = "boot_assets")]
    pub fn create_roger_chooser(ctx: &mut SysContext) -> OptionSheet {
        let params = super::asset_chooser::AssetChooserParams {
            title: "Roger Beep",
            dir: "/user/rogers",
            cur_path: heapless::String::try_from(ctx.config.audio.roger_path_str())
                .unwrap_or_default(),
            is_active: ctx.config.audio.roger_beep != 0,
            none_label: Some("Off"),
            fallback: None,
            extensions: &[".ton", ".rgr"],
        };
        super::asset_chooser::build_asset_chooser(&params, ctx)
    }

    #[cfg(feature = "boot_assets")]
    pub fn apply_roger_choice(sel: usize, ctx: &mut SysContext) -> bool {
        if sel == 0 {
            if ctx.config.audio.roger_beep != 0 {
                ctx.config.audio.roger_beep = 0;
                ctx.config.audio.set_roger_path("");
                ctx.play_beep(crate::services::melody::BeepType::Click);
                return true;
            }
            return false;
        }
        if let Some(path) = super::asset_chooser::get_chosen_asset_path(sel) {
            ctx.config.audio.roger_beep = 1;
            ctx.config.audio.set_roger_path(path);
            let mel = crate::services::melody::load_roger_melody(ctx.storage, path);
            if !mel.is_empty() {
                ctx.radio.play_tone_sequence(mel.as_slice());
            }
            true
        } else {
            false
        }
    }

    #[cfg(feature = "boot_assets")]
    pub fn create_jingle_chooser(ctx: &mut SysContext) -> OptionSheet {
        let params = super::asset_chooser::AssetChooserParams {
            title: "Boot Jingle",
            dir: "/user/jingles",
            cur_path: heapless::String::try_from(ctx.config.audio.jingle_path_str())
                .unwrap_or_default(),
            is_active: ctx.config.audio.boot_jingle != 0,
            none_label: Some("Off"),
            fallback: None,
            extensions: &[".ton"],
        };
        super::asset_chooser::build_asset_chooser(&params, ctx)
    }

    #[cfg(feature = "boot_assets")]
    pub fn apply_jingle_choice(sel: usize, ctx: &mut SysContext) -> bool {
        if sel == 0 {
            if ctx.config.audio.boot_jingle != 0 {
                ctx.config.audio.boot_jingle = 0;
                ctx.config.audio.set_jingle_path("");
                ctx.play_beep(crate::services::melody::BeepType::Click);
                return true;
            }
            return false;
        }
        if let Some(path) = super::asset_chooser::get_chosen_asset_path(sel) {
            ctx.config.audio.boot_jingle = 1;
            ctx.config.audio.set_jingle_path(path);
            let mel = crate::services::melody::load_jingle_melody(ctx.storage, 1, path);
            if !mel.is_empty() {
                ctx.radio.play_tone_sequence(mel.as_slice());
            }
            true
        } else {
            false
        }
    }

    pub fn on_choice_confirmed(sel_idx: usize, sel: usize, ctx: &mut SysContext) {
        match sel_idx {
            2 => {
                ctx.play_beep(crate::services::melody::BeepType::from_u8(sel as u8));
            }
            5 => {
                if sel != 0 {
                    let jingle = crate::services::melody::BootJingleType::from_u8(sel as u8);
                    let notes = crate::services::melody::MelodyEngine::boot_jingle_notes(jingle);
                    if !notes.is_empty() {
                        ctx.radio.play_tone_sequence(notes);
                    }
                } else {
                    ctx.play_beep(crate::services::melody::BeepType::Click);
                }
            }
            _ => {}
        }
    }
}
