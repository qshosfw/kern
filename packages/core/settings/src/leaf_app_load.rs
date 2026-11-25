//! P0 Leaf 1: Parse `.krx`/`.mod` Header Metadata (Identity, Version, Author, Type, Profile, Caps)
//! via `SharedState::sheet_buf`, then tail-chain to `LEAF_APP_ICON` (`0x000B` action & `0x0004` icon).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    KRX_LEAF_KIND_CALL_LEAF, KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext,
    krx_entry,
};
use shared::{
    LEAF_APP_ICON, SharedState, byte_at, copy_bytes, cstr_slice, strip_apps_prefix, subslice,
    subslice_mut,
};

#[inline(never)]
fn cp(dst: &mut [u8], src: &[u8]) -> u8 {
    copy_bytes(dst, 0, src) as u8
}

#[inline(always)]
fn rd16(b: &[u8], o: usize) -> u16 {
    (byte_at(b, o) as u16) | ((byte_at(b, o + 1) as u16) << 8)
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let dir_idx = unsafe { *st.apps().dir_idx.get_unchecked((st.app_sel as usize).min(shared::APPS_MAX - 1)) } as u32;
    let base = st.sheet_buf.as_mut_ptr();
    let ent = unsafe { &mut *(base as *mut KrxDirEntry) };
    if api.list_dir_page(b"/apps", dir_idx, core::slice::from_mut(ent)) == 0 {
        ctx.continuation.kind = KRX_LEAF_KIND_DONE;
        return 1;
    }
    let ent_sz = ent.size;
    let m = unsafe { &mut *(&mut st.detail_sc as *mut _ as *mut shared::AppMeta) };
    let p = {
        let short = strip_apps_prefix(cstr_slice(&ent.name));
        m.name_len = cp(&mut m.name, short);
        let p = copy_bytes(&mut m.path, 0, b"/apps/");
        copy_bytes(&mut m.path, p, short)
    };
    m.path_len = p as u8;
    m.size = ent_sz;
    m.author_len = 0;
    m.action_len = 0;
    m.ver_major = 1;
    m.ver_minor = 0;
    m.ver_patch = 0;
    m.exec_type = 1;
    m.profile = 0x12;
    m.caps = 0;
    m.has_icon16 = 0;
    m.icon_id = 0;

    let buf = unsafe { core::slice::from_raw_parts_mut(base, 64) };
    let path = subslice(&m.path, 0, p);
    if api.read_file_at(path, 0, subslice_mut(buf, 0, 24)) >= 10
        && byte_at(buf, 0) == 0xC0
        && byte_at(buf, 1) == 0xDE
    {
        let mut s = rd16(buf, 6);
        let mut off = 24u32;
        let mut name_off = 0u32;
        while s > 0 && off < 768 {
            s -= 1;
            if api.read_file_at(path, off, subslice_mut(buf, 0, 4)) < 4 {
                break;
            }
            let sid = rd16(buf, 0);
            let slen = rd16(buf, 2) as usize;
            if slen < 4 {
                break;
            }
            let blen = (slen - 4).min(64);
            let n = api.read_file_at(path, off + 4, subslice_mut(buf, 0, blen));
            if sid == 1 && n >= 25 {
                name_off = rd16(buf, 0) as u32;
                m.ver_major = byte_at(buf, 2);
                m.ver_minor = byte_at(buf, 3);
                m.ver_patch = byte_at(buf, 4);
                m.author_len = cp(&mut m.author, cstr_slice(subslice(buf, 7, 19)));
                let ic = rd16(buf, 23);
                if ic > 0 && ic < 255 {
                    m.icon_id = ic as u8;
                }
            } else if sid == 9 && n >= 12 {
                m.exec_type = byte_at(buf, 0);
                m.caps = (rd16(buf, 8) as u32) | ((rd16(buf, 10) as u32) << 16);
            } else if sid == 2 && n >= 27 {
                m.profile = byte_at(buf, 26);
            }
            off += ((slen + 3) & !3) as u32;
        }
        if name_off > 0 {
            let nn = api.read_file_at(path, name_off, subslice_mut(buf, 0, 24));
            let nm = cstr_slice(subslice(buf, 0, nn));
            if !nm.is_empty() {
                m.name_len = cp(&mut m.name, nm);
            }
        }
        ctx.continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
        ctx.continuation.target_or_code = LEAF_APP_ICON;
        ctx.continuation.payload = ctx.arg;
        return 0;
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
