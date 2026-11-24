//! P0 Leaf 21: Startup Asset, Battery Tweak & Language `__LANG_ASCII__` Loader.
//! Reads `/pref/display.cfg`, `/pref/audio.cfg`, `/pref/battery.cfg`, and resolves the
//! active language pack staged in `bp_tail[0..32]` by `leaf_dev_load`. Zero stack buffers.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry};
use shared::{
    SharedState, byte_at, copy_bytes, cstr_slice, fill_bytes, format_pretty_asset,
    read_lng_ascii_name, set_byte, subslice, subslice_mut,
};

#[inline(never)]
fn load_short(path: &[u8], active: bool, out: &mut [u8; 12]) {
    fill_bytes(out, 0);
    if !active || path.is_empty() {
        copy_bytes(out, 0, b"Off");
    } else {
        let _ = format_pretty_asset(path, subslice_mut(out, 0, 11));
    }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    let buf = unsafe { core::slice::from_raw_parts_mut(st.sheet_buf.as_mut_ptr(), 80) };
    if api.read_file(b"/pref/display.cfg", subslice_mut(buf, 0, 48)) >= 48 {
        load_short(
            cstr_slice(subslice(buf, 12, 44)),
            byte_at(buf, 7) != 0,
            &mut st.str_fields[0],
        );
    }
    if api.read_file(b"/pref/audio.cfg", buf) >= 80 {
        load_short(
            cstr_slice(subslice(buf, 9, 41)),
            byte_at(buf, 4) != 0,
            &mut st.str_fields[1],
        );
        load_short(
            cstr_slice(subslice(buf, 41, 73)),
            byte_at(buf, 7) != 0,
            &mut st.str_fields[2],
        );
    }
    if api.read_file(b"/pref/battery.cfg", subslice_mut(buf, 0, 20)) >= 20 {
        let mv = i16::from_le_bytes([byte_at(buf, 6), byte_at(buf, 7)]);
        st.batt_tweak_mv = mv.clamp(-500, 500);
    }

    let scratch = unsafe { st.detail_sc.bp_tail.as_mut_ptr() };
    let lp = cstr_slice(unsafe { core::slice::from_raw_parts(scratch, 32) });
    if !lp.is_empty() {
        let s16 = unsafe { &mut *(scratch.add(32) as *mut [u8; 16]) };
        fill_bytes(&mut st.str_fields[3], 0);
        let out = subslice_mut(&mut st.str_fields[3], 0, 11);
        if read_lng_ascii_name(api, lp, s16, out) == 0 && lp.len() > 15 {
            let _ = copy_bytes(out, 0, subslice(lp, 11, lp.len() - 4));
            let c0 = byte_at(out, 0);
            if c0 >= b'a' && c0 <= b'z' {
                set_byte(out, 0, c0 - 32);
            }
        }
    }

    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
