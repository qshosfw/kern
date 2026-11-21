//! UI & OS Compositor System Call Dispatcher (`Op::Ui*`, domain `0x7`).

use super::{active_canvas_mut, parse_str, slice_or_empty, str_or_default};
use crate::services::primitives::color_from_u8;
use kernel::proc::abi::*;

#[inline(always)]
fn modal_mut() -> &'static mut crate::orchestrator::compositor::ModalCompositor {
    unsafe { &mut *core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL) }
}

#[cfg(feature = "text_input")]
static mut MODAL_TEXT_BUF: [u8; 32] = [0u8; 32];
#[cfg(feature = "text_input")]
static mut MODAL_TEXT_LEN: u8 = 0;

pub fn store_modal_text(_text: &str) {
    #[cfg(feature = "text_input")]
    unsafe {
        let copy_len = _text.len().min(32);
        core::ptr::copy_nonoverlapping(
            _text.as_bytes().as_ptr(),
            core::ptr::addr_of_mut!(MODAL_TEXT_BUF) as *mut u8,
            copy_len,
        );
        MODAL_TEXT_LEN = copy_len as u8;
    }
}

pub fn set_tray_icon(icon_id: u8) -> u8 {
    let slot = crate::runtime::native_runner::slot_ref();
    let pid = slot.pid;
    if icon_id == KRX_ICON_NONE {
        crate::orchestrator::status_tray_mut().clear_icon(pid);
        1
    } else if let Some(g) =
        crate::runtime::native_runner::decode_status_glyph(&slot.status_bar, icon_id)
    {
        crate::orchestrator::status_tray_mut().set_glyph(pid, g) as u8
    } else {
        0
    }
}

unsafe fn show_modal_req(req: *const KrxModalReq) {
    if req.is_null() {
        return;
    }
    let r = unsafe { &*req };
    if r.kind == 2 {
        let t_str = unsafe { str_or_default(r.title, r.t_len as usize, "Input") };
        let init_str = unsafe { str_or_default(r.msg, r.m_len as usize, "") };
        #[cfg(feature = "text_input")]
        modal_mut().prompt_text(r.tag, t_str, init_str, r.max_len as usize, false);
        #[cfg(not(feature = "text_input"))]
        let _ = (t_str, init_str);
        return;
    }
    if r.kind == 3 {
        let t_str = unsafe { str_or_default(r.title, r.t_len as usize, "Select") };
        let opts_len = (r.m_len as usize) | ((r.o_len as usize) << 8);
        if let Some(raw) = unsafe { super::ptr_slice(r.msg, opts_len) } {
            if raw.iter().all(|&b| b == 0 || (0x20..=0x7E).contains(&b)) {
                let opts = unsafe { core::str::from_utf8_unchecked(raw) };
                let sheet = gfx::gui::modules::menu::OptionSheet::new(
                    t_str,
                    gfx::gui::modules::menu::SheetOptions::Packed(opts),
                    r.max_len as usize,
                )
                .with_radio(r.c_len != 0);
                modal_mut().show_sheet(r.tag, sheet);
            }
        }
        return;
    }
    utils::static_bytes!(
        static DEFS = b"Notice\0\0\0\0Confirm\0\0OK\0CANCEL\0";
    );
    let base = u8::from(r.kind != 0) * 4;
    let pick = |p: *const u8, len: u8, off: u8| unsafe {
        str_or_default(p, len as usize, utils::format::nth_cstr(&DEFS, base + off))
    };
    modal_mut().show_confirm(
        r.tag,
        pick(r.title, r.t_len, 0),
        pick(r.msg, r.m_len, 1),
        pick(r.ok, r.o_len, 2),
        pick(r.cancel, r.c_len, 3),
    );
}

#[inline(always)]
fn tr_ui<'a>(s: &'a str, prefer_short: bool, buf: &'a mut [u8; 48]) -> &'a str {
    #[cfg(feature = "filesystem")]
    if !s.is_empty() && crate::services::lang::is_active() {
        #[cfg(not(test))]
        return crate::services::lang::translate(crate::kern_fs(), s, prefer_short, buf);
        #[cfg(test)]
        if let Some(ctx) = super::active_ctx_opt() {
            return crate::services::lang::translate(ctx.storage, s, prefer_short, buf);
        }
    }
    let _ = (prefer_short, buf);
    s
}

unsafe fn draw_widget(kind: u8, desc: *const KrxWidget) -> usize {
    if desc.is_null() {
        return 1;
    }
    let d = unsafe { &*desc };
    let canvas = active_canvas_mut();
    let ux = d.x.max(0) as u8;
    let uy = d.y.max(0) as u8;
    let raw0 = unsafe { slice_or_empty(d.p0, d.len0 as usize) };
    let s0_raw = utils::format::ascii_str(raw0).unwrap_or("");
    let mut tr_buf = [0u8; 48];
    let s0 = if matches!(
        kind,
        KRX_WIDGET_NEGATED_HEADER
            | KRX_WIDGET_SHEET_FRAME
            | KRX_WIDGET_CAPSULE_BUTTON
            | KRX_WIDGET_MENU_ITEM
    ) {
        tr_ui(s0_raw, kind == KRX_WIDGET_CAPSULE_BUTTON, &mut tr_buf)
    } else {
        s0_raw
    };
    let badge = |filled: bool| {
        if filled {
            gfx::widgets::BadgeStyle::Filled
        } else {
            gfx::widgets::BadgeStyle::Empty
        }
    };
    match kind {
        KRX_WIDGET_ROUNDED_BOX => gfx::widgets::draw_rounded_box(canvas, ux, uy, d.w, d.h),
        KRX_WIDGET_NEGATED_HEADER => gfx::widgets::draw_negated_header(canvas, d.y, s0),
        KRX_WIDGET_SHEET_FRAME => {
            gfx::widgets::draw_sheet_frame(canvas, s0, d.y, d.h);
        }
        KRX_WIDGET_PROGRESS_BAR => {
            gfx::widgets::draw_progress_bar(canvas, d.x, d.y, d.w, d.h, d.a, d.b != 0);
        }
        KRX_WIDGET_S_METER => {
            gfx::widgets::draw_s_meter(
                canvas,
                d.x,
                d.y,
                d.extra as i16,
                d.a,
                &gfx::fonts::tomthumb::FONT,
            );
        }
        KRX_WIDGET_TAG_BADGE => {
            if !s0.is_empty() {
                gfx::widgets::draw_tag_badge(canvas, d.x, d.y, s0, badge(d.a == 1));
            }
        }
        KRX_WIDGET_KEY_BADGE => {
            let s1 = unsafe { parse_str(d.p1, d.len1 as usize) }.unwrap_or("");
            gfx::widgets::draw_key_badge(canvas, d.x, d.y, s0, s1);
        }
        KRX_WIDGET_CAPSULE_BUTTON => {
            gfx::widgets::draw_capsule_button(canvas, d.x, d.y, d.w, d.h, s0, badge(d.a != 0));
        }
        KRX_WIDGET_SCROLLBAR => {
            gfx::widgets::draw_scrollbar(
                canvas,
                ux,
                uy,
                d.h,
                d.len0 as usize,
                d.extra as usize,
                d.len1 as usize,
            );
        }
        KRX_WIDGET_MENU_ITEM => {
            let is_sel = (d.a & 1) != 0;
            let has_chevron = (d.a & 2) != 0;
            let is_outline = (d.a & 4) != 0;
            let is_toggle = (d.a & 8) != 0;
            let toggle_on = (d.a & 16) != 0;
            let comfortable = d.h >= 12;
            let row_h: u8 = if comfortable { 13 } else { 10 };
            let icon_dy: i16 = if comfortable { 2 } else { 1 };
            let text_dy: i16 = if comfortable { 9 } else { 7 };
            let filled_sel = is_sel && !is_outline;
            let tc_u8 = if filled_sel { 0 } else { 1 };
            let col = color_from_u8(tc_u8);
            if is_sel {
                let cap_flags = if is_outline {
                    gfx::widgets::CAPSULE_ROUND
                } else {
                    gfx::widgets::CAPSULE_FILL_ROUND
                };
                gfx::widgets::draw_selection_capsule(canvas, [2, uy, 120, row_h], cap_flags);
            }
            let text_x = if d.b != 0 {
                super::display::draw_icon(4, d.y + icon_dy, d.b, tc_u8);
                15
            } else {
                5
            };
            let s1 = unsafe { parse_str(d.p1, d.len1 as usize) }.unwrap_or("");
            let title_max = if is_toggle || has_chevron {
                108i16
            } else if !s1.is_empty() {
                (116 - gfx::text::str_width(s1, &gfx::fonts::tomthumb::FONT)).max(36)
            } else {
                119i16
            };
            gfx::text::draw_str_ellipsis(
                canvas,
                text_x,
                d.y + text_dy,
                s0,
                &gfx::fonts::mumatrix_regular::FONT,
                col,
                title_max,
            );
            if is_toggle {
                let ic = if toggle_on {
                    ::abi::icon::CHECKBOX_ON
                } else {
                    ::abi::icon::CHECKBOX_OFF
                };
                super::display::draw_icon(112, d.y + icon_dy, ic, tc_u8);
            } else if !s1.is_empty() {
                gfx::text::draw_str_align(
                    canvas,
                    119,
                    d.y + text_dy,
                    s1,
                    &gfx::fonts::tomthumb::FONT,
                    col,
                    gfx::text::Align::Right,
                );
            } else if has_chevron {
                let phase = ((d.extra >> 1) & 3) as i16;
                let dx = if is_sel && d.extra != 0 {
                    if phase == 3 { 1 } else { phase }
                } else {
                    0
                };
                super::display::draw_icon(
                    112 + dx,
                    d.y + icon_dy + i16::from(comfortable),
                    35,
                    tc_u8,
                );
            }
        }
        KRX_WIDGET_BIG_DIGIT | KRX_WIDGET_SMALL_DIGIT => {
            gfx::widgets::spectrum::draw_digit(
                canvas,
                d.x,
                d.y,
                d.a,
                color_from_u8(d.b),
                kind == KRX_WIDGET_BIG_DIGIT,
            );
        }
        KRX_WIDGET_BATTERY => gfx::widgets::draw_battery(canvas, d.x, d.y, d.a, d.b != 0),
        KRX_WIDGET_SIGNAL => {
            gfx::widgets::draw_signal(canvas, d.x, d.y, d.a, color_from_u8(d.b));
        }
        KRX_WIDGET_MODAL_BOX => {
            let (ox, oy) = gfx::widgets::draw_modal_box(canvas, d.w, d.h);
            let out = d.p0 as *mut u8;
            if !out.is_null() {
                unsafe {
                    *out = ox;
                    *out.add(1) = oy;
                }
            }
        }
        _ => return 1,
    }
    0
}

#[inline(never)]
pub unsafe fn dispatch_ui(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::{
        idx as op,
        sub::{ui_modal, ui_status},
    };
    match idx {
        op::UiModal => match a0 as u8 {
            ui_modal::CLOSE => {
                crate::orchestrator::sink_enter_keys();
                modal_mut().close();
                0
            }
            ui_modal::ACTIVE => modal_mut().is_active() as usize,
            ui_modal::ANIMATING => modal_mut().is_animating() as usize,
            ui_modal::SET_HOLD => {
                modal_mut().set_hold_ms(a1 as u16);
                0
            }
            ui_modal::GET_TEXT => {
                #[cfg(feature = "text_input")]
                {
                    let buf = a1 as *mut u8;
                    let max_len = a2 as u8;
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
                        copy_len
                    }
                }
                #[cfg(not(feature = "text_input"))]
                0
            }
            ui_modal::SHOW => {
                unsafe { show_modal_req(a1 as *const KrxModalReq) };
                0
            }
            ui_modal::TOAST => {
                let msg_str = unsafe { str_or_default(a1 as *const u8, (a2 as u8) as usize, "") };
                if !msg_str.is_empty() {
                    modal_mut().show_toast(msg_str, (a2 >> 16) as u16);
                }
                0
            }
            _ => 0,
        },
        op::UiStatus => match a0 as u8 {
            ui_status::SET_BAR => {
                crate::runtime::native_runner::set_slot_status_bar(a1 as *const KrxStatusBar);
                1
            }
            ui_status::GET_BAR => {
                crate::runtime::native_runner::get_slot_status_bar(a1 as *mut KrxStatusBar);
                0
            }
            ui_status::TRAY_ICON => set_tray_icon(a1 as u8) as usize,
            _ => 0,
        },
        op::UiWidget => unsafe { draw_widget(a0 as u8, a1 as *const KrxWidget) },
        _ => 0,
    }
}
