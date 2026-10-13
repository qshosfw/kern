//! Security Domain Settings Declarations & Logic

use super::options::OFF_ON;
use crate::services::display::DisplayService;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, ShortString};

pub static TRIES_OPTS: [&str; 3] = ["3", "5", "10"];

pub fn populate_security(items: &mut [MenuItem], ctx: &mut crate::SysContext) -> usize {
    let sec = crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode);
    items[0].title = "Passcode";
    let status_str = if sec.pcfg.is_set { "Set" } else { "None" };
    items[0].item_type = MenuItemType::Value(ShortString::from_str(status_str));

    if sec.pcfg.is_set {
        items[1].title = "Show Length";
        items[1].item_type = MenuItemType::Choice {
            selected: sec.pcfg.expose_len as u16,
            options: &OFF_ON,
        };

        items[2].title = "Only Digits";
        items[2].item_type = MenuItemType::Choice {
            selected: sec.pcfg.digits_only as u16,
            options: &OFF_ON,
        };

        items[3].title = "Dimmed";
        items[3].item_type = MenuItemType::Choice {
            selected: sec.pcfg.stealth as u16,
            options: &OFF_ON,
        };

        let tries_sel = match sec.pcfg.max_tries {
            3 => 0,
            10 => 2,
            _ => 1,
        };
        items[4].title = "Max Tries";
        items[4].item_type = MenuItemType::Choice {
            selected: tries_sel as u16,
            options: &TRIES_OPTS,
        };
        5
    } else {
        1
    }
}

pub fn update_security_option(ctx: &mut crate::SysContext, sel_idx: usize, sel: usize) {
    let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
    let mut expose_len = pcfg.expose_len;
    let mut digits_only = pcfg.digits_only;
    let mut stealth = pcfg.stealth;
    let mut max_tries = pcfg.max_tries;
    match sel_idx {
        1 => expose_len = sel == 1,
        2 => digits_only = sel == 1,
        3 => stealth = sel == 1,
        4 => {
            max_tries = match sel {
                0 => 3,
                2 => 10,
                _ => 5,
            }
        }
        _ => return,
    }
    let _ = crate::security::passcode::set_passcode_options(
        ctx.storage,
        expose_len,
        stealth,
        digits_only,
        max_tries,
    );
}

fn show_progress_wdt(
    disp: &mut Option<&mut crate::services::SystemDisplay<'_, '_>>,
    title: &str,
    pct: u8,
) {
    py32_hal::wdt::Watchdog::feed();
    if let Some(d) = disp {
        d.show_progress("", title, pct);
    }
}

struct ProgressCtx<'a, 'b, 'c, 'd> {
    disp: &'a mut Option<&'b mut crate::services::SystemDisplay<'c, 'd>>,
    title: &'static str,
}

fn on_settings_progress(ctx: *mut (), pct: u8) {
    let pctx = unsafe { &mut *(ctx as *mut ProgressCtx) };
    show_progress_wdt(pctx.disp, pctx.title, pct);
}

#[inline(never)]
pub fn handle_passcode_verify(ctx: &mut crate::SysContext, text: &str) {
    let passcode = text.as_bytes();
    let mut disp = ctx.display.take();
    let res = {
        let mut pctx = ProgressCtx {
            disp: &mut disp,
            title: "Verifying...",
        };
        let hook = crate::security::ProgressHook::new(
            &mut pctx as *mut _ as *mut (),
            on_settings_progress,
        );
        crate::security::passcode::verify_passcode(ctx.storage, passcode, ctx.passcode, Some(hook))
    };
    ctx.display = disp;

    if res.unwrap_or(false) {
        let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
        ctx.prompt_passcode(
            super::app::TAG_PASSCODE_NEW,
            "Set Passcode",
            16,
            pcfg.digits_only,
            false,
        );
    } else {
        let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
        if pcfg.failed_attempts >= pcfg.max_tries {
            cortex_m::peripheral::SCB::sys_reset();
        }
        let mut nbuf = [0u8; 32];
        let msg = crate::security::passcode::format_wrong_passcode_msg(
            pcfg.failed_attempts,
            pcfg.max_tries,
            &mut nbuf,
        );
        ctx.show_popup_str(super::app::TAG_POPUP_INFO, "Passcode Error", msg);
    }
}

#[inline(never)]
pub fn handle_passcode_confirm(candidate: &str, text: &str, ctx: &mut crate::SysContext) {
    if text == candidate {
        let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
        let mut disp = ctx.display.take();
        let res = {
            let mut pctx = ProgressCtx {
                disp: &mut disp,
                title: "Saving...",
            };
            let hook = crate::security::ProgressHook::new(
                &mut pctx as *mut _ as *mut (),
                on_settings_progress,
            );
            crate::security::passcode::set_passcode(
                ctx.storage,
                ctx.passcode,
                candidate.as_bytes(),
                pcfg.expose_len,
                pcfg.stealth,
                pcfg.digits_only,
                pcfg.max_tries,
                Some(hook),
            )
        };
        ctx.display = disp;
        let msg = if res.is_ok() {
            "Passcode Saved"
        } else {
            "Failed to save"
        };
        ctx.show_popup(super::app::TAG_POPUP_INFO, "", msg);
    } else {
        ctx.show_popup(super::app::TAG_POPUP_INFO, "", "Mismatch");
    }
}
