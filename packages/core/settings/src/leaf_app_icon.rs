//! P0 Leaf 16: Stream-Extract Actions (`0x000B`) & 16x16 Icon (`0x0004`) via `SharedState::sheet_buf`.
#![no_std]
#![no_main]

mod shared;

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
    let buf = unsafe { core::slice::from_raw_parts_mut(st.sheet_buf.as_mut_ptr(), 96) };
    let m = st.meta_mut();
    let path = subslice(&m.path, 0, m.path_len as usize);

    if api.read_file_at(path, 0, shared::subslice_mut(buf, 0, 24)) >= 10 && byte_at(buf, 0) == 0xC0 && byte_at(buf, 1) == 0xDE {
        let mut s = rd16(buf, 6);
        let mut off = 24u32;
        while s > 0 && off < 512 {
            s -= 1;
            if api.read_file_at(path, off, shared::subslice_mut(buf, 0, 4)) < 4 {
                break;
            }
            let sid = rd16(buf, 0);
            let slen = rd16(buf, 2) as usize;
            if slen < 4 {
                break;
            }
            let blen = (slen - 4).min(96);
            if sid == 11 && blen >= 16 {
                if api.read_file_at(path, off + 4, shared::subslice_mut(buf, 0, 16)) >= 16 {
                    m.action_len = copy_bytes(&mut m.action_label, 0, cstr_slice(subslice(buf, 4, 16))) as u8;
                }
            } else if sid == 4 && blen >= 36 {
                let n = api.read_file_at(path, off + 4, shared::subslice_mut(buf, 0, blen));
                let mut ao = 0usize;
                while ao + 4 <= n {
                    let aw = byte_at(buf, ao + 1) as usize;
                    let ah = byte_at(buf, ao + 2) as usize;
                    let sz = ((aw + 7) >> 3) * ah;
                    if ao + 4 + sz > n {
                        break;
                    }
                    if aw == 16 && ah == 16 && sz == 32 {
                        copy_bytes(&mut m.icon_16, 0, subslice(buf, ao + 4, ao + 36));
                        m.has_icon16 = 1;
                        break;
                    }
                    ao += 4 + sz;
                }
            }
            off += ((slen + 3) & !3) as u32;
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
