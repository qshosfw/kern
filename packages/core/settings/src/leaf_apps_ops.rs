//! P0 Leaf 13: `/apps/` Scan, Launch, Delete Prompt & Delete Execution.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry};
use shared::{
    CMD_APPS_DO_DEL, CMD_APPS_LAUNCH, CMD_APPS_REFRESH, CMD_APPS_REQ_DEL, MAX_APPS,
    MODAL_DELETE_APP, SharedState, byte_at, copy_bytes, cstr_slice, strip_apps_prefix, subslice,
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

/// Declaratively inspect a KRX header's `SECTION_TYPE_LIFECYCLE` (`0x0009`) to check if `exec_type == Module (5)`.
fn is_krx_module(api: &KrxApiTable, full: &[u8]) -> bool {
    let mut raw = MaybeUninit::<[u8; 96]>::uninit();
    let buf = unsafe { &mut *raw.as_mut_ptr() };
    let n = api.read_file_at(full, 0, buf);
    if n < 28 {
        return false;
    }
    let hlen = ((byte_at(buf, 8) as usize) | ((byte_at(buf, 9) as usize) << 8)).min(n);
    let mut off = 24usize;
    while off + 4 <= hlen {
        let sid = (byte_at(buf, off) as u16) | ((byte_at(buf, off + 1) as u16) << 8);
        let slen = (byte_at(buf, off + 2) as usize) | ((byte_at(buf, off + 3) as usize) << 8);
        if slen < 4 || off + slen > hlen {
            break;
        }
        if sid == 0x0009 && slen >= 5 {
            return byte_at(buf, off + 4) == 5; // KrxExecType::Module
        }
        off += (slen + 3) & !3;
    }
    false
}

fn refresh(api: &KrxApiTable, st: &mut SharedState, mod_only: bool) {
    st.modules_only = u8::from(mod_only);
    let total = api.list_dir(b"/apps", st.apps_mut()).min(MAX_APPS);
    if mod_only {
        let mut p_u = MaybeUninit::<[u8; 32]>::uninit();
        let pbuf = unsafe { &mut *p_u.as_mut_ptr() };
        let mut dst = 0usize;
        let mut i = 0usize;
        while i < total {
            let short = strip_apps_prefix(cstr_slice(unsafe { &st.apps().get_unchecked(i).name }));
            let mut pos = copy_bytes(pbuf, 0, b"/apps/");
            pos = copy_bytes(pbuf, pos, short);
            if is_krx_module(api, subslice(pbuf, 0, pos)) {
                if dst != i {
                    unsafe {
                        let sp = st.apps().as_ptr().add(i) as *const u32;
                        let dp = st.apps_mut().as_mut_ptr().add(dst) as *mut u32;
                        let mut w = 0usize;
                        while w < 8 { *dp.add(w) = *sp.add(w); w += 1; }
                    }
                }
                dst += 1;
            }
            i += 1;
        }
        st.app_count = dst as u8;
    } else {
        st.app_count = total as u8;
    }
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
    let _ = core::mem::size_of::<KrxDirEntry>();

    if st.cmd == CMD_APPS_REFRESH {
        refresh(api, st, st.arg0 != 0);
    } else if st.app_count > 0 {
        let idx = (st.app_sel as usize).min(MAX_APPS - 1);
        let short = strip_apps_prefix(cstr_slice(unsafe { &st.apps().get_unchecked(idx).name }));
        let mut pos = copy_bytes(pbuf, 0, b"/apps/");
        pos = copy_bytes(pbuf, pos, short);
        let full = subslice(pbuf, 0, pos);
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
                    refresh(api, st, st.modules_only != 0);
                    st.result = 1;
                    api.show_toast(b"Removed", 1200);
                } else {
                    api.show_toast(b"Remove Failed", 1200);
                }
            }
            _ => {}
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
