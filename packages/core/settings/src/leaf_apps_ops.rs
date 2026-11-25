//! P0 Leaf 13: O(1) App Launch, Delete Confirm & Delete Execution (chains to `LEAF_APPS_SCAN`).
//! Zero stack buffers: uses `st.sheet_buf` as scratch.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    KRX_LEAF_KIND_CALL_LEAF, KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext,
    krx_entry,
};
use shared::{
    CMD_APPS_DO_DEL, CMD_APPS_LAUNCH, CMD_APPS_REFRESH, CMD_APPS_REQ_DEL, LEAF_APPS_SCAN,
    MODAL_DELETE_APP, SharedState, byte_at, copy_bytes, cstr_slice, strip_apps_prefix, subslice,
};

fn eq_b(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        if byte_at(a, i) != byte_at(b, i) {
            return false;
        }
        i += 1;
    }
    true
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    st.result = 0;

    if st.app_count > 0 {
        let base = st.sheet_buf.as_mut_ptr();
        let pbuf = unsafe { core::slice::from_raw_parts_mut(base.add(32), 32) };
        let plen = if st.modules_only == 0 && st.meta().path_len > 6 {
            let m = st.meta();
            copy_bytes(pbuf, 0, subslice(&m.path, 0, (m.path_len as usize).min(32)))
        } else {
            let dir_i = unsafe { *st.apps().dir_idx.get_unchecked((st.app_sel as usize).min(shared::APPS_MAX - 1)) } as u32;
            let ent = unsafe { &mut *(base as *mut KrxDirEntry) };
            if api.list_dir_page(b"/apps", dir_i, core::slice::from_mut(ent)) > 0 {
                let short = strip_apps_prefix(cstr_slice(&ent.name));
                let p = copy_bytes(pbuf, 0, b"/apps/");
                copy_bytes(pbuf, p, short)
            } else {
                0
            }
        };
        if plen > 6 {
            let full = subslice(pbuf, 0, plen);
            let short = subslice(pbuf, 6, plen);
            match st.cmd {
                CMD_APPS_LAUNCH => {
                    if !api.launch_pkg(full) {
                        api.show_toast(b"Launch Failed", 1200);
                    }
                }
                CMD_APPS_REQ_DEL => {
                    if eq_b(short, b"settings.krx") {
                        api.show_toast(b"Core Settings App", 1200);
                    } else {
                        api.show_confirm(
                            MODAL_DELETE_APP,
                            b"Remove Package?",
                            short,
                            b"Hold",
                            b"Cancel",
                            1000,
                        );
                    }
                }
                CMD_APPS_DO_DEL => {
                    if api.remove_file(full) {
                        st.result = 1;
                        api.show_toast(b"Removed", 1200);
                        st.cmd = CMD_APPS_REFRESH;
                        st.arg0 = st.modules_only;
                        ctx.continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
                        ctx.continuation.target_or_code = LEAF_APPS_SCAN;
                        ctx.continuation.payload = ctx.arg;
                        return 0;
                    } else {
                        api.show_toast(b"Remove Failed", 1200);
                    }
                }
                _ => {}
            }
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
