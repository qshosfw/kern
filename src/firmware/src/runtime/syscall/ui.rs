//! UI & OS Compositor System Call Handlers (`CapUiVTable`, `interface_id = 0x0009`).
//!
//! Provides native executables with direct, declarative access to the microkernel
//! compositor spaces: dialogs, confirmations, text/passcode inputs, toasts, animations,
//! and native UI widgets.

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

pub unsafe extern "C" fn api_ui_draw_progress_bar(
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    percent: u8,
    border: u8,
) {
    if let Some(canvas) = super::active_canvas_opt() {
        gfx::widgets::draw_progress_bar(canvas, x, y, w, h, percent, border != 0);
    }
}

pub unsafe extern "C" fn api_ui_draw_s_meter(x: i16, y: i16, rssi_dbm: i16, peak_hold: u8) {
    if let Some(canvas) = super::active_canvas_opt() {
        gfx::widgets::draw_s_meter(
            canvas,
            x,
            y,
            rssi_dbm,
            peak_hold,
            &gfx::fonts::tomthumb::FONT,
        );
    }
}

pub unsafe extern "C" fn api_ui_draw_tag_badge(
    x: i16,
    y: i16,
    text: *const u8,
    len: u8,
    style: u8,
) {
    if let Some(s) = unsafe { super::parse_str(text, len as usize) } {
        if let Some(canvas) = super::active_canvas_opt() {
            let bstyle = match style {
                0 => gfx::widgets::badge::BadgeStyle::Empty,
                1 => gfx::widgets::badge::BadgeStyle::Filled,
                2 => gfx::widgets::badge::BadgeStyle::Dimmed,
                3 => gfx::widgets::badge::BadgeStyle::Disabled,
                d => gfx::widgets::badge::BadgeStyle::Dither(d.saturating_sub(4)),
            };
            gfx::widgets::badge::draw_tag_badge(canvas, x, y, s, bstyle);
        }
    }
}

pub unsafe extern "C" fn api_ui_draw_key_badge(
    x: i16,
    y: i16,
    key_str: *const u8,
    k_len: u8,
    label: *const u8,
    l_len: u8,
) {
    let k_s = unsafe { super::parse_str(key_str, k_len as usize) }.unwrap_or("");
    let l_s = unsafe { super::parse_str(label, l_len as usize) }.unwrap_or("");
    if let Some(canvas) = super::active_canvas_opt() {
        gfx::widgets::draw_key_badge(canvas, x, y, k_s, l_s);
    }
}

pub unsafe extern "C" fn api_ui_draw_capsule_button(
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    label: *const u8,
    l_len: u8,
    selected: u8,
) {
    let l_s = unsafe { super::parse_str(label, l_len as usize) }.unwrap_or("");
    if let Some(canvas) = super::active_canvas_opt() {
        let bstyle = if selected != 0 {
            gfx::widgets::badge::BadgeStyle::Filled
        } else {
            gfx::widgets::badge::BadgeStyle::Empty
        };
        gfx::widgets::badge::draw_capsule_button(canvas, x, y, w, h, l_s, bstyle);
    }
}

pub unsafe extern "C" fn api_ui_draw_scrollbar(
    x: u8,
    y: u8,
    h: u8,
    current: u16,
    total: u16,
    visible: u16,
) {
    if let Some(canvas) = super::active_canvas_opt() {
        gfx::widgets::draw_scrollbar(
            canvas,
            x,
            y,
            h,
            current as usize,
            total as usize,
            visible as usize,
        );
    }
}

pub unsafe extern "C" fn api_ui_draw_menu_item(
    y: i16,
    title: *const u8,
    t_len: u8,
    value: *const u8,
    v_len: u8,
    selected: u8,
    icon_id: u8,
) {
    if let Some(canvas) = super::active_canvas_opt() {
        let is_sel = selected != 0;
        let ry = y.max(0) as u8;
        let mut tc = gfx::Color::Fill;
        if is_sel {
            canvas.fill_rect(0, ry, 124, 11, gfx::Color::Fill);
            canvas.clear_corners(0, ry, 124, 11);
            tc = gfx::Color::Clear;
        }

        let mut text_x = 4i16;
        if icon_id != 0 {
            if let Some(bits) = super::display::system_icon_8(icon_id) {
                gfx::icons::draw_icon_8(canvas, 3, y + 2, bits, tc);
                text_x = 15;
            } else if let Some(icon) = kernel::canvas::Icon::from_u8(icon_id) {
                gfx::icons::draw_icon(canvas, 3, y + 2, icon, tc);
                text_x = 15;
            }
        }

        if let Some(t) = unsafe { super::parse_str(title, t_len as usize) } {
            gfx::text::draw_str(
                canvas,
                text_x,
                y + 8,
                t,
                &gfx::fonts::mumatrix_regular::FONT,
                tc,
            );
        }

        if let Some(v) = unsafe { super::parse_str(value, v_len as usize) } {
            let vw = gfx::text::str_width(v, &gfx::fonts::tomthumb::FONT);
            gfx::text::draw_str(canvas, 120 - vw, y + 8, v, &gfx::fonts::tomthumb::FONT, tc);
        } else if is_sel {
            gfx::icons::draw_icon_8(canvas, 116, y + 2, &gfx::icons::ARROW_RIGHT, tc);
        }
    }
}

pub unsafe extern "C" fn api_ui_draw_big_digit(x: i16, y: i16, digit: u8, color: u8) {
    let col = crate::services::primitives::color_from_u8(color);
    if let Some(canvas) = super::active_canvas_opt() {
        gfx::widgets::spectrum::draw_big_digit(canvas, x, y, digit, col);
    }
}

pub unsafe extern "C" fn api_ui_draw_small_digit(x: i16, y: i16, digit: u8, color: u8) {
    let col = crate::services::primitives::color_from_u8(color);
    if let Some(canvas) = super::active_canvas_opt() {
        gfx::widgets::spectrum::draw_small_digit(canvas, x, y, digit, col);
    }
}

pub unsafe extern "C" fn api_ui_draw_battery(x: i16, y: i16, pct: u8, charging: u8) {
    if let Some(canvas) = super::active_canvas_opt() {
        if x < 0 || x + 15 > 128 || y < 0 || y + 6 > 64 {
            return;
        }
        let ux = x as u8;
        let uy = y as u8;
        canvas.draw_rect(ux, uy, 13, 6, gfx::Color::Fill);
        canvas.draw_vertical_line(ux + 13, uy + 1, 4, gfx::Color::Fill);
        canvas.fill_rect(ux + 1, uy + 1, 11, 4, gfx::Color::Clear);
        if charging != 0 {
            gfx::icons::draw_icon_5x5(
                canvas,
                x + 4,
                y + 1,
                gfx::icons::Icon::Bolt,
                gfx::Color::Fill,
            );
        } else {
            let fill_w = ((pct.min(100) as u16 * 11) / 100) as u8;
            if fill_w > 0 {
                canvas.fill_rect(ux + 1, uy + 1, fill_w, 4, gfx::Color::Fill);
            }
        }
    }
}

pub unsafe extern "C" fn api_ui_draw_signal(x: i16, y: i16, level: u8, color: u8) {
    let col = crate::services::primitives::color_from_u8(color);
    if let Some(canvas) = super::active_canvas_opt() {
        gfx::widgets::draw_signal(canvas, x, y, level, col);
    }
}

pub unsafe extern "C" fn api_ui_draw_modal_box(w: u8, h: u8, out_origin: *mut u8) {
    if let Some(canvas) = super::active_canvas_opt() {
        let (ox, oy) = gfx::widgets::draw_modal_box(canvas, w, h);
        if !out_origin.is_null() {
            unsafe {
                *out_origin = ox;
                *out_origin.add(1) = oy;
            }
        }
    }
}

pub unsafe extern "C" fn api_ui_show_confirm_labels(
    tag: u8,
    title: *const u8,
    t_len: u8,
    msg: *const u8,
    m_len: u8,
    confirm: *const u8,
    c_len: u8,
    cancel: *const u8,
    cl_len: u8,
) {
    let t_str = unsafe { str_or_default(title, t_len as usize, "Confirm") };
    let m_str = unsafe { str_or_default(msg, m_len as usize, "") };
    let c_str = unsafe { str_or_default(confirm, c_len as usize, "OK") };
    let cl_str = unsafe { str_or_default(cancel, cl_len as usize, "CANCEL") };
    if let Some(ctx) = active_ctx_opt() {
        ctx.show_confirm_dynamic(tag, t_str, m_str, c_str, cl_str);
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
    ..kernel::proc::abi::STUB_UI_VTABLE
};
