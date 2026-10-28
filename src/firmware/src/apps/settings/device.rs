//! Device Domain Settings Declarations

#[cfg(feature = "boot_assets")]
use super::options::ACTION_SPLASH_CHOOSER;
use super::options::{
    ACTION_CHOOSE_MODEL, CHOICE_BOOT_ANIM, CHOICE_SPLASH_TIME, PAGE_BOOT_SPLASH, SettingDef,
    SettingField, SettingKind,
};
#[cfg(feature = "passcode")]
use super::options::{ACTION_EDIT_OWNER, ACTION_EDIT_RADIO_ID, ACTION_EDIT_TAG};
#[cfg(not(feature = "passcode"))]
use super::options::{METRIC_DEVICE_ASSET, METRIC_DEVICE_OWNER, METRIC_DEVICE_RADIO_ID};
#[cfg(feature = "boot_assets")]
use crate::SysContext;
#[cfg(feature = "boot_assets")]
use gfx::gui::modules::menu::OptionSheet;

#[cfg(feature = "passcode")]
pub static MODEL_OPTIONS: [&'static str; 3] = ["UV-K1", "UV-K5 V3", "Custom..."];
#[cfg(not(feature = "passcode"))]
pub static MODEL_OPTIONS: [&'static str; 2] = ["UV-K1", "UV-K5 V3"];

pub static SPLASH_TIME_OPTIONS: [&'static str; 5] = ["Skip", "0.5s", "1.0s", "2.0s", "3.0s"];
pub static BOOT_ANIM_OPTIONS: [&'static str; 4] =
    ["None", "Dither Fill", "Invert Dissolve", "Curtain Wipe"];

#[cfg(feature = "boot_assets")]
pub static SPLASH_PAGE_DEFS: &[SettingDef] = &[
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

#[cfg(not(feature = "boot_assets"))]
pub static SPLASH_PAGE_DEFS: &[SettingDef] = &[
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

#[cfg(feature = "passcode")]
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

#[cfg(not(feature = "passcode"))]
pub static DEVICE_ROOT: [SettingDef; 5] = [
    SettingDef {
        title: "Model",
        kind: SettingKind::Action(ACTION_CHOOSE_MODEL),
        field: SettingField::None,
    },
    SettingDef {
        title: "Owner",
        kind: SettingKind::Live(METRIC_DEVICE_OWNER),
        field: SettingField::None,
    },
    SettingDef {
        title: "Tag",
        kind: SettingKind::Live(METRIC_DEVICE_ASSET),
        field: SettingField::None,
    },
    SettingDef {
        title: "Radio ID",
        kind: SettingKind::Live(METRIC_DEVICE_RADIO_ID),
        field: SettingField::None,
    },
    SettingDef {
        title: "Boot Splash",
        kind: SettingKind::Submenu(PAGE_BOOT_SPLASH),
        field: SettingField::None,
    },
];

#[cfg(feature = "boot_assets")]
pub struct DeviceDomain;

#[cfg(feature = "boot_assets")]
impl DeviceDomain {
    pub fn create_splash_chooser(ctx: &mut SysContext) -> OptionSheet {
        let params = super::asset_chooser::AssetChooserParams {
            title: "Boot Splash",
            dir: "/user/splash",
            cur_path: heapless::String::try_from(ctx.config.display.splash_path_str())
                .unwrap_or_default(),
            is_active: ctx.config.display.boot_splash_mode != 0,
            none_label: Some("None"),
            fallback: None,
            extensions: &[".rle", ".img", ".bin"],
        };
        super::asset_chooser::build_asset_chooser(&params, ctx)
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
