//! UI & OS Compositor System Call Handlers (`CapUiVTable`, `interface_id = 0x0009`).
//!
//! Provides native executables with direct, declarative access to the microkernel
//! compositor spaces: dialogs, confirmations, text/passcode inputs, toasts, and animations.

use super::{active_ctx_opt, str_or_default};
use kernel::proc::abi::CapUiVTable;

static mut MODAL_TEXT_BUF: [u8; 64] = [0u8; 64];
static mut MODAL_TEXT_LEN: usize = 0;

pub fn store_modal_text(text: &str) {
    unsafe {
        let copy_len = text.len().min(64);
        core::ptr::copy_nonoverlapping(
            text.as_bytes().as_ptr(),
            core::ptr::addr_of_mut!(MODAL_TEXT_BUF) as *mut u8,
            copy_len,
        );
        MODAL_TEXT_LEN = copy_len;
    }
}

pub unsafe extern "C" fn api_ui_close_modal() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.close_modal();
    }
}

pub unsafe extern "C" fn api_ui_has_modal() -> u8 {
    if let Some(ctx) = active_ctx_opt() {
        if ctx.has_active_modal() { 1 } else { 0 }
    } else {
        0
    }
}

pub unsafe extern "C" fn api_ui_show_popup(
    tag: u8,
    title: *const u8,
    t_len: u8,
    msg: *const u8,
    m_len: u8,
) {
    let t_str = unsafe { str_or_default(title, t_len as usize, "Notice") };
    let m_str = unsafe { str_or_default(msg, m_len as usize, "") };
    if let Some(ctx) = active_ctx_opt() {
        ctx.show_popup_dynamic(tag, t_str, m_str);
    }
}

pub unsafe extern "C" fn api_ui_show_confirm(
    tag: u8,
    title: *const u8,
    t_len: u8,
    msg: *const u8,
    m_len: u8,
) {
    let t_str = unsafe { str_or_default(title, t_len as usize, "Confirm") };
    let m_str = unsafe { str_or_default(msg, m_len as usize, "") };
    if let Some(ctx) = active_ctx_opt() {
        ctx.show_confirm_dynamic(tag, t_str, m_str, "OK", "CANCEL");
    }
}

pub unsafe extern "C" fn api_ui_prompt_text(
    tag: u8,
    title: *const u8,
    t_len: u8,
    initial: *const u8,
    i_len: u8,
    max_len: u8,
) {
    let t_str = unsafe { str_or_default(title, t_len as usize, "Input") };
    let init_str = unsafe { str_or_default(initial, i_len as usize, "") };
    if let Some(ctx) = active_ctx_opt() {
        ctx.prompt_text_dynamic(tag, t_str, init_str, max_len as usize);
    }
}

pub unsafe extern "C" fn api_ui_get_modal_text(buf: *mut u8, max_len: u8) -> u8 {
    if buf.is_null() || max_len == 0 {
        return 0;
    }
    unsafe {
        let copy_len = MODAL_TEXT_LEN.min(max_len as usize);
        core::ptr::copy_nonoverlapping(
            core::ptr::addr_of!(MODAL_TEXT_BUF) as *const u8,
            buf,
            copy_len,
        );
        copy_len as u8
    }
}

pub unsafe extern "C" fn api_ui_show_toast(msg: *const u8, len: u8, duration_ms: u16) {
    let msg_str = unsafe { str_or_default(msg, len as usize, "") };
    if !msg_str.is_empty() {
        unsafe {
            (*core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL))
                .show_toast(msg_str, duration_ms);
        }
    }
}

pub unsafe extern "C" fn api_ui_is_animating() -> u8 {
    unsafe {
        if (*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL)).is_animating() {
            1
        } else {
            0
        }
    }
}

pub unsafe extern "C" fn api_ui_set_status_bar(sb: *const kernel::proc::abi::KrxStatusBar) -> u8 {
    crate::runtime::native_runner::set_slot_status_bar(sb);
    1
}

pub unsafe extern "C" fn api_ui_get_status_bar(out_sb: *mut kernel::proc::abi::KrxStatusBar) {
    if !out_sb.is_null() {
        crate::runtime::native_runner::get_slot_status_bar(out_sb);
    }
}

pub unsafe extern "C" fn api_ui_set_tray_icon(icon_id: u8) -> u8 {
    let pid = crate::runtime::native_runner::slot_ref().pid;
    if icon_id == 0xFF {
        crate::orchestrator::status_tray_mut().clear_icon(pid);
        1
    } else if let Some(icon) = kernel::canvas::Icon::from_u8(icon_id) {
        if crate::orchestrator::status_tray_mut().set_icon(pid, icon) {
            1
        } else {
            0
        }
    } else {
        0
    }
}

pub static UI_VTABLE: CapUiVTable = CapUiVTable {
    ui_close_modal: api_ui_close_modal,
    ui_has_modal: api_ui_has_modal,
    ui_show_popup: api_ui_show_popup,
    ui_show_confirm: api_ui_show_confirm,
    ui_prompt_text: api_ui_prompt_text,
    ui_get_modal_text: api_ui_get_modal_text,
    ui_show_toast: api_ui_show_toast,
    ui_is_animating: api_ui_is_animating,
    ui_set_status_bar: api_ui_set_status_bar,
    ui_get_status_bar: api_ui_get_status_bar,
    ui_set_tray_icon: api_ui_set_tray_icon,
};
