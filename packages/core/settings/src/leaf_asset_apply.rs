//! P0 Leaf 7: Persist Selected Splash / Roger / Jingle Asset Path to `/pref/*.cfg` + CRC32.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    ASSET_LANG, ASSET_ROGER, ASSET_SPLASH, SharedState, byte_at, copy_bytes, cstr_slice,
    fill_bytes, set_byte, subslice, subslice_mut,
};

fn format_pretty(raw: &[u8], out: &mut [u8]) {
    let mut s = raw.len();
    while s > 0 && byte_at(raw, s - 1) != b'/' {
        s -= 1;
    }
    let mut e = raw.len();
    while e > s && byte_at(raw, e - 1) != b'.' {
        e -= 1;
    }
    let end = if e > s { e - 1 } else { raw.len() };
    let mut d = s;
    while d < end && byte_at(raw, d).is_ascii_digit() {
        d += 1;
    }
    if d > s && d < end && (byte_at(raw, d) == b'_' || byte_at(raw, d) == b'-') {
        s = d + 1;
    }
    let mut o = 0usize;
    let mut cap = true;
    while s < end && o < out.len() {
        let b = byte_at(raw, s);
        if b == b'_' || b == b'-' {
            cap = true;
            set_byte(out, o, b' ');
        } else {
            set_byte(out, o, if cap && b.is_ascii_lowercase() { b - 32 } else { b });
            cap = false;
        }
        o += 1;
        s += 1;
    }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let pick = st.arg0 as usize;
    let dom = st.asset_domain;
    let base = st.sheet_buf.as_mut_ptr();

    let (file, dir, clen, flag_off, path_off, name_dst): (&[u8], &[u8], usize, usize, usize, &mut [u8]) =
        match dom {
            ASSET_SPLASH => (b"/pref/display.cfg", b"/user/splash", 48, 7, 12, &mut st.splash_name),
            ASSET_ROGER => (b"/pref/audio.cfg", b"/user/rogers", 80, 4, 9, &mut st.roger_name),
            ASSET_LANG => (b"/pref/system.cfg", b"/user/lang", 80, 40, 44, &mut st.lang_name),
            _ => (b"/pref/audio.cfg", b"/user/jingles", 80, 7, 41, &mut st.jingle_name),
        };

    let cfg = unsafe { core::slice::from_raw_parts_mut(base, clen) };
    if api.read_file(file, cfg) >= clen {
        fill_bytes(name_dst, 0);
        if pick == 0 {
            set_byte(cfg, flag_off, 0);
            copy_bytes(name_dst, 0, if dom == ASSET_LANG { b"English" } else { b"Off" });
        } else if pick - 1 < st.asset_count as usize {
            let target = pick - 1;
            let ent = unsafe { &mut *(base.add(80) as *mut KrxDirEntry) };
            let mut off = 0u32;
            let mut seen = 0usize;
            while api.list_dir_page(dir, off, core::slice::from_mut(ent)) > 0 {
                off += 1;
                if ent.is_dir == 0 {
                    if seen == target {
                        set_byte(cfg, flag_off, 1);
                        let dst = unsafe { core::slice::from_raw_parts_mut(base.add(path_off), 32) };
                        fill_bytes(dst, 0);
                        let raw = cstr_slice(&ent.name);
                        let p = copy_bytes(dst, 0, dir);
                        set_byte(dst, p, b'/');
                        copy_bytes(dst, p + 1, raw);
                        format_pretty(raw, name_dst);
                        break;
                    }
                    seen += 1;
                }
            }
        }
        let crc = api.crc32(unsafe { core::slice::from_raw_parts(base, clen - 4) }).to_le_bytes();
        copy_bytes(cfg, clen - 4, &crc);
        if api.write_file(file, cfg) {
            let _ = api.reload_config();
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
