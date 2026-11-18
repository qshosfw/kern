//! Security Domain Settings Declarations & Logic

use crate::services::display::DisplayService;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, SheetOptions, ShortString};

pub static TRIES_OPTS: &str = "3\05\010\0";

#[inline(never)]
pub fn populate_security(items: &mut [MenuItem], ctx: &mut crate::SysContext) -> usize {
    let sec = crate::security::passcode::get_security_summary(ctx.storage, ctx.passcode);
    items[0].title = "Passcode";
    let status_str = if sec.pcfg.is_set { "Set" } else { "None" };
    items[0].item_type = MenuItemType::Value(ShortString::from_str(status_str));

    if sec.pcfg.is_set {
        items[1].title = "Show Length";
        items[1].item_type = MenuItemType::Toggle(sec.pcfg.expose_len);

        items[2].title = "Only Digits";
        items[2].item_type = MenuItemType::Toggle(sec.pcfg.digits_only);

        items[3].title = "Dimmed";
        items[3].item_type = MenuItemType::Toggle(sec.pcfg.stealth);

        let tries_sel = match sec.pcfg.max_tries {
            3 => 0,
            10 => 2,
            _ => 1,
        };
        items[4].title = "Max Tries";
        items[4].item_type = MenuItemType::Choice {
            selected: tries_sel as u16,
            options: SheetOptions::Packed(TRIES_OPTS),
        };

        5
    } else {
        1
    }
}

pub fn update_security_toggle(ctx: &mut crate::SysContext, sel_idx: usize, val: bool) {
    let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
    let mut expose_len = pcfg.expose_len;
    let mut digits_only = pcfg.digits_only;
    let mut stealth = pcfg.stealth;
    match sel_idx {
        1 => expose_len = val,
        2 => digits_only = val,
        3 => stealth = val,
        _ => return,
    }
    let _ = crate::security::passcode::set_passcode_options(
        ctx.storage,
        expose_len,
        stealth,
        digits_only,
        pcfg.max_tries,
    );
}

pub fn update_security_option(ctx: &mut crate::SysContext, sel_idx: usize, sel: usize) {
    if sel_idx != 4 {
        return;
    }
    let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
    let max_tries = match sel {
        0 => 3,
        2 => 10,
        _ => 5,
    };
    let _ = crate::security::passcode::set_passcode_options(
        ctx.storage,
        pcfg.expose_len,
        pcfg.stealth,
        pcfg.digits_only,
        max_tries,
    );
}

fn show_progress_wdt(
    disp: &mut Option<&mut crate::services::DisplayHardware<'_, '_>>,
    title: &str,
    pct: u8,
) {
    py32_hal::wdt::Watchdog::feed();
    if let Some(d) = disp {
        d.show_progress("", title, pct);
    }
}

struct ProgressCtx<'a, 'b, 'c, 'd> {
    disp: &'a mut Option<&'b mut crate::services::DisplayHardware<'c, 'd>>,
    title: &'static str,
}

fn on_settings_progress(ctx: *mut (), pct: u8) {
    let pctx = unsafe { &mut *(ctx as *mut ProgressCtx) };
    show_progress_wdt(pctx.disp, pctx.title, pct);
}

#[inline(never)]
fn make_progress_hook(pctx: &mut ProgressCtx<'_, '_, '_, '_>) -> crate::security::ProgressHook {
    crate::security::ProgressHook::new(pctx as *mut _ as *mut (), on_settings_progress)
}

#[inline(never)]
pub(crate) fn with_passcode_progress<F, R>(
    disp: &mut Option<&mut crate::services::DisplayHardware<'_, '_>>,
    title: &'static str,
    f: F,
) -> R
where
    F: FnOnce(Option<crate::security::ProgressHook>) -> R,
{
    let mut d = disp.take();
    let res = {
        let mut pctx = ProgressCtx {
            disp: &mut d,
            title,
        };
        let hook = make_progress_hook(&mut pctx);
        f(Some(hook))
    };
    *disp = d;
    res
}

#[inline(never)]
fn verify_passcode_interactive(ctx: &mut crate::SysContext, text: &str) -> bool {
    let res = with_passcode_progress(&mut ctx.display, "Verifying...", |hook| {
        crate::security::passcode::verify_passcode(ctx.storage, text.as_bytes(), ctx.passcode, hook)
    });

    if res.unwrap_or(false) {
        true
    } else {
        let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
        if pcfg.failed_attempts >= pcfg.max_tries {
            kernel::power::PowerControl::reboot();
        }
        let mut nbuf = [0u8; 32];
        let msg = crate::security::passcode::format_wrong_passcode_msg(
            pcfg.failed_attempts,
            pcfg.max_tries,
            &mut nbuf,
        );
        ctx.show_popup(super::dialog::TAG_POPUP_INFO, "Passcode Error", msg);
        false
    }
}

#[inline(never)]
pub fn handle_passcode_verify(ctx: &mut crate::SysContext, text: &str) {
    if verify_passcode_interactive(ctx, text) {
        let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
        ctx.prompt_passcode(
            super::dialog::TAG_PASSCODE_NEW,
            "Set Passcode",
            16,
            pcfg.digits_only,
            false,
        );
    }
}

#[inline(never)]
pub fn handle_passcode_confirm(candidate: &str, text: &str, ctx: &mut crate::SysContext) {
    if text == candidate {
        let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
        let res = with_passcode_progress(&mut ctx.display, "Saving...", |hook| {
            crate::security::passcode::set_passcode(
                ctx.storage,
                ctx.passcode,
                candidate.as_bytes(),
                pcfg.expose_len,
                pcfg.stealth,
                pcfg.digits_only,
                pcfg.max_tries,
                hook,
            )
        });
        let msg = if res.is_ok() {
            "Passcode Saved"
        } else {
            "Failed to save"
        };
        ctx.show_popup(super::dialog::TAG_POPUP_INFO, "", msg);
    } else {
        ctx.show_popup(super::dialog::TAG_POPUP_INFO, "", "Mismatch");
    }
}
