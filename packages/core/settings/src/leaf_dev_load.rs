//! P0 Leaf 8: Load Device Identity (`/pref/system.cfg`) & Active Asset Names into `SharedState`.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry};
use shared::{
    SharedState, byte_at, copy_bytes, cstr_slice, fill_bytes, set_byte, subslice, subslice_mut,
};

fn short_name(path: &[u8], active: bool, out: &mut [u8; 16]) {
    fill_bytes(out, 0);
    if !active || path.is_empty() {
        copy_bytes(out, 0, b"Off");
        return;
    }
    let mut s = path.len();
    while s > 0 && byte_at(path, s - 1) != b'/' {
        s -= 1;
    }
    let tail = subslice(path, s, path.len());
    let mut dot = tail.len();
    while dot > 0 && byte_at(tail, dot - 1) != b'.' {
        dot -= 1;
    }
    let base = if dot > 0 { subslice(tail, 0, dot - 1) } else { tail };
    let mut d = 0usize;
    while d < base.len() && byte_at(base, d) >= b'0' && byte_at(base, d) <= b'9' {
        d += 1;
    }
    let clean = if d > 0 && d < base.len() && (byte_at(base, d) == b'_' || byte_at(base, d) == b'-') {
        subslice(base, d + 1, base.len())
    } else {
        base
    };
    let mut o = 0usize;
    let mut cap = true;
    let mut i = 0usize;
    while i < clean.len() && o < 15 {
        let b = byte_at(clean, i);
        let ch = if b == b'_' || b == b'-' {
            cap = true;
            b' '
        } else if cap && b >= b'a' && b <= b'z' {
            cap = false;
            b - 32
        } else {
            cap = false;
            b
        };
        set_byte(out, o, ch);
        o += 1;
        i += 1;
    }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let mut raw = MaybeUninit::<[u8; 80]>::uninit();
    let buf = unsafe { &mut *raw.as_mut_ptr() };

    if api.read_file(b"/pref/system.cfg", buf) >= 80 {
        st.dev_model_idx = byte_at(buf, 1);
        st.dev_radio_id = u32::from_le_bytes([
            byte_at(buf, 4),
            byte_at(buf, 5),
            byte_at(buf, 6),
            byte_at(buf, 7),
        ]);
        copy_bytes(&mut st.dev_owner, 0, subslice(buf, 8, 20));
        copy_bytes(&mut st.dev_tag, 0, subslice(buf, 20, 28));
        copy_bytes(&mut st.dev_model, 0, subslice(buf, 28, 40));
    }
    if api.read_file(b"/pref/display.cfg", subslice_mut(buf, 0, 48)) >= 48 {
        short_name(cstr_slice(subslice(buf, 12, 44)), byte_at(buf, 7) != 0, &mut st.splash_name);
    }
    if api.read_file(b"/pref/audio.cfg", buf) >= 80 {
        short_name(cstr_slice(subslice(buf, 9, 41)), byte_at(buf, 4) != 0, &mut st.roger_name);
        short_name(cstr_slice(subslice(buf, 41, 73)), byte_at(buf, 7) != 0, &mut st.jingle_name);
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
