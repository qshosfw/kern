//! P0 Leaf 9: Load Device Identity (`/pref/system.cfg`), stage active `lang_path` into
//! `bp_tail[0..32]`, and tail-chain to `LEAF_BOOT_ASSETS`. Zero stack buffers.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_CALL_LEAF, KrxApiTable, KrxLeafContext, KrxSysInfo, krx_entry};
use shared::{
    LEAF_BOOT_ASSETS, SharedState, byte_at, copy_bytes, copy_trimmed, cstr_slice, fill_bytes,
    format_u32_into, set_byte, subslice,
};

#[inline(always)]
fn hex_nib(v: u8) -> u8 {
    let n = v & 0x0F;
    if n < 10 { b'0' + n } else { b'A' + (n - 10) }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let buf = unsafe { core::slice::from_raw_parts_mut(st.sheet_buf.as_mut_ptr(), 80) };
    let lang_stage = unsafe { core::slice::from_raw_parts_mut(st.detail_sc.bp_tail.as_mut_ptr(), 32) };
    fill_bytes(lang_stage, 0);
    fill_bytes(&mut st.str_fields[3], 0);
    copy_bytes(&mut st.str_fields[3], 0, b"English");

    if api.read_file(b"/pref/system.cfg", buf) >= 80 {
        st.dev_model_idx = byte_at(buf, 1);
        let radio_id = u32::from_le_bytes([
            byte_at(buf, 4),
            byte_at(buf, 5),
            byte_at(buf, 6),
            byte_at(buf, 7),
        ]);
        fill_bytes(&mut st.str_fields[7], 0);
        let _ = format_u32_into(api, radio_id, &mut st.str_fields[7]);
        copy_trimmed(&mut st.str_fields[5], subslice(buf, 8, 20));

        let tag_raw = subslice(buf, 20, 28);
        let mut all_def = true;
        let mut ti = 0usize;
        while ti < 8 {
            let b = byte_at(tag_raw, ti);
            if b != 0 && b != b' ' && b != b'0' {
                all_def = false;
                break;
            }
            ti += 1;
        }
        if all_def {
            fill_bytes(&mut st.str_fields[6], 0);
        } else {
            copy_trimmed(&mut st.str_fields[6], tag_raw);
        }
        let msrc: &[u8] = match st.dev_model_idx {
            0 => b"UV-K1",
            1 => b"UV-K5 V3",
            _ => subslice(buf, 28, 40),
        };
        copy_trimmed(&mut st.str_fields[4], msrc);

        if byte_at(buf, 40) != 0 {
            let lp = cstr_slice(subslice(buf, 44, 76));
            copy_bytes(lang_stage, 0, lp);
        }

        if all_def {
            let info = unsafe { &mut *(st.sheet_buf.as_mut_ptr() as *mut KrxSysInfo) };
            if api.sys_info(info) {
                let b4 = info.mac[4];
                let b5 = info.mac[5];
                set_byte(&mut st.str_fields[6], 0, hex_nib(b4 >> 4));
                set_byte(&mut st.str_fields[6], 1, hex_nib(b4));
                set_byte(&mut st.str_fields[6], 2, hex_nib(b5 >> 4));
                set_byte(&mut st.str_fields[6], 3, hex_nib(b5));
            }
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
    ctx.continuation.target_or_code = LEAF_BOOT_ASSETS;
    ctx.continuation.payload = ctx.arg;
    0
}

krx_entry!(leaf_entry);
