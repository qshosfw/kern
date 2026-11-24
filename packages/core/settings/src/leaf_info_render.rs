//! P0 Leaf 14: Live Battery Info Renderer (Voltage, Percentage, Current, Status, Core Temp).
//! Zero stack buffers (uses `st.fmt_buf[20]`).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext, KrxWidget,
    Op, format_u16_into, krx_entry,
};
use shared::{SharedState, subslice};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    let sy = st.scroll_y;
    let sel = st.info_sel;
    let mut idx = 0u8;
    while idx < 5 {
        let y = 9 + (idx as i16) * 13 - sy;
        if y > -13 && y < 64 {
            let mut neg = false;
            let mut direct: &[u8] = b"";
            let (lbl, num, dec, sfx): (&[u8], u16, u8, &[u8]) = match idx {
                0 => (b"Voltage", (((api.battery_mv() as u32) * 52_429) >> 19) as u16, 2, b"V"),
                1 => (b"Percentage", api.battery_pct() as u16, 0, b"%"),
                2 => (b"Current", api.battery_ma(), 0, b"mA"),
                3 => {
                    direct = if api.is_charging() { b"Charging" } else { b"Discharging" };
                    (b"Status", 0, 0, b"")
                }
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
