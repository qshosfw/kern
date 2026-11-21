//! P0 Leaf 16: Extract Embedded 16x16 Icon Bitmap (`SEC_ASSETS = 0x0004`) from `.krx`/`.mod`.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry};
use shared::{SharedState, byte_at, copy_bytes, cstr_slice, subslice};

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
    let m = st.meta_mut();
    let mut raw = MaybeUninit::<[u8; 512]>::uninit();
    let hdr = unsafe { &mut *raw.as_mut_ptr() };
    let n = api.read_file(subslice(&m.path, 0, m.path_len as usize), hdr);
    if n >= 28 && byte_at(hdr, 0) == 0xC0 && byte_at(hdr, 1) == 0xDE {
        let mut off = 24usize;
        let mut s = rd16(hdr, 6);
        while s > 0 && off + 4 <= n {
            s -= 1;
            let sid = rd16(hdr, off);
            let slen = rd16(hdr, off + 2) as usize;
            if slen < 4 || off + slen > n { break; }
            if sid == 11 && slen >= 20 {
                m.action_len = copy_bytes(&mut m.action_label, 0, cstr_slice(subslice(hdr, off + 8, off + 20))) as u8;
            } else if sid == 4 && slen >= 40 {
                let mut ao = off + 4;
                let end = off + slen;
                while ao + 4 <= end {
                    let aw = byte_at(hdr, ao + 1) as usize;
                    let ah = byte_at(hdr, ao + 2) as usize;
                    let sz = ((aw + 7) >> 3) * ah;
                    if ao + 4 + sz > end { break; }
                    if aw == 16 && ah == 16 && sz == 32 {
                        copy_bytes(&mut m.icon_16, 0, subslice(hdr, ao + 4, ao + 36));
                        m.has_icon16 = 1;
                        break;
                    }
                    ao += 4 + sz;
                }
            }
            off += (slen + 3) & !3;
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
