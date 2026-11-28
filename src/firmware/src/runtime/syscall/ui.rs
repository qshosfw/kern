//! UI & OS Compositor System Call Handlers (`Op::UiModal`, `Op::UiWidget`, domain `0x7`).

use super::{active_canvas_mut, parse_str, ptr_ref, str_or_default};
use kernel::proc::abi::*;

#[inline(always)]
fn modal_mut() -> &'static mut crate::orchestrator::compositor::ModalCompositor {
    crate::orchestrator::compositor::active_modal_mut()
}

#[cfg(feature = "text_input")]
static mut MODAL_TEXT: utils::format::InlineStr<31> = utils::format::InlineStr::new();

pub fn store_modal_text(_text: &str) {
    #[cfg(feature = "text_input")]
    unsafe {
        (*core::ptr::addr_of_mut!(MODAL_TEXT)).set(_text);
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

fn is_persistent_guest_ptr(ptr: *const u8, len: usize) -> bool {
    if ptr.is_null() {
        return true;
    }
    let slot = crate::runtime::native_runner::slot_ref();
    if !slot.is_loaded || !slot.profile.is_structured_paged() {
        return true;
    }
    let base = crate::runtime::native_runner::arena_base_ptr() as usize;
    let overlay_end = base + crate::data::arena::TRANSIENT_ARENA_BYTES;
    let overlay_start = overlay_end - 1024;
    let addr = ptr as usize;
    let end = addr.saturating_add(len);
    !(addr < overlay_end && end > overlay_start)
}

unsafe fn show_modal_req(req: *const KrxModalReq) {
    let Some(r) = (unsafe { ptr_ref(req) }) else {
        return;
    };
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
                m.is_k1 = crate::services::config().is_k1();
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
        if let Some(raw) = unsafe { super::ptr_slice(r.msg, opts_len) }
            && raw.iter().all(|&b| b == 0 || (0x20..=0x7E).contains(&b))
        {
            let opts = unsafe { core::str::from_utf8_unchecked(raw) };
            let sheet = gfx::views::OptionSheet::new(
                t_str,
                gfx::views::SheetOptions::Packed(opts),
                r.max_len as usize,
            )
            .with_radio(r.c_len != 0);
            modal_mut().show_sheet(r.tag, sheet);
        }
        return;
    }
    utils::static_bytes!(
        static DEFS = b"Notice\0\0\0\0Confirm\0\0OK\0CANCEL\0";
    );
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

unsafe fn widget_str(p: *const u8, len: u16) -> &'static str {
    unsafe { parse_str(p, len as usize) }.unwrap_or("")
}

unsafe fn draw_menu_item_widget(canvas: &mut gfx::Canvas, d: &KrxWidget) {
    use gfx::elements::*;
    let row_h = if d.h >= 12 { 13 } else { 10 };
    let s0 = unsafe { widget_str(d.p0, d.len0) };
    let s1 = unsafe { widget_str(d.p1, d.len1) };
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

pub unsafe fn op_ui_widget(a0: usize, a1: usize, _a2: usize) -> usize {
    if a0 as u8 != KRX_WIDGET_MENU_ITEM {
        return ::abi::Status::Unsupported.to_raw();
    }
    let Some(d) = (unsafe { ptr_ref(a1 as *const KrxWidget) }) else {
        return ::abi::Status::InvalidArg.to_raw();
    };
    unsafe { draw_menu_item_widget(active_canvas_mut(), d) };
    0
}

pub unsafe fn op_ui_modal(a0: usize, a1: usize, a2: usize) -> usize {
    use ::abi::sub::ui_modal;
    match a0 as u8 {
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
        ui_modal::PROMPT_NUM => {
            let Some(r) = (unsafe { ptr_ref(a1 as *const ::abi::KrxNumReq) }) else {
                return ::abi::Status::InvalidArg.to_raw();
            };
            let title = unsafe { str_or_default(r.title, r.title_len as usize, "Value") };
            let unit = unsafe { str_or_default(r.unit, r.unit_len as usize, "") };
            modal_mut().prompt_number(r.tag, title, unit, r);
            0
        }
        ui_modal::GET_TEXT => {
            #[cfg(feature = "text_input")]
            {
                let max_len = a2 as u8 as usize;
                let Some(dst) = (unsafe { super::ptr_slice_mut(a1 as *mut u8, max_len) }) else {
                    return 0;
                };
                let m = unsafe { &*core::ptr::addr_of!(MODAL_TEXT) };
                let copy_len = m.len().min(max_len);
                dst[..copy_len].copy_from_slice(&m.buf[..copy_len]);
                copy_len
            }
            #[cfg(not(feature = "text_input"))]
            {
                let _ = (a1, a2);
                0
            }
        }
        ui_modal::SET_BAR => {
            if a1 == 0 || super::guest_range_valid(a1, 64, false) {
                crate::runtime::native_runner::set_slot_status_bar(a1 as *const KrxStatusBar);
                1
            } else {
                0
            }
        }
        ui_modal::GET_BAR => {
            if super::guest_range_valid(a1, 64, true) {
                crate::runtime::native_runner::get_slot_status_bar(a1 as *mut KrxStatusBar);
            }
            0
        }
        _ => ::abi::Status::Unsupported.to_raw(),
    }
}
