//! P0 Leaf 4: Load `/pref/keys.cfg` & Render Buttons -> Shortcuts List (Scope, F-Drawer, 19 Slots).
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{
    COLOR_CLEAR, KRX_LEAF_KIND_DONE, KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxLeafContext, KrxWidget,
    Op, icon, krx_entry, nth_packed_slice,
};
use shared::{
    CMD_SC_LOAD, MAX_APPS, SharedState, byte_at, copy_bytes, cstr_slice, hash_app_path,
    strip_apps_prefix, subslice,
};

const SLOT_LABELS: &[u8] =
    b"S1\0S1+\0S2\0S2+\0M+\0F+1\0F+2\0F+3\0F+4\0F+5\0F+6\0F+7\0F+8\0F+9\0F+0\0F+*\0F+M\0F+S1\0F+S2\0";
const CTX_NAMES: &[u8] = b"Global\0VFO\0Home\0Settings\0Apps\0";
const BUILTIN_NAMES: &[u8] = b"None\0Flashlight\0Settings\0Lock Keypad\0Home\0Sleep\0";
const BUILTIN_ICONS: [u8; 6] = [0, icon::DISPLAY, icon::APP_SETTINGS, icon::SYSTEM, icon::DEVICE, icon::BATTERY];

fn get_slot_action(cfg: &[u8; 64], slot: u8, scope: u8) -> u16 {
    if scope != 0 {
        let mut i = 0usize;
        while i < 3 {
            let o = 48 + i * 4;
            if byte_at(cfg, o) == slot && byte_at(cfg, o + 1) == scope {
                return (byte_at(cfg, o + 2) as u16) | ((byte_at(cfg, o + 3) as u16) << 8);
            }
            i += 1;
        }
    }
    let o = 10 + (slot as usize) * 2;
    (byte_at(cfg, o) as u16) | ((byte_at(cfg, o + 1) as u16) << 8)
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let sy = st.scroll_y;
    let mut raw_p = MaybeUninit::<[u8; 32]>::uninit();
    let p = unsafe { &mut *raw_p.as_mut_ptr() };
    let mut row = 0u8;
    while row < 21 {
        let y = 9 + (row as i16) * 13 - sy;
        if y > -13 && y < 64 {
            let mut fl = u8::from(row == st.sc_sel);
            let mut ic = 0u8;
            let (l, v): (&[u8], &[u8]) = if row == 0 {
                (b"Scope", nth_packed_slice(CTX_NAMES, st.sc_scope))
            } else if row == 1 {
                fl |= 8 | if byte_at(&st.sc().keys_cfg, 8) != 0 { 16 } else { 0 };
                (b"Show Drawer", b"")
            } else {
                let slot = row - 2;
                let act = get_slot_action(&st.sc().keys_cfg, slot, st.sc_scope);
                let lbl: &[u8] = if act <= 5 {
                    ic = unsafe { *BUILTIN_ICONS.get_unchecked(act as usize) };
                    nth_packed_slice(BUILTIN_NAMES, act as u8)
                } else {
                    ic = icon::APPS;
                    let mut found: &[u8] = b"App";
                    let mut i = 0usize;
                    let n = (st.app_count as usize).min(MAX_APPS);
                    while i < n {
                        let short = strip_apps_prefix(cstr_slice(unsafe { &st.apps().get_unchecked(i).name }));
                        let pl = copy_bytes(p, 0, b"/apps/");
                        let pl = copy_bytes(p, pl, short);
                        if hash_app_path(subslice(p, 0, pl)) == act {
                            found = short;
                            break;
                        }
                        i += 1;
                    }
                    found
                };
                (nth_packed_slice(SLOT_LABELS, slot), lbl)
            };
            let desc = KrxWidget {
                p0: l.as_ptr(), p1: v.as_ptr(), x: 2, y,
                len0: l.len() as u16, len1: v.len() as u16, extra: 0,
                w: 120, h: 13, a: fl, b: ic, _pad: [0; 2],
            };
            let _ = api.call_op(Op::UiWidget, KRX_WIDGET_MENU_ITEM as usize, &raw const desc as usize, 0);
        }
        row += 1;
    }
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, 21 * 13, 52);
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
