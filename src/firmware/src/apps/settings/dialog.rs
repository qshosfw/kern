//! Modal Dialogs, Input Prompts & Action Dispatch for OS Settings

use super::app::SettingsApp;
use super::page::ActionId;
use crate::SysContext;
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
#[cfg(feature = "boot_assets")]
pub const TAG_ASSET_CHOOSER: u8 = 11;
pub const TAG_MODEL_CHOOSER: u8 = 12;
pub const TAG_MODEL_INPUT: u8 = 13;
pub const TAG_APP_RESET_CONFIRM: u8 = 15;

pub fn parse_u32_digits(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut val = 0u32;
    for &b in s.as_bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(val)
}

pub fn copy_padded(dst: &mut [u8], src: &[u8]) {
    dst.fill(b' ');
    let len = src.len().min(dst.len());
    dst[..len].copy_from_slice(&src[..len]);
}

pub fn prompt_config_str(ctx: &mut SysContext, tag: u8) {
    let mut buf = ShortString::empty();
    let (title, max_len, digits) = match tag {
        TAG_OWNER_INPUT => {
            buf.set_str(ctx.config.system.owner_str());
            ("Owner", 12, false)
        }
        TAG_TAG_INPUT => {
            buf.set_str(ctx.config.system.tag_str());
            ("Tag", 8, false)
        }
        TAG_MODEL_INPUT => {
            buf.set_str(ctx.config.system.custom_model_str());
            ("Model", 12, false)
        }
        _ => {
            buf.push_int_fixed(ctx.config.system.radio_id as i32, 0, "", "");
            ("Radio ID", 8, true)
        }
    };
    ctx.prompt_text(tag, title, buf.as_str(), max_len, digits);
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

#[inline(never)]
pub fn handle_action_press(ctx: &mut SysContext, action_id: ActionId) {
    match action_id {
        ActionId::ChooseModel => {
            let sel =
                (ctx.config.system.model as usize).min(super::device::MODEL_OPTIONS.len() - 1);
            ctx.show_sheet(
                TAG_MODEL_CHOOSER,
                OptionSheet::new("Model", &super::device::MODEL_OPTIONS, sel).with_radio(true),
            );
        }
        #[cfg(feature = "text_input")]
        ActionId::EditOwner => prompt_config_str(ctx, TAG_OWNER_INPUT),
        #[cfg(feature = "text_input")]
        ActionId::EditTag => prompt_config_str(ctx, TAG_TAG_INPUT),
        #[cfg(feature = "text_input")]
        ActionId::EditRadioId => prompt_config_str(ctx, TAG_RADIO_ID_INPUT),
        ActionId::ClearLogs => {
            #[cfg(feature = "flight_recorder")]
            crate::panic::BOOT_RECORD.get_mut().reset_all();
            ctx.show_popup(TAG_POPUP_INFO, "Debug", "Crash logs cleared");
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
        #[cfg(feature = "boot_assets")]
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
                ctx.show_popup(TAG_POPUP_INFO, "", "Passcode Removed");
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
pub fn handle_confirm_result(_app: &mut SettingsApp, ctx: &mut SysContext, tag: u8) -> EventResult {
    match tag {
        TAG_APP_RESET_CONFIRM => {
            ctx.show_popup(
                TAG_POPUP_INFO,
                "Config Reset",
                "Defaults restored on next launch",
            );
            EventResult::Consumed
        }
        _ => EventResult::Ignored,
    }
}
