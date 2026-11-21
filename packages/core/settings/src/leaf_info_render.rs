//! P0 Leaf 14: Live Battery Info & System Info Renderer.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext, KrxWidget,
    Op, format_u16_into, krx_entry, nth_packed_slice,
};
use shared::{CMD_INFO_BATT, SharedState};

const BATT_LABELS: &[u8] = b"Voltage\0Percentage\0Current\0Status\0Core Temp\0";
const SYS_LABELS: &[u8] = b"MCU\0CPU Clock\0Free Stack\0Battery\0Core Temp\0";

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &*(ctx.arg as *const SharedState) };
    let mut raw = MaybeUninit::<[u8; 16]>::uninit();
    let vbuf = unsafe { &mut *raw.as_mut_ptr() };

    let is_batt = st.cmd == CMD_INFO_BATT;
    let labels = if is_batt { BATT_LABELS } else { SYS_LABELS };
    let sy = st.scroll_y;
    let mut idx = 0u8;
    while idx < 5 {
        let y = 9 + (idx as i16) * 13 - sy;
        if y > -13 && y < 64 {
            let val: &[u8] = if idx == 4 {
                let t = api.mcu_temp_deci_c();
                format_u16_into(if t < 0 { (-t) as u16 } else { t as u16 }, 1, b"C", vbuf)
            } else if (is_batt && idx == 0) || (!is_batt && idx == 3) {
                format_u16_into((((api.battery_mv() as u32) * 52_429) >> 19) as u16, 2, b"V", vbuf)
            } else if is_batt {
                match idx {
                    1 => format_u16_into(api.battery_pct() as u16, 0, b"%", vbuf),
                    2 => format_u16_into(api.battery_ma(), 0, b"mA", vbuf),
                    _ => if api.is_charging() { b"Charging" } else { b"Discharging" },
                }
            } else {
                match idx {
                    0 => b"PY32F071",
                    1 => format_u16_into((((api.cpu_hz() >> 12) * 1074) >> 18) as u16, 0, b"MHz", vbuf),
                    _ => format_u16_into(api.stack_free() as u16, 0, b"B", vbuf),
                }
            };
            let lbl = nth_packed_slice(labels, idx);
            let desc = KrxWidget {
                p0: lbl.as_ptr(), p1: val.as_ptr(), x: 2, y,
                len0: lbl.len() as u16, len1: val.len() as u16, extra: 0,
                w: 120, h: 13, a: u8::from(idx == st.info_sel), b: 0, _pad: [0; 2],
            };
            let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
        }
        idx += 1;
    }
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, 65, 52);
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
