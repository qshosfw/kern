//! UI & OS Compositor System Call Handlers (`UiApi`, `interface_id = 0x0009`).
//!
//! Provides native executables with direct, declarative access to the microkernel
//! compositor spaces: dialogs, confirmations, text/passcode inputs, toasts, animations,
//! and native UI widgets.

use super::str_or_default;
use kernel::proc::abi::UiApi;

#[inline(always)]
fn modal_mut() -> &'static mut crate::orchestrator::compositor::ModalCompositor {
    unsafe { &mut *core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL) }
}

static mut MODAL_TEXT_BUF: [u8; 32] = [0u8; 32];
static mut MODAL_TEXT_LEN: u8 = 0;

pub fn store_modal_text(text: &str) {
    unsafe {
        let copy_len = text.len().min(32);
        core::ptr::copy_nonoverlapping(
            text.as_bytes().as_ptr(),
            core::ptr::addr_of_mut!(MODAL_TEXT_BUF) as *mut u8,
            copy_len,
        );
        MODAL_TEXT_LEN = copy_len as u8;
    }
}

pub unsafe extern "C" fn api_ui_close_modal() {
    crate::orchestrator::sink_enter_keys();
    modal_mut().close();
}

pub unsafe extern "C" fn api_ui_has_modal() -> u8 {
    modal_mut().is_active() as u8
}

#[inline(never)]
fn show_modal_confirm(
    tag: u8,
    title: *const u8,
    t_len: u8,
    default_title: &'static str,
    msg: *const u8,
    m_len: u8,
    confirm: &'static str,
    cancel: &'static str,
) {
    let t_str = unsafe { str_or_default(title, t_len as usize, default_title) };
    let m_str = unsafe { str_or_default(msg, m_len as usize, "") };
    modal_mut().show_confirm(tag, t_str, m_str, confirm, cancel);
}

pub unsafe extern "C" fn api_ui_show_popup(
    tag: u8,
    title: *const u8,
    t_len: u8,
    msg: *const u8,
    m_len: u8,
) {
    show_modal_confirm(tag, title, t_len, "Notice", msg, m_len, "", "");
}

pub unsafe extern "C" fn api_ui_show_confirm(
    tag: u8,
    title: *const u8,
    t_len: u8,
    msg: *const u8,
    m_len: u8,
) {
    show_modal_confirm(tag, title, t_len, "Confirm", msg, m_len, "OK", "CANCEL");
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
    #[cfg(feature = "text_input")]
    modal_mut().prompt_text(tag, t_str, init_str, max_len as usize, false);
    #[cfg(not(feature = "text_input"))]
    let _ = (tag, t_str, init_str, max_len);
}

pub unsafe extern "C" fn api_ui_get_modal_text(buf: *mut u8, max_len: u8) -> u8 {
    if buf.is_null() || max_len == 0 {
        return 0;
    }
    unsafe {
        let copy_len = (MODAL_TEXT_LEN as usize).min(max_len as usize);
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
    crate::runtime::native_runner::get_slot_status_bar(out_sb);
}

pub unsafe extern "C" fn api_ui_set_tray_icon(icon_id: u8) -> u8 {
    let slot = crate::runtime::native_runner::slot_ref();
    let pid = slot.pid;
    if icon_id == kernel::proc::abi::KRX_ICON_NONE {
        crate::orchestrator::status_tray_mut().clear_icon(pid);
        1
    } else {
        let glyph = crate::runtime::native_runner::decode_status_glyph(
            icon_id,
            &slot.status_bar.custom_icon_0,
            &slot.status_bar.custom_icon_1,
        );
        if let Some(g) = glyph {
            crate::orchestrator::status_tray_mut().set_glyph(pid, g) as u8
        } else {
            0
        }
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
    gfx::widgets::draw_progress_bar(super::active_canvas_mut(), x, y, w, h, percent, border != 0);
}

pub unsafe extern "C" fn api_ui_draw_s_meter(x: i16, y: i16, rssi_dbm: i16, peak_hold: u8) {
    gfx::widgets::draw_s_meter(
        super::active_canvas_mut(),
        x,
        y,
        rssi_dbm,
        peak_hold,
        &gfx::fonts::tomthumb::FONT,
    );
}

pub unsafe extern "C" fn api_ui_draw_tag_badge(
    x: i16,
    y: i16,
    text: *const u8,
    len: u8,
    style: u8,
) {
    if let Some(s) = unsafe { super::parse_str(text, len as usize) } {
        let st = if style == 1 {
            gfx::widgets::BadgeStyle::Filled
        } else {
            gfx::widgets::BadgeStyle::Empty
        };
        gfx::widgets::draw_tag_badge(super::active_canvas_mut(), x, y, s, st);
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
    gfx::widgets::draw_key_badge(super::active_canvas_mut(), x, y, k_s, l_s);
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
    let s = unsafe { super::parse_str(label, l_len as usize) }.unwrap_or("");
    let st = if selected != 0 {
        gfx::widgets::BadgeStyle::Filled
    } else {
        gfx::widgets::BadgeStyle::Empty
    };
    gfx::widgets::draw_capsule_button(super::active_canvas_mut(), x, y, w, h, s, st);
}

pub unsafe extern "C" fn api_ui_draw_scrollbar(
    x: u8,
    y: u8,
    h: u8,
    current: u16,
    total: u16,
    visible: u16,
) {
    gfx::widgets::draw_scrollbar(
        super::active_canvas_mut(),
        x,
        y,
        h,
        current as usize,
        total as usize,
        visible as usize,
    );
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
    let is_sel = selected != 0;
    let ry = y.max(0) as u8;
    let tc_u8 = if is_sel { 0 } else { 1 };
    let canvas = super::active_canvas_mut();
    if is_sel {
        canvas.fill_rect(2, ry, 120, 10, gfx::Color::Fill);
        canvas.clear_corners(2, ry, 120, 10);
    }
    let mut text_x = 5i16;
    if icon_id != 0 {
        unsafe {
            super::display::api_draw_icon(4, y + 1, icon_id, tc_u8);
        }
        text_x = 15;
    }
    unsafe {
        super::display::api_draw_str_align(text_x, y + 7, title, t_len as u16, 1, tc_u8, 0);
        super::display::api_draw_str_align(119, y + 7, value, v_len as u16, 0, tc_u8, 2);
    }
}

pub unsafe extern "C" fn api_ui_draw_big_digit(x: i16, y: i16, digit: u8, color: u8) {
    gfx::widgets::spectrum::draw_big_digit(
        super::active_canvas_mut(),
        x,
        y,
        digit,
        crate::services::primitives::color_from_u8(color),
    );
}

pub unsafe extern "C" fn api_ui_draw_small_digit(x: i16, y: i16, digit: u8, color: u8) {
    gfx::widgets::spectrum::draw_small_digit(
        super::active_canvas_mut(),
        x,
        y,
        digit,
        crate::services::primitives::color_from_u8(color),
    );
}

pub unsafe extern "C" fn api_ui_draw_battery(x: i16, y: i16, pct: u8, charging: u8) {
    if (x as u16) > (128 - 15) || (y as u16) > (64 - 6) {
        return;
    }
    let canvas = super::active_canvas_mut();
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
        let fill_w = ((pct.min(100) as u16 * 28) >> 8) as u8;
        if fill_w > 0 {
            canvas.fill_rect(ux + 1, uy + 1, fill_w, 4, gfx::Color::Fill);
        }
    }
}

pub unsafe extern "C" fn api_ui_draw_signal(x: i16, y: i16, level: u8, color: u8) {
    gfx::widgets::draw_signal(
        super::active_canvas_mut(),
        x,
        y,
        level,
        crate::services::primitives::color_from_u8(color),
    );
}

pub unsafe extern "C" fn api_ui_draw_modal_box(w: u8, h: u8, out_origin: *mut u8) {
    let (ox, oy) = gfx::widgets::draw_modal_box(super::active_canvas_mut(), w, h);
    if !out_origin.is_null() {
        unsafe {
            *out_origin = ox;
            *out_origin.add(1) = oy;
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
    let c_str = unsafe { str_or_default(confirm, c_len as usize, "OK") };
    let cl_str = unsafe { str_or_default(cancel, cl_len as usize, "CANCEL") };
    show_modal_confirm(tag, title, t_len, "Confirm", msg, m_len, c_str, cl_str);
}

pub static UI_VTABLE: UiApi = UiApi {
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
    ui_draw_progress_bar: api_ui_draw_progress_bar,
    ui_draw_s_meter: api_ui_draw_s_meter,
    ui_draw_tag_badge: api_ui_draw_tag_badge,
    ui_draw_key_badge: api_ui_draw_key_badge,
    ui_draw_capsule_button: api_ui_draw_capsule_button,
    ui_draw_scrollbar: api_ui_draw_scrollbar,
    ui_draw_menu_item: api_ui_draw_menu_item,
    ui_draw_big_digit: api_ui_draw_big_digit,
    ui_draw_small_digit: api_ui_draw_small_digit,
    ui_draw_battery: api_ui_draw_battery,
    ui_draw_signal: api_ui_draw_signal,
    ui_draw_modal_box: api_ui_draw_modal_box,
    ui_show_confirm_labels: api_ui_show_confirm_labels,
};
