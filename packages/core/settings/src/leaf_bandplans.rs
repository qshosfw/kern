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
const BP_FILE_BUF: usize = 16 + MAX_BP * 32; // 272 B

#[inline(always)]
fn is_valid_bnd(buf: &[u8], n: usize) -> bool {
    n >= 48
        && byte_at(buf, 0) == b'B'
        && byte_at(buf, 1) == b'N'
        && byte_at(buf, 2) == b'D'
        && byte_at(buf, 3) == b'P'
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let mut raw = MaybeUninit::<[u8; BP_FILE_BUF]>::uninit();
    let buf = unsafe { &mut *raw.as_mut_ptr() };
    let n = api.read_file(b"/user/bandplans.bnd", buf);
    let valid = is_valid_bnd(buf, n);
    let max_recs = if valid { ((n - 16) >> 5).min(MAX_BP) } else { 0 };

    if st.cmd == CMD_BP_LOAD {
        let mut cnt = 0u8;
        let mut i = 0usize;
        while i < max_recs {
            if (byte_at(buf, 16 + (i << 5) + 18) & 1) != 0 {
                cnt += 1;
            }
            i += 1;
        }
        st.bp_count = cnt;
        if st.bp_sel >= cnt {
            st.bp_sel = 0;
        }
    } else if st.cmd == CMD_BP_TOGGLE_TX && valid {
        let mut seen = 0u8;
        let mut i = 0usize;
        while i < max_recs {
            let off = 16 + (i << 5);
            let fl = byte_at(buf, off + 18);
            if (fl & 1) != 0 {
                if seen == st.bp_sel {
                    set_byte(buf, off + 18, fl ^ 2);
                    let _ = api.write_file(b"/user/bandplans.bnd", subslice(buf, 0, n));
                    let msg: &[u8] = if (fl & 2) == 0 { b"TX Enabled" } else { b"RX Only" };
                    api.show_toast(msg, 900);
                    break;
                }
                seen += 1;
            }
            i += 1;
        }
    } else {
        let sy = st.scroll_y;
        if st.bp_count == 0 {
            let l = b"No bandplans";
            let v = b".bnd";
            let desc = KrxWidget {
                p0: l.as_ptr(), p1: v.as_ptr(), x: 2, y: 9,
                len0: l.len() as u16, len1: v.len() as u16, extra: 0,
                w: 120, h: 13, a: 0, b: icon::DEVICE, _pad: [0; 2],
            };
            let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
        } else {
            let mut row = 0u8;
            let mut i = 0usize;
            while i < max_recs {
                let off = 16 + (i << 5);
                let fl = byte_at(buf, off + 18);
                if (fl & 1) != 0 {
                    let y = 9 + (row as i16) * 13 - sy;
                    if y > -13 && y < 64 {
                        let name = cstr_slice(subslice(buf, off + 24, off + 32));
                        let lbl = if name.is_empty() { b"Band" } else { name };
                        let tx = (fl & 2) != 0;
                        let wf = u8::from(row == st.bp_sel) | 8 | if tx { 16 } else { 0 };
                        let desc = KrxWidget {
                            p0: lbl.as_ptr(), p1: core::ptr::null(), x: 2, y,
                            len0: lbl.len() as u16, len1: 0, extra: 0,
                            w: 120, h: 13, a: wf, b: 0, _pad: [0; 2],
                        };
                        let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
                    }
                    row += 1;
                }
                i += 1;
            }
        }
        api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
        if st.bp_count > 4 {
            api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, (st.bp_count as u16) * 13, 52);
        }
    }
    let _ = subslice_mut(buf, 0, 0);
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
