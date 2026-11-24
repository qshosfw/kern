//! P0 Leaf 24: Live System Info (`Op::SysInfo`) Renderer.
//! Zero stack buffers (uses `st.sheet_buf` & `st.fmt_buf[20]`).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext, KrxSysInfo,
    KrxWidget, Op, format_u16_into, krx_entry,
};
use shared::{SharedState, cstr_slice, subslice};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    let info = unsafe { &mut *(st.sheet_buf.as_mut_ptr() as *mut KrxSysInfo) };
    let has_info = api.sys_info(info);

    let sy = st.scroll_y;
    let sel = st.info_sel;
    let mut idx = 0u8;
    while idx < 5 {
        let y = 9 + (idx as i16) * 13 - sy;
        if y > -13 && y < 64 {
            let mut neg = false;
            let mut direct: &[u8] = b"";
            let (lbl, num, dec, sfx): (&[u8], u16, u8, &[u8]) = match idx {
                0 => {
                    let m = if has_info { cstr_slice(&info.mcu) } else { b"" };
                    direct = if m.is_empty() { b"PY32F071" } else { m };
                    (b"MCU", 0, 0, b"")
                }
                1 => (b"CPU Clock", (((api.cpu_hz() >> 12) * 1074) >> 18) as u16, 0, b"MHz"),
                2 => (b"Free Stack", api.stack_free() as u16, 0, b"B"),
                3 => (b"Battery", (((api.battery_mv() as u32) * 52_429) >> 19) as u16, 2, b"V"),
                _ => {
                    let t = api.mcu_temp_deci_c();
                    neg = t < 0;
                    (b"Core Temp", if neg { (-t) as u16 } else { t as u16 }, 1, b"C")
                }
            };
            let val: &[u8] = if !direct.is_empty() {
                direct
            } else {
                let off = usize::from(neg);
                if neg {
                    st.fmt_buf[0] = b'-';
                }
                let dst = unsafe { &mut *(st.fmt_buf.as_mut_ptr().add(off) as *mut [u8; 16]) };
                let len = format_u16_into(num, dec, sfx, dst).len();
                subslice(&st.fmt_buf, 0, off + len)
            };
            let desc = KrxWidget {
                p0: lbl.as_ptr(),
                p1: val.as_ptr(),
                x: 2,
                y,
                len0: lbl.len() as u16,
                len1: val.len() as u16,
                extra: 0,
                w: 120,
                h: 13,
                a: u8::from(idx == sel),
                b: 0,
                _pad: [0; 2],
            };
            let _ = api.call_op(
                Op::UiWidget,
                KRX_WIDGET_MENU_ITEM as usize,
                &raw const desc as usize,
                0,
            );
        }
        idx += 1;
    }
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, 65, 52);
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
