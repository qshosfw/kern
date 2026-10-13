//! Audio Domain Settings Declarations & Hardware Binding

use super::options::{
    ACTION_ROGER_CHOOSER, CHOICE_AUDIO_PROFILE, CHOICE_BOOT_JINGLE, CHOICE_KEY_BEEP, RANGE_0_10,
    RANGE_MIC_GAIN, SettingDef, SettingField, SettingKind,
};
use crate::SysContext;
use crate::services::melody::BeepType;

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
    SettingDef {
        title: "Roger Beep",
        kind: SettingKind::Action(ACTION_ROGER_CHOOSER),
        field: SettingField::None,
    },
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
    pub fn create_roger_chooser(ctx: &mut SysContext) -> gfx::gui::modules::menu::OptionSheet {
        crate::services::melody::provision_rogers(ctx.storage);
        let is_active = ctx.config.audio.roger_beep != 0;
        let mut cur_path = heapless::String::<32>::new();
        let _ = cur_path.push_str(ctx.config.audio.roger_path_str());

        super::asset_chooser::build_asset_chooser(
            "Roger Beep",
            "/user/rogers",
            |p| p.ends_with(".ton") || p.ends_with(".rgr"),
            |ctx, p| crate::services::melody::get_roger_name(ctx.storage, p),
            is_active,
            cur_path.as_str(),
            ctx,
        )
    }

    pub fn apply_roger_choice(sel: usize, ctx: &mut SysContext) -> bool {
        if sel == 0 {
            if ctx.config.audio.roger_beep != 0 {
                ctx.config.audio.roger_beep = 0;
                ctx.config.audio.set_roger_path("");
                ctx.play_beep(BeepType::Click);
                return true;
            }
            return false;
        }
        if let Some(path) = super::asset_chooser::get_chosen_asset_path(sel) {
            ctx.config.audio.roger_beep = 1;
            ctx.config.audio.set_roger_path(path);
            let notes = crate::services::melody::load_roger_melody(ctx.storage, path);
            if !notes.is_empty() {
                ctx.radio.play_tone_sequence(&notes);
            }
            true
        } else {
            false
        }
    }

    pub fn on_choice_confirmed(sel_idx: usize, sel: usize, ctx: &mut SysContext) -> bool {
        match sel_idx {
            2 => {
                ctx.config.audio.key_beep = sel as u8;
                ctx.config.mark_audio_dirty();
                ctx.play_beep(crate::services::melody::BeepType::from_u8(sel as u8));
                true
            }
            5 => {
                ctx.config.audio.boot_jingle = sel as u8;
                ctx.config.mark_audio_dirty();
                if sel != 0 {
                    let jingle = crate::services::melody::BootJingleType::from_u8(sel as u8);
                    let notes = crate::services::melody::MelodyEngine::boot_jingle_notes(jingle);
                    if !notes.is_empty() {
                        ctx.radio.play_tone_sequence(notes);
                    }
                } else {
                    ctx.play_beep(crate::services::melody::BeepType::Click);
                }
                true
            }
            _ => false,
        }
    }
}
