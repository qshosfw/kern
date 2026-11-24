//! P0 Leaf 8: Persist Selected Splash / Roger / Jingle Asset Path + CRC32.
//! Zero stack buffers: uses `st.sheet_buf[80..112]` (`KrxDirEntry`).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    ASSET_ROGER, ASSET_SPLASH, SharedState, asset_matches, copy_bytes, cstr_slice, fill_bytes,
    format_pretty_asset, set_byte,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let pick = st.arg0 as usize;
    let dom = st.asset_domain.min(2);
    let base = st.sheet_buf.as_mut_ptr();

    let (file, dir, clen, flag_off, path_off): (&[u8], &[u8], usize, usize, usize) = match dom {
        ASSET_SPLASH => (b"/pref/display.cfg", b"/user/splash", 48, 7, 12),
        ASSET_ROGER => (b"/pref/audio.cfg", b"/user/rogers", 80, 4, 9),
        _ => (b"/pref/audio.cfg", b"/user/jingles", 80, 7, 41),
    };

    let cfg = unsafe { core::slice::from_raw_parts_mut(base, clen) };
    if api.read_file(file, cfg) >= clen {
        let name_dst = unsafe { &mut *st.str_fields.as_mut_ptr().add(dom as usize) };
        fill_bytes(name_dst, 0);
        if pick == 0 {
            set_byte(cfg, flag_off, 0);
            copy_bytes(name_dst, 0, b"Off");
        } else if pick - 1 < st.asset_count as usize {
            let target = pick - 1;
            let ent = unsafe { &mut *(base.add(80) as *mut KrxDirEntry) };
            let mut off = 0u32;
            let mut seen = 0usize;
            while api.list_dir_page(dir, off, core::slice::from_mut(ent)) > 0 {
                off += 1;
                let raw = cstr_slice(&ent.name);
                if ent.is_dir == 0 && asset_matches(dom, raw) {
                    if seen == target {
                        set_byte(cfg, flag_off, 1);
                        let dst = unsafe { core::slice::from_raw_parts_mut(base.add(path_off), 32) };
                        fill_bytes(dst, 0);
                        let p = copy_bytes(dst, 0, dir);
                        set_byte(dst, p, b'/');
                        let _ = copy_bytes(dst, p + 1, raw);
                        let _ = format_pretty_asset(raw, name_dst);
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
