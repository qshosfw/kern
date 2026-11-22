//! P0 Leaf 7: Persist Selected Splash / Roger / Jingle Asset Path to `/pref/*.cfg` + CRC32.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
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
    let tail = subslice(raw, s, raw.len());
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
    let pick = st.arg0 as usize;
    let dom = st.asset_domain;
    let mut raw_c = MaybeUninit::<[u8; 80]>::uninit();
    let cfg = unsafe { &mut *raw_c.as_mut_ptr() };

    let (file, clen, flag_off, path_off): (&[u8], usize, usize, usize) = if dom == ASSET_SPLASH {
        (b"/pref/display.cfg", 48, 7, 12)
    } else if dom == ASSET_ROGER {
        (b"/pref/audio.cfg", 80, 4, 9)
    } else if dom == ASSET_LANG {
        (b"/pref/system.cfg", 80, 40, 44)
    } else {
        (b"/pref/audio.cfg", 80, 7, 41)
    };

    if api.read_file(file, subslice_mut(cfg, 0, clen)) >= clen {
        let mut pretty = [0u8; 16];
        if pick == 0 {
            set_byte(cfg, flag_off, 0);
            copy_bytes(&mut pretty, 0, if dom == ASSET_LANG { b"English" } else { b"Off" });
        } else if pick - 1 < st.asset_count as usize {
            let dir: &[u8] = match dom {
                ASSET_SPLASH => b"/user/splash",
                ASSET_ROGER => b"/user/rogers",
                ASSET_LANG => b"/user/lang",
                _ => b"/user/jingles",
            };
            let target = pick - 1;
            let mut raw_e = MaybeUninit::<KrxDirEntry>::uninit();
            let ent = unsafe { &mut *raw_e.as_mut_ptr() };
            let mut off = 0u32;
            let mut seen = 0usize;
            while api.list_dir_page(dir, off, core::slice::from_mut(ent)) > 0 {
                off += 1;
                if ent.is_dir == 0 {
                    if seen == target {
                        set_byte(cfg, flag_off, 1);
                        let dst = subslice_mut(cfg, path_off, path_off + 32);
                        fill_bytes(dst, 0);
                        let raw = cstr_slice(&ent.name);
                        if !raw.is_empty() && byte_at(raw, 0) == b'/' {
                            copy_bytes(dst, 0, raw);
                        } else {
                            let p = copy_bytes(dst, 0, dir);
                            set_byte(dst, p, b'/');
                            copy_bytes(dst, p + 1, raw);
                        }
                        format_pretty(raw, &mut pretty);
                        break;
                    }
                    seen += 1;
                }
            }
        }
        let name_dst = if dom == ASSET_SPLASH {
            &mut st.splash_name
        } else if dom == ASSET_ROGER {
            &mut st.roger_name
        } else if dom == ASSET_LANG {
            &mut st.lang_name
        } else {
            &mut st.jingle_name
        };
        copy_bytes(name_dst, 0, subslice(&pretty, 0, 12));
        let crc = api.crc32(subslice(cfg, 0, clen - 4)).to_le_bytes();
        copy_bytes(subslice_mut(cfg, clen - 4, clen), 0, &crc);
        if api.write_file(file, subslice(cfg, 0, clen)) {
            let _ = api.reload_config();
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
