//! Device Domain Settings Declarations

use super::options::{
    ACTION_CHOOSE_MODEL, ACTION_EDIT_OWNER, ACTION_EDIT_RADIO_ID, ACTION_EDIT_TAG,
    ACTION_SPLASH_CHOOSER, CHOICE_BOOT_ANIM, CHOICE_SPLASH_TIME, PAGE_BOOT_SPLASH, SettingDef,
    SettingField, SettingKind,
};
use crate::SysContext;
use gfx::gui::modules::menu::OptionSheet;

pub static MODEL_OPTIONS: [&'static str; 3] = ["UV-K1", "UV-K5 V3", "Custom..."];
pub static SPLASH_TIME_OPTIONS: [&'static str; 5] = ["Skip", "0.5s", "1.0s", "2.0s", "3.0s"];
pub static BOOT_ANIM_OPTIONS: [&'static str; 4] =
    ["None", "Dither Fill", "Invert Dissolve", "Curtain Wipe"];

pub static SPLASH_PAGE_DEFS: [SettingDef; 3] = [
    SettingDef {
        title: "Image",
        kind: SettingKind::Action(ACTION_SPLASH_CHOOSER),
        field: SettingField::None,
    },
    SettingDef {
        title: "Animation",
        kind: SettingKind::Choice(CHOICE_BOOT_ANIM),
        field: SettingField::BOOT_SPLASH_ANIM,
    },
    SettingDef {
        title: "Duration",
        kind: SettingKind::Choice(CHOICE_SPLASH_TIME),
        field: SettingField::BOOT_SPLASH_DURATION,
    },
];

pub static DEVICE_ROOT: [SettingDef; 5] = [
    SettingDef {
        title: "Model",
        kind: SettingKind::Action(ACTION_CHOOSE_MODEL),
        field: SettingField::None,
    },
    SettingDef {
        title: "Owner",
        kind: SettingKind::Action(ACTION_EDIT_OWNER),
        field: SettingField::None,
    },
    SettingDef {
        title: "Tag",
        kind: SettingKind::Action(ACTION_EDIT_TAG),
        field: SettingField::None,
    },
    SettingDef {
        title: "Radio ID",
        kind: SettingKind::Action(ACTION_EDIT_RADIO_ID),
        field: SettingField::None,
    },
    SettingDef {
        title: "Boot Splash",
        kind: SettingKind::Submenu(PAGE_BOOT_SPLASH),
        field: SettingField::None,
    },
];

pub struct DeviceDomain;

impl DeviceDomain {
    pub fn create_splash_chooser(ctx: &mut SysContext) -> OptionSheet {
        crate::boot::splash::provision_splashes(ctx.storage);
        super::asset_chooser::build_splash_chooser(ctx)
    }

    pub fn apply_splash_choice(sel: usize, ctx: &mut SysContext) -> bool {
        if sel == 0 {
            if ctx.config.display.boot_splash_mode != 0 {
                ctx.config.display.boot_splash_mode = 0;
                ctx.config.display.set_splash_path("");
                return true;
            }
            return false;
        }
        if let Some(path) = super::asset_chooser::get_chosen_asset_path(sel) {
            ctx.config.display.boot_splash_mode = 1;
            ctx.config.display.set_splash_path(path);
            true
        } else {
            false
        }
    }
}
