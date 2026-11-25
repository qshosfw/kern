//! P0 Leaf 12: Pure-RAM Packages & Services List Renderer (Zero SPI Flash I/O).
//!
//! Renders Apps first, then a dashed separator with 1px top/bottom padding, then all other
//! package types (Services, Daemons, Drivers, Modules).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    COLOR_CLEAR, COLOR_FILL, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext,
    KrxWidget, Op, format_u16_into, krx_entry,
};
use shared::{APPS_MAX, SharedState, cstr_slice};

#[inline(always)]
fn draw_row(api: &KrxApiTable, y: i16, lbl: &[u8], val: &[u8], icon_id: u8, flags: u8, anim_tick: u16) {
    let desc = KrxWidget {
        p0: lbl.as_ptr(), p1: val.as_ptr(), x: 2, y,
        len0: lbl.len() as u16, len1: val.len() as u16, extra: anim_tick,
        w: 120, h: 13, a: flags, b: icon_id, _pad: [0; 2],
    };
    let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
}

#[inline(never)]
fn draw_dashed_sep(api: &KrxApiTable, y: i16) {
    if y >= 9 && y < 63 {
        let mut x = 4i16;
        while x < 120 {
            api.fill_rect(x, y, 2, 1, COLOR_FILL);
            x += 4;
        }
    }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let sy = st.scroll_y;
    let app_count = st.app_count.min(APPS_MAX as u8);
    let app_sel = st.app_sel;
    let mod_only = st.modules_only != 0;
    let anim_tick = st.anim_tick;

    let cache = unsafe { &*(&st.detail_sc.apps as *const shared::AppListCache) };
    let apps_n = cache.apps_n;
    let has_sep = !mod_only && apps_n > 0 && apps_n < app_count;

    if app_count == 0 {
        let lbl: &[u8] = if mod_only { b"No services" } else { b"No packages" };
        draw_row(api, 9, lbl, b"/apps", 0, 0, 0);
    } else {
        let sz_buf = unsafe { &mut *(st.fmt_buf.as_mut_ptr() as *mut [u8; 16]) };
        let mut idx = 0u8;
        while idx < app_count {
            let sep_off: i16 = if has_sep && idx >= apps_n { 3 } else { 0 };
            let y = 9 + (idx as i16) * 13 + sep_off - sy;
            if y > -13 && y < 64 {
                let i = (idx as usize).min(APPS_MAX - 1);
                let flags = if idx == app_sel { 3u8 } else { 0u8 };
                let lbl = cstr_slice(unsafe { cache.names.get_unchecked(i) });
                let et = unsafe { *cache.types.get_unchecked(i) };
                let val: &[u8] = if et >= 2 {
                    match et {
                        2 => b"SVC",
                        3 => b"DMN",
                        4 => b"DRV",
                        _ => b"MOD",
                    }
                } else {
                    let enc = unsafe { *cache.sizes.get_unchecked(i) };
                    if (enc & 0x80) != 0 {
                        let tenths = ((enc & 0x7F) as u16) * 5;
                        format_u16_into(tenths, 1, b"K", sz_buf)
                    } else {
                        format_u16_into((enc as u16) << 4, 0, b"B", sz_buf)
                    }
                };
                draw_row(api, y, lbl, val, unsafe { *cache.icons.get_unchecked(i) }, flags, anim_tick);
            }
            idx += 1;
        }
        if has_sep {
            // 1px top padding + 1px dashed line + 1px bottom padding between Apps and other types.
            draw_dashed_sep(api, 9 + (apps_n as i16) * 13 - sy + 1);
        }
    }
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    let total_h = (app_count as u16) * 13 + if has_sep { 3 } else { 0 };
    if total_h > 52 {
        api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, total_h, 52);
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
