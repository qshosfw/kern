//! P0 Leaf 22: Stream `/apps/` Once to Populate `SharedState::apps()` RAM Cache.
//! Zero stack buffers: uses `st.sheet_buf` (`[0..32]` DirEntry, `[32..96]` KRX hdr, `[96..128]` path).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    SharedState, byte_at, copy_bytes, cstr_slice, fill_bytes, read_krx_meta, strip_apps_prefix,
    subslice,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let mod_only = st.arg0 != 0;
    st.modules_only = u8::from(mod_only);

    let base = st.sheet_buf.as_mut_ptr();
    let ent = unsafe { &mut *(base as *mut KrxDirEntry) };
    let hbuf = unsafe { core::slice::from_raw_parts_mut(base.add(32), 64) };
    let pbuf = unsafe { core::slice::from_raw_parts_mut(base.add(96), 32) };
    let mut off = 0u32;
    let mut matched = 0usize;

    while matched < 9 && api.list_dir_page(b"/apps", off, core::slice::from_mut(ent)) > 0 {
        let dir_i = off as u8;
        off += 1;
        let short = strip_apps_prefix(cstr_slice(&ent.name));
        let pos = copy_bytes(pbuf, 0, b"/apps/");
        let pos = copy_bytes(pbuf, pos, short);
        let (exec_type, icon_id) = read_krx_meta(api, subslice(pbuf, 0, pos), hbuf);
        if mod_only != (exec_type >= 2) {
            continue;
        }
        let cache = st.apps_mut();
        let name_dst = unsafe { cache.names.get_unchecked_mut(matched) };
        fill_bytes(name_dst, 0);
        let nlen = if short.len() > 4 && byte_at(short, short.len() - 4) == b'.' {
            short.len() - 4
        } else {
            short.len()
        };
        copy_bytes(name_dst, 0, subslice(short, 0, nlen));
        let sz = ent.size;
        unsafe {
            *cache.sizes.get_unchecked_mut(matched) = if sz >= 1024 {
                0x8000 | (((sz >> 10) * 10 + (((sz & 1023) * 10) >> 10)).min(0x7FFF) as u16)
            } else {
                sz as u16
            };
            *cache.icons.get_unchecked_mut(matched) = if icon_id != 0 {
                icon_id
            } else if mod_only {
                abi::icon::SYSTEM
            } else {
                abi::icon::APPS
            };
            *cache.types.get_unchecked_mut(matched) = exec_type;
            *cache.dir_idx.get_unchecked_mut(matched) = dir_i;
        }
        matched += 1;
    }
    st.app_count = matched as u8;
    if st.app_count == 0 {
        st.app_sel = 0;
    } else if st.app_sel >= st.app_count {
        st.app_sel = st.app_count - 1;
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
