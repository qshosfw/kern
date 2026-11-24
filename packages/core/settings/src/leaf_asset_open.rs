//! P0 Leaf 7: Dynamic Splash / Roger / Jingle Asset Picker (Pretty Names + Active Highlight).
//! Zero stack buffers: uses `detail_sc.bp_tail` (`[0..32]` DirEntry, `[32..52]` pretty).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    MODAL_ASSET_PICK, SharedState, asset_matches, byte_at, copy_bytes, cstr_slice,
    format_pretty_asset, set_byte, subslice,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let dom = st.arg0.min(2);
    st.asset_domain = dom;
    let (dir, title): (&[u8], &[u8]) = if dom == 0 {
        (b"/user/splash", b"Splash Image")
    } else if dom == 1 {
        (b"/user/rogers", b"Roger Beep")
    } else {
        (b"/user/jingles", b"Boot Jingle")
    };
    let tlen = copy_bytes(&mut st.sheet_title, 0, title);
    let cur_name = cstr_slice(unsafe { &*st.str_fields.as_ptr().add(dom as usize) });
    let mut pos = copy_bytes(&mut st.sheet_buf, 0, b"Off\0");
    let mut cnt = 0u8;
    let mut sel = 0u8;
    let mut off = 0u32;

    let scratch = unsafe { st.detail_sc.bp_tail.as_mut_ptr() };
    let ent = unsafe { &mut *(scratch as *mut KrxDirEntry) };
    let pretty = unsafe { core::slice::from_raw_parts_mut(scratch.add(32), 20) };

    while api.list_dir_page(dir, off, core::slice::from_mut(ent)) > 0 {
        off += 1;
        let raw = cstr_slice(&ent.name);
        if ent.is_dir != 0 || !asset_matches(dom, raw) {
            continue;
        }
        let nlen = format_pretty_asset(raw, pretty);
        if nlen > 0 && pos + nlen + 1 < 128 {
            cnt = cnt.saturating_add(1);
            if sel == 0 && !cur_name.is_empty() && nlen >= cur_name.len() {
                let mut i = 0usize;
                while i < cur_name.len() && byte_at(pretty, i) == byte_at(cur_name, i) {
                    i += 1;
                }
                if i == cur_name.len() {
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
        MODAL_ASSET_PICK,
        subslice(&st.sheet_title, 0, tlen),
        subslice(&st.sheet_buf, 0, pos),
        sel,
        true,
    );
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
