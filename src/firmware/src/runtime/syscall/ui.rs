//! UI & OS Compositor System Call Dispatcher (`Op::Ui*`, domain `0x7`).

use super::{active_canvas_mut, parse_str, slice_or_empty, str_or_default};
use crate::services::primitives::color_from_u8;
use kernel::proc::abi::*;

#[inline(always)]
fn modal_mut() -> &'static mut crate::orchestrator::compositor::ModalCompositor {
    unsafe { &mut *core::ptr::addr_of_mut!(crate::orchestrator::compositor::ACTIVE_MODAL) }
}

#[cfg(feature = "text_input")]
#[repr(C)]
struct ModalTextBuf {
    len: u8,
    buf: [u8; 31],
}

#[cfg(feature = "text_input")]
static mut MODAL_TEXT: ModalTextBuf = ModalTextBuf {
    len: 0,
    buf: [0u8; 31],
};

pub fn store_modal_text(_text: &str) {
    #[cfg(feature = "text_input")]
    unsafe {
        let m = &mut *core::ptr::addr_of_mut!(MODAL_TEXT);
        let copy_len = _text.len().min(m.buf.len());
        core::ptr::copy_nonoverlapping(_text.as_bytes().as_ptr(), m.buf.as_mut_ptr(), copy_len);
        m.len = copy_len as u8;
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

/// Ensure a guest pointer kept alive across frames (e.g. `OptionSheet`) never points
/// into a volatile P0 paged leaf overlay window (`>= overlay_vma`).
#[inline(never)]
fn is_persistent_guest_ptr(ptr: *const u8, len: usize) -> bool {
    if ptr.is_null() {
        return true;
    }
    let slot = crate::runtime::native_runner::slot_ref();
    if !slot.is_loaded || !slot.profile.is_structured_paged() {
        return true;
    }
    let base = crate::runtime::native_runner::arena_base_ptr() as usize;
    let overlay_start = base + (crate::data::arena::TRANSIENT_ARENA_BYTES - 1024);
    let overlay_end = base + crate::data::arena::TRANSIENT_ARENA_BYTES;
    let addr = ptr as usize;
    let end = addr.saturating_add(len);
    !(addr < overlay_end && end > overlay_start)
}

#[inline(never)]
unsafe fn show_modal_req(req: *const KrxModalReq) {
    if req.is_null() {
        return;
    }
    let r = unsafe { &*req };
    if r.kind == 2 {
        let t_str = unsafe { str_or_default(r.title, r.t_len as usize, "Input") };
        let init_str = unsafe { str_or_default(r.msg, r.m_len as usize, "") };
        #[cfg(feature = "text_input")]
        {
            let flags = r.o_len;
            let digits_only = (flags & ::abi::TEXT_FLAG_DIGITS_ONLY) != 0;
            if (flags & ::abi::TEXT_FLAG_PASSCODE) != 0 {
                let m = modal_mut().prompt_passcode(
                    r.tag,
                    t_str,
                    r.max_len as usize,
                    digits_only,
                    (flags & ::abi::TEXT_FLAG_EXPOSE_LEN) != 0,
                );
                m.is_k1 = crate::master_config_ref().is_k1();
            } else {
                modal_mut().prompt_text(r.tag, t_str, init_str, r.max_len as usize, digits_only);
            }
        }
        #[cfg(not(feature = "text_input"))]
        let _ = (t_str, init_str);
        return;
    }
    if r.kind == 3 {
        let opts_len = (r.m_len as usize) | ((r.o_len as usize) << 8);
        if !is_persistent_guest_ptr(r.title, r.t_len as usize)
            || !is_persistent_guest_ptr(r.msg, opts_len)
        {
            return;
        }
        let t_str = unsafe { str_or_default(r.title, r.t_len as usize, "Select") };
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
    #[inline(never)]
    unsafe fn pick_str(p: *const u8, len: u8, idx: u8) -> &'static str {
        unsafe { str_or_default(p, len as usize, utils::format::nth_cstr(&DEFS, idx)) }
    }
    let base = u8::from(r.kind != 0) * 4;
    unsafe {
        modal_mut().show_confirm(
            r.tag,
            pick_str(r.title, r.t_len, base),
            pick_str(r.msg, r.m_len, base + 1),
            pick_str(r.ok, r.o_len, base + 2),
            pick_str(r.cancel, r.c_len, base + 3),
        );
    }
}

#[inline(never)]
unsafe fn draw_menu_item_widget(canvas: &mut gfx::Canvas, d: &KrxWidget, s0: &str) {
    use gfx::widgets::*;
    let row_h = if d.h >= 12 { 13 } else { 10 };
    let s1 = unsafe { parse_str(d.p1, d.len1 as usize) }.unwrap_or("");
    let phase = ((d.extra >> 1) & 3) as i8;
    let chevron_dx = if (d.a & 1) != 0 && d.extra != 0 {
        if phase == 3 { 1 } else { phase }
    } else {
        0
    };
    let mut spec = ListRowSpec::new(2, d.y, 120, row_h, s0);
    spec.flags = d.a;
    spec.value = s1;
    spec.icon = if d.b != 0 {
        super::display::system_icon_8(d.b)
    } else {
        None
    };
    spec.chevron_dx = chevron_dx;
    draw_list_row(canvas, &spec);
}

#[inline(never)]
unsafe fn draw_widget(kind: u8, desc: *const KrxWidget) -> usize {
    if desc.is_null() {
        return 1;
    }
    let d = unsafe { &*desc };
    let canvas = active_canvas_mut();
    let ux = d.x.max(0) as u8;
    let uy = d.y.max(0) as u8;
    let raw0 = unsafe { slice_or_empty(d.p0, d.len0 as usize) };
    let s0 = utils::format::ascii_str(raw0).unwrap_or("");
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
        KRX_WIDGET_MENU_ITEM => unsafe { draw_menu_item_widget(canvas, d, s0) },
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
                        let m = &*core::ptr::addr_of!(MODAL_TEXT);
                        let copy_len = (m.len as usize).min(max_len as usize);
                        core::ptr::copy_nonoverlapping(m.buf.as_ptr(), buf, copy_len);
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
