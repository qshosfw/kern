//! P0 Leaf 17: Bandplans Viewer & TX-Permit Toggler (`/user/bandplans.bnd`, 16B `BNDP` + 32B records).
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext, KrxWidget,
    Op, icon, krx_entry,
};
use shared::{
    CMD_BP_LOAD, CMD_BP_TOGGLE_TX, SharedState, byte_at, cstr_slice, set_byte, subslice,
    subslice_mut,
};

const MAX_BP: usize = 8;
const BP_PATH: &[u8] = b"/user/bandplans.bnd";

#[inline(always)]
fn is_valid_hdr(buf: &[u8], n: usize) -> bool {
    n >= 16
        && byte_at(buf, 0) == b'B'
        && byte_at(buf, 1) == b'N'
        && byte_at(buf, 2) == b'D'
        && byte_at(buf, 3) == b'P'
}

#[inline(never)]
fn draw_row(api: &KrxApiTable, y: i16, l: &[u8], v: &[u8], flags: u8, ic: u8) {
    let desc = KrxWidget {
        p0: l.as_ptr(), p1: if v.is_empty() { core::ptr::null() } else { v.as_ptr() },
        x: 2, y, len0: l.len() as u16, len1: v.len() as u16, extra: 0,
        w: 120, h: 13, a: flags, b: ic, _pad: [0; 2],
    };
    let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
}

#[inline(never)]
fn toggle_tx(api: &KrxApiTable, sel: u8) {
    let mut raw = MaybeUninit::<[u8; 16 + MAX_BP * 32]>::uninit();
    let buf = unsafe { &mut *raw.as_mut_ptr() };
    let n = api.read_file(BP_PATH, buf);
    if !is_valid_hdr(buf, n) || n < 48 {
        return;
    }
    let max_recs = ((n - 16) >> 5).min(MAX_BP);
    let mut seen = 0u8;
    let mut i = 0usize;
    while i < max_recs {
        let off = 16 + (i << 5);
        let fl = byte_at(buf, off + 18);
        if (fl & 1) != 0 {
            if seen == sel {
                set_byte(buf, off + 18, fl ^ 2);
                let _ = api.write_file(BP_PATH, subslice(buf, 0, n));
                let msg: &[u8] = if (fl & 2) == 0 { b"TX Enabled" } else { b"RX Only" };
                api.show_toast(msg, 900);
                break;
            }
            seen += 1;
        }
        i += 1;
    }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    if st.cmd == CMD_BP_TOGGLE_TX {
        toggle_tx(api, st.bp_sel);
        ctx.continuation.kind = KRX_LEAF_KIND_DONE;
        return 0;
    }

    // Stream-read 16B header + 32B records one-by-one into `st.sheet_buf[..32]` (zero large stack frame).
    let rec = unsafe { core::slice::from_raw_parts_mut(st.sheet_buf.as_mut_ptr(), 32) };
    let hn = api.read_file_at(BP_PATH, 0, subslice_mut(rec, 0, 16));
    let max_recs = if is_valid_hdr(rec, hn) { MAX_BP } else { 0 };

    if st.cmd == CMD_BP_LOAD {
        let mut cnt = 0u8;
        let mut i = 0usize;
        while i < max_recs {
            if api.read_file_at(BP_PATH, (16 + (i << 5)) as u32, rec) >= 32 && (byte_at(rec, 18) & 1) != 0 {
                cnt += 1;
            }
            i += 1;
        }
        st.bp_count = cnt;
        if st.bp_sel >= cnt {
            st.bp_sel = 0;
        }
    } else {
        let sy = st.scroll_y;
        let bp_count = st.bp_count;
        let bp_sel = st.bp_sel;
        if bp_count == 0 {
            draw_row(api, 9, b"No bandplans", b".bnd", 0, icon::DEVICE);
        } else {
            let mut row = 0u8;
            let mut i = 0usize;
            while i < max_recs {
                if api.read_file_at(BP_PATH, (16 + (i << 5)) as u32, rec) >= 32 {
                    let fl = byte_at(rec, 18);
                    if (fl & 1) != 0 {
                        let y = 9 + (row as i16) * 13 - sy;
                        if y > -13 && y < 64 {
                            let name = cstr_slice(subslice(rec, 24, 32));
                            let lbl = if name.is_empty() { b"Band" } else { name };
                            let tx = (fl & 2) != 0;
                            let wf = u8::from(row == bp_sel) | 8 | if tx { 16 } else { 0 };
                            draw_row(api, y, lbl, b"", wf, 0);
                        }
                        row += 1;
                    }
                }
                i += 1;
            }
        }
        api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
        if bp_count > 4 {
            api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, (bp_count as u16) * 13, 52);
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
