//! P0 Leaf 20: Dedicated Language Apply (`/pref/system.cfg`).
//! Persists `lang_active`, `lang_path`, and CRC32, updates `str_fields[3]` with `__LANG_ASCII__`,
//! and triggers immediate OS language reload. Zero stack buffers.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    ASSET_LANG, SharedState, asset_matches, byte_at, copy_bytes, cstr_slice, fill_bytes,
    read_lng_ascii_name, set_byte, subslice, subslice_mut,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    let pick = st.arg0 as usize;
    let base = st.sheet_buf.as_mut_ptr();
    let cfg = unsafe { core::slice::from_raw_parts_mut(base, 80) };

    if api.read_file(b"/pref/system.cfg", cfg) >= 80 {
        fill_bytes(&mut st.str_fields[3], 0);
        if pick == 0 {
            set_byte(cfg, 40, 0);
            copy_bytes(&mut st.str_fields[3], 0, b"English");
        } else if pick - 1 < st.asset_count as usize {
            let target = pick - 1;
            let ents = unsafe {
                core::slice::from_raw_parts_mut(st.detail_sc.bp_tail.as_mut_ptr() as *mut KrxDirEntry, 4)
            };
            let s16 = unsafe { &mut *(base.add(80) as *mut [u8; 16]) };
            const DIR: &[u8] = b"/user/lang/";
            let mut off = 0u32;
            let mut seen = 0usize;
            let mut done = false;
            while !done {
                let got = api.list_dir_page(subslice(DIR, 0, 10), off, ents);
                if got == 0 {
                    break;
                }
                off += got as u32;
                let mut i = 0usize;
                while i < got {
                    let ent = unsafe { &*ents.as_ptr().add(i) };
                    let raw = cstr_slice(&ent.name);
                    if ent.is_dir == 0 && asset_matches(ASSET_LANG, raw) {
                        if seen == target {
                            set_byte(cfg, 40, 1);
                            let dst = unsafe { core::slice::from_raw_parts_mut(base.add(44), 32) };
                            fill_bytes(dst, 0);
                            let p = copy_bytes(dst, 0, DIR);
                            let fl = copy_bytes(dst, p, raw);
                            let full = subslice(dst, 0, fl);
                            let out = subslice_mut(&mut st.str_fields[3], 0, 11);
                            if read_lng_ascii_name(api, full, s16, out) == 0 && fl > 15 {
                                let _ = copy_bytes(out, 0, subslice(full, 11, fl - 4));
                                let c0 = byte_at(out, 0);
                                if c0 >= b'a' && c0 <= b'z' {
                                    set_byte(out, 0, c0 - 32);
                                }
                            }
                            done = true;
                            break;
                        }
                        seen += 1;
                    }
                    i += 1;
                }
                if got < 4 {
                    break;
                }
            }
        }
        let crc = api.crc32(subslice(cfg, 0, 76)).to_le_bytes();
        copy_bytes(cfg, 76, &crc);
        if api.write_file(b"/pref/system.cfg", cfg) {
            let _ = api.reload_config();
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
