//! P0 Leaf 28: Pure-RAM Category & Submenu List Renderer (100% cache-hit while scrolling).
//!
//! Reads resident `ITEMS` via `st.items_ptr`, hydrated labels from `bp_tail[0..104]`,
//! and active choice values from `bp_tail[104..144]` — zero SPI Flash I/O during scrolling.
#![no_std]
#![no_main]

mod schema;
mod shared;
mod ui;

use kern_sdk::{
    KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, SEC_FLAG_IS_SET, krx_entry, nth_packed_slice,
};
use schema::{
    ACT_ASSET_SPLASH, ACT_DEV_RADIO_ID, ACT_SEC_DIGITS, ACT_SEC_EXPOSE, ACT_SEC_MAX_TRIES,
    ACT_SEC_PASSCODE, ACT_SUB_BATT_TWEAKS, ACT_SYS_STORAGE, Item, Kind, PROP_BATT_TWEAK, UNIT_MV,
};
use shared::{SharedState, cstr_slice, subslice};

/// Ultra-compact unsigned/signed integer formatter without division or bounds panics.
#[inline(never)]
fn fmt_num<'a>(val: i16, unit: u8, buf: &'a mut [u8; 16]) -> &'a [u8] {
    let p = buf.as_mut_ptr();
    let mut n = 0usize;
    let mut mag: u16 = if val < 0 {
        unsafe { *p = b'-' };
        n = 1;
        (-val) as u16
    } else {
        if unit == UNIT_MV && val > 0 {
            unsafe { *p = b'+' };
            n = 1;
        }
        val as u16
    };
    let mut started = false;
    let mut d = 100u16;
    loop {
        let mut q = 0u8;
        while mag >= d {
            mag -= d;
            q += 1;
        }
        if q != 0 || started || d == 1 {
            unsafe { *p.add(n) = b'0' + q };
            n += 1;
            started = true;
        }
        if d == 100 {
            d = 10;
        } else if d == 10 {
            d = 1;
        } else {
            break;
        }
    }
    let suf = nth_packed_slice(b"\0%\0dB\0mV\0", unit);
    let mut j = 0usize;
    while j < suf.len() && n < 16 {
        unsafe { *p.add(n) = *suf.get_unchecked(j) };
        n += 1;
        j += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    let items = st.items_ptr as *const Item;
    if items.is_null() {
        ctx.continuation.kind = KRX_LEAF_KIND_DONE;
        return 0;
    }
    let start = st.cat_start as usize;
    let count = st.cat_count;
    let sel = st.arg0;
    let edit_flags = st.cat_editing;
    let sy = st.scroll_y;
    let anim_tick = st.anim_tick;
    let sec = st.sec_status;
    let tail = unsafe { &st.detail_sc.bp_tail };

    let mut ch_ord = 0u8;
    let mut idx = 0u8;
    while idx < count {
        let item = unsafe { *items.add((start + idx as usize).min(58)) };
        let is_choice = matches!(item.kind, Kind::Choice);
        let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - sy;
        if y > -ui::ROW_STEP_Y && y < 64 {
            let is_sel = idx == sel;
            let mut flags = u8::from(is_sel);
            let fbuf = unsafe { &mut *(st.fmt_buf.as_mut_ptr() as *mut [u8; 16]) };
            let vstr: &[u8] = match item.kind {
                Kind::Choice => nth_packed_slice(subslice(tail, 104, 144), ch_ord),
                Kind::Toggle => {
                    let on = api.prop_get(item.prop_id) != 0;
                    flags |= ui::FLAG_TOGGLE | if on { ui::FLAG_TOGGLE_ON } else { 0 };
                    b""
                }
                Kind::Num => {
                    let val: i16 = if item.prop_id == PROP_BATT_TWEAK {
                        st.batt_tweak_mv
                    } else {
                        api.prop_get(item.prop_id) as i16
                    };
                    if is_sel {
                        flags |= edit_flags;
                    }
                    fmt_num(val, item.meta, fbuf)
                }
                Kind::Action => {
                    let m = item.meta;
                    if m >= ACT_SEC_EXPOSE && m <= ACT_SEC_DIGITS {
                        let on = (sec & (1u32 << (m - ACT_SEC_PASSCODE))) != 0;
                        flags |= ui::FLAG_TOGGLE | if on { ui::FLAG_TOGGLE_ON } else { 0 };
                    } else if is_sel && (m <= ACT_SUB_BATT_TWEAKS || m >= ACT_SYS_STORAGE) {
                        flags |= ui::FLAG_CHEVRON;
                    }
                    if m >= ACT_ASSET_SPLASH && m <= ACT_DEV_RADIO_ID {
                        cstr_slice(unsafe {
                            st.str_fields.get_unchecked((m - ACT_ASSET_SPLASH) as usize)
                        })
                    } else if m == ACT_SEC_MAX_TRIES {
                        fmt_num(((sec >> 16) & 0xFF) as i16, 0, fbuf)
                    } else if m == ACT_SEC_PASSCODE {
                        if (sec & SEC_FLAG_IS_SET) != 0 { b"Set" } else { b"None" }
                    } else {
                        b""
                    }
                }
            };
            let lbl = nth_packed_slice(subslice(tail, 0, 104), idx);
            ui::draw_row(api, y, lbl, vstr, (flags as u16) << 8, anim_tick);
        }
        if is_choice {
            ch_ord += 1;
        }
        idx += 1;
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
