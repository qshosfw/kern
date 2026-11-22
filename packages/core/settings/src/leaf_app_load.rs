//! P0 Leaf 1: Stream-Parse `.krx`/`.mod` Header (Identity, Version, Author, Type, Profile, Caps)
//! using only a 128-byte stack buffer, then chain to `leaf_app_icon` for Actions & 16x16 Icon.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{KRX_LEAF_KIND_CALL_LEAF, KrxApiTable, KrxLeafContext, krx_entry};
use shared::{
    LEAF_APP_ICON, MAX_APPS, SharedState, byte_at, copy_bytes, cstr_slice, strip_apps_prefix,
    subslice,
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
    let idx = (st.app_sel as usize).min(MAX_APPS - 1);
    let mut raw_n = MaybeUninit::<[u8; 24]>::uninit();
    let nbuf = unsafe { &mut *raw_n.as_mut_ptr() };
    let (ent_sz, slen_n) = {
        let ent = unsafe { st.apps().get_unchecked(idx) };
        let short = strip_apps_prefix(cstr_slice(&ent.name));
        (ent.size, copy_bytes(nbuf, 0, short))
    };
    let short = subslice(nbuf, 0, slen_n);
    let m = st.meta_mut();
    let p = copy_bytes(&mut m.path, 0, b"/apps/");
    let p = copy_bytes(&mut m.path, p, short);
    m.path_len = p as u8;
    m.size = ent_sz;
    m.name_len = cp(&mut m.name, short);
    m.author_len = 0;
    m.action_len = 0;
    m.ver_major = 1;
    m.ver_minor = 0;
    m.ver_patch = 0;
    m.exec_type = 1;
    m.profile = 0x12;
    m.caps = 0;
    m.has_icon16 = 0;

    // Preamble (24B) + Identity (44B) + TypeLifecycle (16B) + Execution (32B) = 116B <= 128B.
    let mut raw = MaybeUninit::<[u8; 128]>::uninit();
    let hdr = unsafe { &mut *raw.as_mut_ptr() };
    let path = subslice(&m.path, 0, p);
    let n = api.read_file_at(path, 0, hdr);
    if n >= 28 && byte_at(hdr, 0) == 0xC0 && byte_at(hdr, 1) == 0xDE {
        let mut off = 24usize;
        let mut s = rd16(hdr, 6);
        while s > 0 && off + 4 <= n {
            s -= 1;
            let sid = rd16(hdr, off);
            let slen = rd16(hdr, off + 2) as usize;
            if slen < 4 || off + slen > n {
                break;
            }
            let b = off + 4;
            let end = off + slen;
            if sid == 1 && end >= b + 26 {
                let noff = rd16(hdr, b) as u32;
                m.ver_major = byte_at(hdr, b + 2);
                m.ver_minor = byte_at(hdr, b + 3);
                m.ver_patch = byte_at(hdr, b + 4);
                m.author_len = cp(&mut m.author, cstr_slice(subslice(hdr, b + 7, b + 19)));
                let nn = api.read_file_at(path, noff, nbuf);
                let nm = cstr_slice(subslice(nbuf, 0, nn));
                if !nm.is_empty() {
                    m.name_len = cp(&mut m.name, nm);
                }
            } else if sid == 9 && end >= b + 12 {
                m.exec_type = byte_at(hdr, b);
                m.caps = (rd16(hdr, b + 8) as u32) | ((rd16(hdr, b + 10) as u32) << 16);
            } else if sid == 2 && end >= b + 27 {
                m.profile = byte_at(hdr, b + 26);
            }
            off += (slen + 3) & !3;
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
    ctx.continuation.target_or_code = LEAF_APP_ICON;
    ctx.continuation.payload = ctx.arg;
    0
}

krx_entry!(leaf_entry);
