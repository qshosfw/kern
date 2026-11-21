//! P0 Leaf 12: Render `/apps/` Installed Apps & System Modules List.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext, KrxWidget,
    Op, format_u16_into, krx_entry,
};
use shared::{SharedState, byte_at, cstr_slice, strip_apps_prefix, subslice};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &*(ctx.arg as *const SharedState) };
    let sy = st.scroll_y;
    let mut raw_n = MaybeUninit::<[u8; 16]>::uninit();
    let sz_buf = unsafe { &mut *raw_n.as_mut_ptr() };

    if st.app_count == 0 {
        let lbl: &[u8] = if st.modules_only != 0 { b"No .mod files" } else { b"No packages" };
        let desc = KrxWidget {
            p0: lbl.as_ptr(), p1: b"/apps".as_ptr(), x: 2, y: 9,
            len0: lbl.len() as u16, len1: 5, extra: 0, w: 120, h: 13, a: 0, b: 0, _pad: [0; 2],
        };
        let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
    } else {
        let mut idx = 0u8;
        while idx < st.app_count {
            let y = 9 + (idx as i16) * 13 - sy;
            if y > -13 && y < 64 {
                let e = unsafe { st.apps().get_unchecked(idx as usize) };
                let short = strip_apps_prefix(cstr_slice(&e.name));
                let flags = if idx == st.app_sel { 3u8 } else { 0u8 };
                let (lbl, val): (&[u8], &[u8]) = if st.modules_only != 0 {
                    let n = short.len();
                    let l = if n > 4 && byte_at(short, n - 4) == b'.' { subslice(short, 0, n - 4) } else { short };
                    (l, b"")
                } else {
                    let sz = if e.size < 1024 {
                        format_u16_into(e.size as u16, 0, b"B", sz_buf)
                    } else {
                        let kb = (e.size >> 10) as u16;
                        let t = (((e.size & 1023) * 10) >> 10) as u16;
                        format_u16_into(kb * 10 + t, 1, b"K", sz_buf)
                    };
                    (short, sz)
                };
                let desc = KrxWidget {
                    p0: lbl.as_ptr(), p1: val.as_ptr(), x: 2, y,
                    len0: lbl.len() as u16, len1: val.len() as u16, extra: st.anim_tick,
                    w: 120, h: 13, a: flags, b: 0, _pad: [0; 2],
                };
                let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
            }
            idx += 1;
        }
    }
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    if st.app_count > 4 {
        api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, (st.app_count as u16) * 13, 52);
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
