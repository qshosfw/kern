//! P0 Leaf 22: Stream `/apps/` Once to Populate `SharedState::apps()` RAM Cache.
//!
//! Reads real KRX header display titles, `icon_id`, and `exec_type` across the full header
//! (up to 768 B). In `Packages` mode (`mod_only == 0`), shows ALL package types with Apps
//! ordered first (`[0..apps_n]`) and Services/Daemons/Drivers/Modules second (`[apps_n..count]`).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, icon, krx_entry};
use shared::{APPS_MAX, SharedState, cstr_slice, strip_apps_prefix};

#[inline(always)]
fn rd16(p: *const u8, o: usize) -> u16 {
    unsafe { (*p.add(o) as u16) | ((*p.add(o + 1) as u16) << 8) }
}

#[inline(never)]
fn rdat(api: &KrxApiTable, full: &[u8], off: u32, bp: *mut u8, len: usize) -> usize {
    api.read_file_at(full, off, unsafe { core::slice::from_raw_parts_mut(bp, len) })
}

/// Copy `src` into `dst[..max]` and zero-fill remaining bytes (zero `memset` call).
#[inline(never)]
fn cpy_z(dst: *mut u8, src: &[u8], max: usize) -> usize {
    let n = src.len().min(max);
    let mut i = 0usize;
    while i < max {
        unsafe { *dst.add(i) = if i < n { *src.get_unchecked(i) } else { 0 } };
        i += 1;
    }
    n
}

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
    let bp = unsafe { base.add(32) };
    let pp = unsafe { base.add(96) };
    cpy_z(pp, b"/apps/", 6);

    let mut matched = 0usize;
    let mut apps_n = 0u8;
    let mut pass = u8::from(mod_only);
    while pass < 2 && matched < APPS_MAX {
        let mut off = 0u32;
        while matched < APPS_MAX && api.list_dir_page(b"/apps", off, core::slice::from_mut(ent)) > 0 {
            let dir_i = off as u8;
            off += 1;
            let short = strip_apps_prefix(cstr_slice(&ent.name));
            let plen = 6 + cpy_z(unsafe { pp.add(6) }, short, 26);
            let full = unsafe { core::slice::from_raw_parts(pp, plen) };

            let mut exec_type = 1u8;
            let mut icon_id = 0u8;
            let mut name_off = 0u32;
            if rdat(api, full, 0, bp, 24) >= 10 && rd16(bp, 0) == 0xDEC0 {
                let mut secs = rd16(bp, 6);
                let hlen = (rd16(bp, 8) as u32).min(768);
                let mut soff = 24u32;
                while secs > 0 && soff + 4 <= hlen {
                    secs -= 1;
                    if rdat(api, full, soff, bp, 4) < 4 {
                        break;
                    }
                    let sid = rd16(bp, 0);
                    let slen = rd16(bp, 2) as usize;
                    if slen < 4 {
                        break;
                    }
                    if sid == 1 && slen >= 29 && rdat(api, full, soff + 4, bp, 25) >= 25 {
                        name_off = rd16(bp, 0) as u32;
                        let ic = rd16(bp, 23);
                        if ic < 255 {
                            icon_id = ic as u8;
                        }
                    } else if sid == 9 && slen >= 5 && rdat(api, full, soff + 4, bp, 1) >= 1 {
                        exec_type = unsafe { *bp };
                    }
                    soff += ((slen + 3) & !3) as u32;
                }
            }
            if (pass == 0) == (exec_type >= 2) {
                continue;
            }

            let cache = st.apps_mut();
            let dst = unsafe { cache.names.get_unchecked_mut(matched) };
            let nn = if name_off >= 24 { rdat(api, full, name_off, bp, 9) } else { 0 };
            let nm = cstr_slice(unsafe { core::slice::from_raw_parts(bp, nn) });
            cpy_z(dst.as_mut_ptr(), if nm.is_empty() { short } else { nm }, 9);

            let sz = ent.size;
            let enc_sz: u8 = if sz >= 1024 {
                0x80 | ((sz >> 9).min(127) as u8)
            } else {
                (sz >> 4).min(63) as u8
            };
            if icon_id == 0 {
                icon_id = if exec_type <= 1 { icon::APPS } else { icon::SYSTEM };
            }
            unsafe {
                *cache.sizes.get_unchecked_mut(matched) = enc_sz;
                *cache.icons.get_unchecked_mut(matched) = icon_id;
                *cache.types.get_unchecked_mut(matched) = exec_type;
                *cache.dir_idx.get_unchecked_mut(matched) = dir_i;
            }
            matched += 1;
            if pass == 0 {
                apps_n += 1;
            }
        }
        pass += 1;
    }

    st.apps_mut().apps_n = apps_n;
    st.app_count = matched as u8;
    if st.app_sel >= st.app_count {
        st.app_sel = st.app_count.saturating_sub(1);
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
