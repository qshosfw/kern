//! P0 Leaf 12: Pure-RAM Installed Apps & System Modules List Renderer (Zero SPI Flash I/O).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext, KrxWidget,
    Op, format_u16_into, krx_entry,
};
use shared::{SharedState, cstr_slice};

#[inline(always)]
fn draw_row(api: &KrxApiTable, y: i16, lbl: &[u8], val: &[u8], icon_id: u8, flags: u8, anim_tick: u16) {
    let desc = KrxWidget {
        p0: lbl.as_ptr(), p1: val.as_ptr(), x: 2, y,
        len0: lbl.len() as u16, len1: val.len() as u16, extra: anim_tick,
        w: 120, h: 13, a: flags, b: icon_id, _pad: [0; 2],
    };
    let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let sy = st.scroll_y;
    let app_count = st.app_count.min(9);
    let app_sel = st.app_sel;
    let mod_only = st.modules_only != 0;
    let anim_tick = st.anim_tick;

    if app_count == 0 {
        let lbl: &[u8] = if mod_only { b"No services" } else { b"No apps" };
        draw_row(api, 9, lbl, b"/apps", 0, 0, 0);
    } else {
        let sz_buf = unsafe { &mut *(st.fmt_buf.as_mut_ptr() as *mut [u8; 16]) };
        let cache = unsafe { &*(&st.detail_sc.apps as *const shared::AppListCache) };
        let mut idx = 0u8;
        while idx < app_count {
            let y = 9 + (idx as i16) * 13 - sy;
            if y > -13 && y < 64 {
                let i = (idx as usize).min(8);
                let flags = if idx == app_sel { 3u8 } else { 0u8 };
                let lbl = cstr_slice(unsafe { cache.names.get_unchecked(i) });
                let val: &[u8] = if mod_only {
                    match unsafe { *cache.types.get_unchecked(i) } {
                        2 => b"SVC",
                        3 => b"DMN",
                        4 => b"DRV",
                        _ => b"MOD",
                    }
                } else {
                    let enc = unsafe { *cache.sizes.get_unchecked(i) };
                    if (enc & 0x8000) != 0 {
                        format_u16_into(enc & 0x7FFF, 1, b"K", sz_buf)
                    } else {
                        format_u16_into(enc, 0, b"B", sz_buf)
                    }
                };
                draw_row(api, y, lbl, val, unsafe { *cache.icons.get_unchecked(i) }, flags, anim_tick);
            }
            idx += 1;
        }
    }
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    if app_count > 4 {
        api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, (app_count as u16) * 13, 52);
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
