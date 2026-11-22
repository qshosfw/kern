//! P0 Leaf 6: Dynamic Asset Scanner & Native OptionSheet Opener (Splash, Roger, Jingle).
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    ASSET_LANG, ASSET_ROGER, ASSET_SPLASH, MAX_ASSETS, MODAL_ASSET_PICK, SharedState, byte_at,
    copy_bytes, cstr_slice, fill_bytes, set_byte, subslice,
};

#[inline(never)]
fn format_pretty(raw: &[u8], out: &mut [u8]) -> usize {
    let mut s = raw.len();
    while s > 0 && byte_at(raw, s - 1) != b'/' {
        s -= 1;
    }
    let tail = subslice(raw, s, raw.len());
    let mut dot = tail.len();
    while dot > 0 && byte_at(tail, dot - 1) != b'.' {
        dot -= 1;
    }
    let clean = if dot > 0 { subslice(tail, 0, dot - 1) } else { tail };
    let mut o = 0usize;
    let mut cap = true;
    let mut i = 0usize;
    while i < clean.len() && o < out.len() {
        let b = byte_at(clean, i);
        let ch = if b == b'_' || b == b'-' {
            cap = true;
            b' '
        } else {
            let c = if cap && b >= b'a' && b <= b'z' { b - 32 } else { b };
            cap = false;
            c
        };
        set_byte(out, o, ch);
        o += 1;
        i += 1;
    }
    o
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let dom = st.arg0;
    st.asset_domain = dom;
    let (dir, title, def0): (&[u8], &[u8], &[u8]) = match dom {
        ASSET_SPLASH => (b"/user/splash", b"Splash Image", b"Off\0"),
        ASSET_ROGER => (b"/user/rogers", b"Roger Beep", b"Off\0"),
        ASSET_LANG => (b"/user/lang", b"Language", b"English\0"),
        _ => (b"/user/jingles", b"Boot Jingle", b"Off\0"),
    };
    let mut raw_e = MaybeUninit::<[KrxDirEntry; MAX_ASSETS]>::uninit();
    let entries = unsafe { &mut *raw_e.as_mut_ptr() };
    let total = api.list_dir(dir, entries).min(MAX_ASSETS);

    let mut pos = copy_bytes(&mut st.sheet_buf, 0, def0);
    let mut cnt = 0usize;
    let mut i = 0usize;
    let mut raw_p = MaybeUninit::<[u8; 16]>::uninit();
    let pretty = unsafe { &mut *raw_p.as_mut_ptr() };
    while i < total && cnt < MAX_ASSETS {
        let ent = unsafe { entries.get_unchecked(i) };
        if ent.is_dir == 0 {
            let raw = cstr_slice(&ent.name);
            let dst_fp = unsafe { st.assets_mut().get_unchecked_mut(cnt) };
            fill_bytes(dst_fp, 0);
            let plen = if !raw.is_empty() && byte_at(raw, 0) == b'/' {
                copy_bytes(dst_fp, 0, raw)
            } else {
                let p = copy_bytes(dst_fp, 0, dir);
                set_byte(dst_fp, p, b'/');
                copy_bytes(dst_fp, p + 1, raw)
            };
            let nlen = format_pretty(subslice(dst_fp, 0, plen), pretty);
            if nlen > 0 && pos + nlen + 1 < 88 {
                pos = copy_bytes(&mut st.sheet_buf, pos, subslice(pretty, 0, nlen));
                set_byte(&mut st.sheet_buf, pos, 0);
                pos += 1;
                cnt += 1;
            }
        }
        i += 1;
    }
    st.asset_count = cnt as u8;
    st.sheet_len = pos as u16;
    let tlen = copy_bytes(&mut st.sheet_title, 0, title);
    api.show_sheet(MODAL_ASSET_PICK, subslice(&st.sheet_title, 0, tlen), subslice(&st.sheet_buf, 0, pos), 0, true);
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
