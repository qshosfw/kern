//! P0 Leaf 4: Pure-RAM Shortcuts List Renderer (Zero SPI Flash I/O & Override `*` Marker).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext, KrxWidget,
    Op, icon, krx_entry, nth_packed_slice,
};
use shared::{
    SharedState, byte_at, copy_bytes, cstr_slice, get_slot_action_override, set_byte, subslice,
};

const SLOT_LABELS: &[u8] =
    b"S1\0S1+\0S2\0S2+\0M+\0F+1\0F+2\0F+3\0F+4\0F+5\0F+6\0F+7\0F+8\0F+9\0F+0\0F+*\0F+M\0F+S1\0F+S2\0";
const CTX_NAMES: &[u8] = b"Global\0VFO\0Home\0Settings\0Apps\0";
const BUILTIN_NAMES: &[u8] = b"None\0Flashlight\0Settings\0Lock Keypad\0Home\0Sleep\0";
const BUILTIN_ICONS: [u8; 6] = [0, icon::DISPLAY, icon::APP_SETTINGS, icon::SYSTEM, icon::DEVICE, icon::BATTERY];

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let sy = st.scroll_y;
    let sc_sel = st.sc_sel;
    let sc_scope = st.sc_scope;
    let sc = unsafe { &*core::ptr::addr_of!(st.detail_sc.sc) };
    let app_n = (sc.app_n as usize).min(6);
    let mut row = 0u8;
    let mut y = 9i16 - sy;
    while row < 21 {
        if y > -13 && y < 64 {
            let mut fl = u8::from(row == sc_sel);
            let mut ic = 0u8;
            let (l, v): (&[u8], &[u8]) = if row == 0 {
                (b"Scope", nth_packed_slice(CTX_NAMES, sc_scope))
            } else if row == 1 {
                fl |= 8 | if byte_at(&sc.keys_cfg, 8) != 0 { 16 } else { 0 };
                (b"Show Drawer", b"")
            } else {
                let slot = row - 2;
                fl |= 128; // FLAG_SHORTCUT
                let (act, overridden) = get_slot_action_override(&sc.keys_cfg, slot, sc_scope);
                let base_lbl: &[u8] = if act <= 5 {
                    ic = unsafe { *BUILTIN_ICONS.get_unchecked(act as usize) };
                    nth_packed_slice(BUILTIN_NAMES, act as u8)
                } else {
                    ic = icon::APPS;
                    let mut found: &[u8] = b"App";
                    let mut i = 0usize;
                    while i < app_n {
                        if sc.app_hashes[i] == act {
                            found = cstr_slice(&sc.app_names[i]);
                            break;
                        }
                        i += 1;
                    }
                    found
                };
                let disp = if overridden {
                    set_byte(&mut st.fmt_buf, 0, b'*');
                    let end = copy_bytes(&mut st.fmt_buf, 1, base_lbl);
                    subslice(&st.fmt_buf, 0, end)
                } else {
                    base_lbl
                };
                (nth_packed_slice(SLOT_LABELS, slot), disp)
            };
            let desc = KrxWidget {
                p0: l.as_ptr(),
                p1: v.as_ptr(),
                x: 2,
                y,
                len0: l.len() as u16,
                len1: v.len() as u16,
                extra: 0,
                w: 120,
                h: 13,
                a: fl,
                b: ic,
                _pad: [0; 2],
            };
            let _ = api.call_op(
                Op::UiWidget,
                KRX_WIDGET_MENU_ITEM as usize,
                &raw const desc as usize,
                0,
            );
        }
        y += 13;
        row += 1;
    }
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, 21 * 13, 52);
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
