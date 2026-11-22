//! P0 Leaf 13: `/apps/` Scan, Launch, Delete Prompt & Delete Execution.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    CMD_APPS_DO_DEL, CMD_APPS_LAUNCH, CMD_APPS_REFRESH, CMD_APPS_REQ_DEL, MODAL_DELETE_APP,
    SharedState, byte_at, copy_bytes, cstr_slice, is_krx_module, strip_apps_prefix, subslice,
};

fn eq_b(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut i = 0usize;
    while i < b.len() {
        if byte_at(a, i) != byte_at(b, i) { return false; }
        i += 1;
    }
    true
}

/// Stream `/apps` once: counts all matching entries, and when `target < 255` writes that
/// entry's full `/apps/<name>` path into `pbuf` and returns its byte length in `out_len`.
#[inline(never)]
fn scan_apps(
    api: &KrxApiTable,
    mod_only: bool,
    target: u8,
    pbuf: &mut [u8; 32],
    out_len: &mut usize,
) -> u8 {
    *out_len = 0;
    if !mod_only && target == 255 {
        return api.count_dir(b"/apps").min(255) as u8;
    }
    let mut raw_e = MaybeUninit::<KrxDirEntry>::uninit();
    let ent = unsafe { &mut *raw_e.as_mut_ptr() };
    let mut off = 0u32;
    let mut matched = 0usize;
    while api.list_dir_page(b"/apps", off, core::slice::from_mut(ent)) > 0 {
        off += 1;
        let short = strip_apps_prefix(cstr_slice(&ent.name));
        let pos = copy_bytes(pbuf, 0, b"/apps/");
        let pos = copy_bytes(pbuf, pos, short);
        if mod_only && !is_krx_module(api, subslice(pbuf, 0, pos)) {
            continue;
        }
        if matched == target as usize {
            *out_len = pos;
            return (matched + 1).min(255) as u8;
        }
        matched += 1;
    }
    matched.min(255) as u8
}

fn refresh(api: &KrxApiTable, st: &mut SharedState, mod_only: bool, pbuf: &mut [u8; 32]) {
    st.modules_only = u8::from(mod_only);
    let mut dummy = 0usize;
    st.app_count = scan_apps(api, mod_only, 255, pbuf, &mut dummy);
    if st.app_count == 0 { st.app_sel = 0; }
    else if st.app_sel >= st.app_count { st.app_sel = st.app_count - 1; }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() { return 1; }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let mut raw_p = MaybeUninit::<[u8; 32]>::uninit();
    let pbuf = unsafe { &mut *raw_p.as_mut_ptr() };
    st.result = 0;

    if st.cmd == CMD_APPS_REFRESH {
        refresh(api, st, st.arg0 != 0, pbuf);
    } else if st.app_count > 0 {
        let mut plen = 0usize;
        let _ = scan_apps(api, st.modules_only != 0, st.app_sel, pbuf, &mut plen);
        if plen > 6 {
            let full = subslice(pbuf, 0, plen);
            let short = subslice(pbuf, 6, plen);
            match st.cmd {
                CMD_APPS_LAUNCH => {
                    if !api.launch_pkg(full) { api.show_toast(b"Launch Failed", 1200); }
                }
                CMD_APPS_REQ_DEL => {
                    if eq_b(short, b"settings.krx") {
                        api.show_toast(b"Core Settings App", 1200);
                    } else {
                        api.show_confirm(MODAL_DELETE_APP, b"Remove App?", short, b"Hold", b"Cancel", 1000);
                    }
                }
                CMD_APPS_DO_DEL => {
                    if api.remove_file(full) {
                        refresh(api, st, st.modules_only != 0, pbuf);
                        st.result = 1;
                        api.show_toast(b"Removed", 1200);
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
