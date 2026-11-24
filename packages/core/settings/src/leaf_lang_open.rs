//! P0 Leaf 19: Dedicated Language Picker (`__LANG_ASCII__` + Active Radio Highlight).
//! Scans `/user/lang/*.lng` (skipping app packs containing `_`), reads self-describing
//! `__LANG_ASCII__` labels (falling back to capitalized code stem), and opens `OptionSheet`.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    ASSET_LANG, MODAL_LANG_PICK, SharedState, asset_matches, byte_at, copy_bytes, cstr_slice,
    read_lng_ascii_name, set_byte, subslice,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    st.asset_domain = ASSET_LANG;
    let tlen = copy_bytes(&mut st.sheet_title, 0, b"Language");
    let cur = cstr_slice(&st.str_fields[3]);
    let mut pos = copy_bytes(&mut st.sheet_buf, 0, b"English\0");
    let mut cnt = 0u8;
    let mut sel = 0u8;
    let mut off = 0u32;

    let scratch = unsafe { st.detail_sc.bp_tail.as_mut_ptr() };
    let ent = unsafe { &mut *(scratch as *mut KrxDirEntry) };
    let full = unsafe { core::slice::from_raw_parts_mut(scratch.add(32), 32) };
    let pretty = unsafe { core::slice::from_raw_parts_mut(scratch.add(64), 20) };
    let lng16 = unsafe { &mut *(scratch.add(84) as *mut [u8; 16]) };

    while api.list_dir_page(b"/user/lang", off, core::slice::from_mut(ent)) > 0 {
        off += 1;
        let raw = cstr_slice(&ent.name);
        if ent.is_dir != 0 || !asset_matches(ASSET_LANG, raw) {
            continue;
        }
        let fp = copy_bytes(full, 0, b"/user/lang/");
        let fl = copy_bytes(full, fp, raw);
        let mut nlen = read_lng_ascii_name(api, subslice(full, 0, fl), lng16, pretty);
        if nlen == 0 {
            nlen = copy_bytes(pretty, 0, subslice(raw, 0, raw.len() - 4));
            let c0 = byte_at(pretty, 0);
            if c0 >= b'a' && c0 <= b'z' {
                set_byte(pretty, 0, c0 - 32);
            }
        }
        if nlen > 0 && pos + nlen + 1 < 128 {
            cnt = cnt.saturating_add(1);
            if sel == 0 && !cur.is_empty() && nlen >= cur.len() {
                let mut k = 0usize;
                while k < cur.len() && byte_at(pretty, k) == byte_at(cur, k) {
                    k += 1;
                }
                if k == cur.len() {
                    sel = cnt;
                }
            }
            pos = copy_bytes(&mut st.sheet_buf, pos, subslice(pretty, 0, nlen));
            set_byte(&mut st.sheet_buf, pos, 0);
            pos += 1;
        }
    }
    st.asset_count = cnt;
    api.show_sheet(
        MODAL_LANG_PICK,
        subslice(&st.sheet_title, 0, tlen),
        subslice(&st.sheet_buf, 0, pos),
        sel,
        true,
    );
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
