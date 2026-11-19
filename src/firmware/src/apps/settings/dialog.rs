//! Modal Dialogs, Input Prompts & Action Dispatch for OS Settings

use super::app::SettingsApp;
use super::page::ActionId;
use crate::SysContext;
use crate::services::power::PowerServiceConfigExt;
use gfx::gui::modules::menu::OptionSheet;
use gfx::gui::modules::menu::ShortString;
use kernel::app::EventResult;

#[cfg(feature = "passcode")]
pub const TAG_PASSCODE_VERIFY: u8 = 2;
#[cfg(feature = "passcode")]
pub const TAG_PASSCODE_NEW: u8 = 3;
#[cfg(feature = "passcode")]
pub const TAG_PASSCODE_CONFIRM: u8 = 4;
pub const TAG_CHOICE_SHEET: u8 = 6;
pub const TAG_POPUP_INFO: u8 = 7;
pub const TAG_OWNER_INPUT: u8 = 8;
pub const TAG_TAG_INPUT: u8 = 9;
pub const TAG_RADIO_ID_INPUT: u8 = 10;
#[cfg(feature = "filesystem")]
pub const TAG_ASSET_CHOOSER: u8 = 11;
pub const TAG_MODEL_CHOOSER: u8 = 12;
pub const TAG_MODEL_INPUT: u8 = 13;
#[cfg(feature = "filesystem")]
pub const TAG_FORMAT_KEEP_CALIB: u8 = 16;
#[cfg(feature = "filesystem")]
pub const TAG_FORMAT_WIPE_ALL: u8 = 17;
pub const TAG_RESET_SETTINGS: u8 = 20;
pub const TAG_REBOOT_FW: u8 = 21;
pub const TAG_SHORTCUT_SHEET: u8 = 23;

pub fn parse_u32_digits(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut val = 0u32;
    for &b in s.as_bytes() {
        if !b.is_ascii_digit() || val > u32::MAX / 10 {
            return None;
        }
        val = (val * 10).checked_add((b - b'0') as u32)?;
    }
    Some(val)
}

pub fn copy_padded(dst: &mut [u8], src: &[u8]) {
    dst.fill(b' ');
    let len = src.len().min(dst.len());
    dst[..len].copy_from_slice(&src[..len]);
}

#[inline(never)]
pub fn prompt_config_str(ctx: &mut SysContext, tag: u8) {
    let mut buf = ShortString::empty();
    let (title, initial, max_len, digits) = if tag == TAG_RADIO_ID_INPUT {
        buf.push_int_fixed(ctx.config.system.radio_id as i32, 0, "", "");
        ("Radio ID", buf.as_str(), 8, true)
    } else {
        let (title, s, max_len) = match tag {
            TAG_OWNER_INPUT => ("Owner", ctx.config.system.owner_str(), 12),
            TAG_TAG_INPUT => ("Tag", ctx.config.system.tag_str(), 8),
            _ => ("Model", ctx.config.system.custom_model_str(), 12),
        };
        (title, s, max_len, false)
    };
    ctx.prompt_text(tag, title, initial, max_len, digits);
}

#[cfg(feature = "passcode")]
#[inline(never)]
pub fn handle_security_press(ctx: &mut SysContext) {
    let is_set = crate::security::passcode::is_passcode_set(ctx.storage);
    let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
    if is_set {
        let max_len = if pcfg.passcode_len > 0 {
            pcfg.passcode_len as usize
        } else {
            16
        };
        let expose_len = pcfg.passcode_len > 0;
        ctx.prompt_passcode(
            TAG_PASSCODE_VERIFY,
            "Verify Passcode",
            max_len,
            pcfg.digits_only,
            expose_len,
        );
    } else {
        ctx.prompt_passcode(
            TAG_PASSCODE_NEW,
            "Set Passcode",
            16,
            pcfg.digits_only,
            false,
        );
    }
}

struct DestructiveConfirm {
    action: ActionId,
    tag: u8,
    hold_ds: u8,
}

utils::static_bytes!(
    static DESTRUCTIVE_STRS = b"\
Format Storage?\0Erases user files & settings.\nCalibration is kept.\0\
Erase All Data?\0Erases all files, settings,\nand radio calibration!\0\
Reset Settings?\0Restores default settings.\nChannels & calib are kept.\0\
Restart Radio?\0Saves settings and restarts\nthe radio.\0";
);

#[cfg(feature = "filesystem")]
const DESTRUCTIVE_COUNT: usize = 4;
#[cfg(not(feature = "filesystem"))]
const DESTRUCTIVE_COUNT: usize = 2;

static DESTRUCTIVE_CONFIRMS: [DestructiveConfirm; DESTRUCTIVE_COUNT] = [
    #[cfg(feature = "filesystem")]
    DestructiveConfirm {
        action: ActionId::FormatKeepCalib,
        tag: TAG_FORMAT_KEEP_CALIB,
        hold_ds: 15,
    },
    #[cfg(feature = "filesystem")]
    DestructiveConfirm {
        action: ActionId::FormatWipeAll,
        tag: TAG_FORMAT_WIPE_ALL,
        hold_ds: 20,
    },
    DestructiveConfirm {
        action: ActionId::ResetSettings,
        tag: TAG_RESET_SETTINGS,
        hold_ds: 15,
    },
    DestructiveConfirm {
        action: ActionId::RebootFirmware,
        tag: TAG_REBOOT_FW,
        hold_ds: 8,
    },
];

#[inline(never)]
pub(super) fn info_popup(ctx: &mut SysContext, title: &str, msg: &str) {
    ctx.show_popup(TAG_POPUP_INFO, title, msg);
}

#[inline(never)]
pub fn show_radio_sheet(
    ctx: &mut SysContext,
    tag: u8,
    title: &'static str,
    options: gfx::gui::modules::menu::SheetOptions,
    selected: usize,
) {
    ctx.show_sheet(
        tag,
        OptionSheet::new(title, options, selected).with_radio(true),
    );
}

#[inline(never)]
pub fn show_destructive_hold_confirm(
    ctx: &mut SysContext,
    tag: u8,
    title: &str,
    message: &str,
    hold_ds: u8,
) {
    ctx.show_hold_confirm(
        tag,
        title,
        message,
        "Hold",
        "Cancel",
        (hold_ds as u16) * 100,
    );
}

#[inline(never)]
pub fn handle_action_press(ctx: &mut SysContext, action_id: ActionId) {
    for (i, spec) in DESTRUCTIVE_CONFIRMS.iter().enumerate() {
        if spec.action == action_id {
            let base = if cfg!(feature = "filesystem") {
                i
            } else {
                i + 2
            } as u8
                * 2;
            show_destructive_hold_confirm(
                ctx,
                spec.tag,
                utils::format::nth_cstr(&DESTRUCTIVE_STRS, base),
                utils::format::nth_cstr(&DESTRUCTIVE_STRS, base + 1),
                spec.hold_ds,
            );
            return;
        }
    }
    match action_id {
        ActionId::ChooseModel => {
            let opts = gfx::gui::modules::menu::SheetOptions::Packed(super::device::MODEL_OPTIONS);
            let sel = (ctx.config.system.model as usize).min(opts.len() - 1);
            show_radio_sheet(ctx, TAG_MODEL_CHOOSER, "Model", opts, sel);
        }
        #[cfg(feature = "text_input")]
        ActionId::EditOwner => prompt_config_str(ctx, TAG_OWNER_INPUT),
        #[cfg(feature = "text_input")]
        ActionId::EditTag => prompt_config_str(ctx, TAG_TAG_INPUT),
        #[cfg(feature = "text_input")]
        ActionId::EditRadioId => prompt_config_str(ctx, TAG_RADIO_ID_INPUT),
        #[cfg(feature = "settings_debug")]
        ActionId::ClearLogs => {
            crate::panic::BOOT_RECORD.get_mut().reset_all();
            ctx.show_popup(TAG_POPUP_INFO, "Debug", "Crash logs were cleared.");
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

#[inline(never)]
pub fn handle_sheet_selected(
    app: &mut SettingsApp,
    ctx: &mut SysContext,
    tag: u8,
    idx: usize,
) -> EventResult {
    match tag {
        #[cfg(feature = "filesystem")]
        TAG_ASSET_CHOOSER => {
            if let Some(desc) = app.active_asset.take() {
                if desc.apply_choice(idx, &mut ctx.config) {
                    super::options::apply_page_changes(app.current_page, ctx);
                }
            }
        }
        TAG_MODEL_CHOOSER => {
            if idx <= 1 {
                let name = if idx == 0 { "UV-K1" } else { "UV-K5 V3" };
                ctx.config.system.set_custom_model(name);
                ctx.config.system.model = idx as u8;
                ctx.config.sanitize();
                ctx.config.mark_system_dirty();
            } else {
                prompt_config_str(ctx, TAG_MODEL_INPUT);
                return EventResult::Consumed;
            }
        }
        TAG_CHOICE_SHEET => {
            app.handle_radio_confirmed(ctx, idx);
        }
        TAG_SHORTCUT_SHEET => {
            let slot = app.selected_idx.saturating_sub(1);
            if let Some(target_id) = crate::apps::shortcut::picker_target_at(idx) {
                match crate::apps::shortcut::assign_rule(
                    &mut ctx.config.keys,
                    app.shortcut_ctx,
                    slot,
                    target_id,
                ) {
                    1 => ctx.config.mark_keys_dirty(),
                    2 => info_popup(ctx, "Shortcuts", "Up to 3 custom rules\nare allowed."),
                    _ => {}
                }
            }
        }
        _ => return EventResult::Ignored,
    }
    app.flush_and_refresh(ctx);
    EventResult::Consumed
}

#[inline(never)]
pub fn handle_text_input_done(
    app: &mut SettingsApp,
    ctx: &mut SysContext,
    tag: u8,
    text: &str,
) -> EventResult {
    match tag {
        TAG_MODEL_INPUT | TAG_OWNER_INPUT | TAG_TAG_INPUT | TAG_RADIO_ID_INPUT => {
            match tag {
                TAG_MODEL_INPUT => {
                    ctx.config.system.set_custom_model(text);
                    ctx.config.sanitize();
                }
                TAG_RADIO_ID_INPUT => {
                    if let Some(id) = parse_u32_digits(text) {
                        ctx.config.system.radio_id = id;
                    }
                }
                _ => {
                    let dst: &mut [u8] = if tag == TAG_OWNER_INPUT {
                        &mut ctx.config.system.owner
                    } else {
                        &mut ctx.config.system.tag
                    };
                    copy_padded(dst, text.as_bytes());
                }
            }
            ctx.config.mark_system_dirty();
        }
        #[cfg(feature = "passcode")]
        TAG_PASSCODE_VERIFY => {
            super::security::handle_passcode_verify(ctx, text);
            return EventResult::Consumed;
        }
        #[cfg(feature = "passcode")]
        TAG_PASSCODE_NEW => {
            if text.is_empty() {
                let _ = crate::security::passcode::disable_passcode(ctx.storage, ctx.passcode);
                ctx.show_popup(TAG_POPUP_INFO, "", "Passcode removed.");
            } else {
                app.passcode_candidate.clear();
                let _ = app.passcode_candidate.push_str(text);
                let candidate_len = app.passcode_candidate.len();
                let digits_only =
                    crate::security::passcode::get_passcode_config(ctx.storage).digits_only;
                ctx.prompt_passcode(
                    TAG_PASSCODE_CONFIRM,
                    "Confirm Passcode",
                    candidate_len,
                    digits_only,
                    true,
                );
                return EventResult::Consumed;
            }
        }
        #[cfg(feature = "passcode")]
        TAG_PASSCODE_CONFIRM => {
            super::security::handle_passcode_confirm(app.passcode_candidate.as_str(), text, ctx);
        }
        _ => return EventResult::Ignored,
    }
    app.flush_and_refresh(ctx);
    EventResult::Consumed
}

#[inline(never)]
fn reset_config_to_defaults(ctx: &mut SysContext) {
    unsafe {
        crate::config::MasterConfig::init_in_place(ctx.config);
    }
    ctx.config.sanitize();
    #[cfg(feature = "filesystem")]
    let _ = ctx.config.save_all(ctx.storage);
}

#[inline(never)]
pub fn handle_confirm_result(
    #[allow(unused_variables)] app: &mut SettingsApp,
    ctx: &mut SysContext,
    tag: u8,
) -> EventResult {
    let (title, msg) = match tag {
        #[cfg(feature = "filesystem")]
        TAG_FORMAT_KEEP_CALIB | TAG_FORMAT_WIPE_ALL => {
            let keep = tag == TAG_FORMAT_KEEP_CALIB;
            if crate::services::calibration::reformat_storage(
                ctx.storage,
                ctx.calibration().raw_bytes(),
                keep,
            )
            .is_ok()
            {
                reset_config_to_defaults(ctx);
                app.flush_and_refresh(ctx);
                (
                    "Storage",
                    if keep {
                        "Storage formatted.\nCalibration was kept."
                    } else {
                        "All data erased."
                    },
                )
            } else {
                ("Storage", "Could not format storage.")
            }
        }
        TAG_RESET_SETTINGS => {
            reset_config_to_defaults(ctx);
            ctx.apply_display_config();
            ctx.apply_audio_config();
            ctx.power.load_battery_config(&ctx.config.battery);
            ctx.config.hw_dirty = 0x3F;
            app.populate_current_menu(ctx);
            ("Settings", "Settings restored to\ndefaults.")
        }
        TAG_REBOOT_FW => {
            let _ = ctx.flush_config();
            kernel::power::PowerControl::reboot();
        }
        _ => return EventResult::Ignored,
    };
    info_popup(ctx, title, msg);
    EventResult::Consumed
}
