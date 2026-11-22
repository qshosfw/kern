//! P0 Leaf 12: Render `/apps/` Installed Apps & System Modules List.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxDirEntry,
    KrxLeafContext, KrxWidget, Op, krx_entry,
};
use shared::{
    SharedState, byte_at, copy_bytes, cstr_slice, is_krx_module, strip_apps_prefix, subslice,
};

#[inline(never)]
fn fmt_kb(api: &KrxApiTable, sz: u32, out: &mut [u8; 8]) -> usize {
    let kb = sz >= 1024;
    let mut v = if kb { (sz >> 10) * 10 + (((sz & 1023) * 10) >> 10) } else { sz };
    let mut tmp = [0u8; 6];
    let mut n = 0usize;
    while n < 6 {
        let mut r = 0u32;
        v = api.call_op(Op::SysDiv, v as usize, 10, &raw mut r as usize) as u32;
        unsafe { *tmp.get_unchecked_mut(n) = b'0' + (r as u8) };
        n += 1;
        if v == 0 && (!kb || n >= 2) {
            break;
        }
    }
    let mut o = 0usize;
    while n > 0 {
        n -= 1;
        if kb && n == 0 {
            unsafe { *out.get_unchecked_mut(o) = b'.' };
            o += 1;
        }
        unsafe { *out.get_unchecked_mut(o) = *tmp.get_unchecked(n) };
        o += 1;
    }
    unsafe { *out.get_unchecked_mut(o) = if kb { b'K' } else { b'B' } };
    o + 1
}

#[inline(always)]
fn draw_row(api: &KrxApiTable, y: i16, lbl: &[u8], val: &[u8], flags: u8, anim_tick: u16) {
    let desc = KrxWidget {
        p0: lbl.as_ptr(), p1: val.as_ptr(), x: 2, y,
        len0: lbl.len() as u16, len1: val.len() as u16, extra: anim_tick,
        w: 120, h: 13, a: flags, b: 0, _pad: [0; 2],
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
    let app_count = st.app_count;
    let app_sel = st.app_sel;
    let mod_only = st.modules_only != 0;
    let anim_tick = st.anim_tick;

    // Partition `st.sheet_buf[0..128]` (4B aligned): [0..32]=KrxDirEntry, [32..64]=pbuf, [64..80]=sz_buf, [32..128]=hdr scratch
    let base = st.sheet_buf.as_mut_ptr();
    if app_count == 0 {
        let lbl: &[u8] = if mod_only { b"No .mod files" } else { b"No packages" };
        draw_row(api, 9, lbl, b"/apps", 0, 0);
    } else {
        let e = unsafe { &mut *(base as *mut KrxDirEntry) };
        let mut off = 0u32;
        let mut idx = 0u8;
        while idx < app_count && api.list_dir_page(b"/apps", off, core::slice::from_mut(e)) > 0 {
            off += 1;
            let short = strip_apps_prefix(cstr_slice(&e.name));
            if mod_only {
                let pbuf = unsafe { core::slice::from_raw_parts_mut(base.add(96), 32) };
                let pos = copy_bytes(pbuf, 0, b"/apps/");
                let pos = copy_bytes(pbuf, pos, short);
                let hbuf = unsafe { core::slice::from_raw_parts_mut(base.add(32), 64) };
                if !is_krx_module(api, subslice(pbuf, 0, pos), hbuf) {
                    continue;
                }
            }
            let y = 9 + (idx as i16) * 13 - sy;
            if y > -13 && y < 64 {
                let flags = if idx == app_sel { 3u8 } else { 0u8 };
                let (lbl, val): (&[u8], &[u8]) = if mod_only {
                    let n = short.len();
                    let l = if n > 4 && byte_at(short, n - 4) == b'.' { subslice(short, 0, n - 4) } else { short };
                    (l, b"")
                } else {
                    let sz_buf = unsafe { &mut *(base.add(32) as *mut [u8; 8]) };
                    let sl = fmt_kb(api, e.size, sz_buf);
                    (short, subslice(sz_buf, 0, sl))
                };
                draw_row(api, y, lbl, val, flags, anim_tick);
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
