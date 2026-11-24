//! P0 Leaf 11: Persist Model / Owner / Tag / 32-bit Radio ID (`/pref/system.cfg`) + CRC32 + reload.
//! Zero stack buffers, zero `__aeabi_lmul`.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry};
use shared::{
    CMD_DEV_MODEL_PICK, MODAL_TEXT_MODEL, MODAL_TEXT_OWNER, MODAL_TEXT_RADIO_ID, MODAL_TEXT_TAG,
    SharedState, byte_at, copy_bytes, copy_trimmed, cstr_slice, fill_bytes, format_u32_into,
    set_byte, subslice, subslice_mut,
};

#[inline(never)]
fn eq_ci(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < b.len() {
        if (byte_at(a, i) | 32) != (byte_at(b, i) | 32) {
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

    if st.cmd == CMD_DEV_MODEL_PICK && st.arg0 >= 2 {
        api.prompt_text(MODAL_TEXT_MODEL, b"Model", cstr_slice(&st.str_fields[4]), 12);
        ctx.continuation.kind = KRX_LEAF_KIND_DONE;
        return 0;
    }

    let base = st.sheet_buf.as_mut_ptr();
    let cfg = unsafe { core::slice::from_raw_parts_mut(base, 80) };
    if api.read_file(b"/pref/system.cfg", cfg) >= 80 {
        let mut new_model_id: Option<u8> = None;
        if st.cmd == CMD_DEV_MODEL_PICK {
            new_model_id = Some(st.arg0);
        } else {
            let txt = unsafe { core::slice::from_raw_parts_mut(base.add(80), 16) };
            let n = api.get_modal_text(txt).min(12);
            let raw = subslice(txt, 0, n);
            match st.arg0 {
                MODAL_TEXT_OWNER => {
                    fill_bytes(subslice_mut(cfg, 8, 20), b' ');
                    copy_bytes(cfg, 8, raw);
                    copy_trimmed(&mut st.str_fields[5], subslice(cfg, 8, 20));
                }
                MODAL_TEXT_TAG => {
                    fill_bytes(subslice_mut(cfg, 20, 28), b' ');
                    copy_bytes(cfg, 20, subslice(raw, 0, n.min(8)));
                    copy_trimmed(&mut st.str_fields[6], subslice(cfg, 20, 28));
                }
                MODAL_TEXT_RADIO_ID => {
                    let mut v = 0u32;
                    let mut i = 0usize;
                    while i < n {
                        let c = byte_at(txt, i);
                        if c >= b'0' && c <= b'9' && v <= 99_999_999 {
                            v = (v << 3) + (v << 1) + ((c - b'0') as u32);
                        }
                        i += 1;
                    }
                    fill_bytes(&mut st.str_fields[7], 0);
                    let _ = format_u32_into(api, v, &mut st.str_fields[7]);
                    copy_bytes(cfg, 4, &v.to_le_bytes());
                }
                _ => {
                    let id = if eq_ci(raw, b"uv-k1") || eq_ci(raw, b"k1") {
                        0u8
                    } else if eq_ci(raw, b"uv-k5") || eq_ci(raw, b"k5") {
                        1u8
                    } else {
                        2u8
                    };
                    fill_bytes(subslice_mut(cfg, 28, 40), b' ');
                    copy_bytes(cfg, 28, raw);
                    new_model_id = Some(id);
                }
            }
        }
        if let Some(id) = new_model_id {
            set_byte(cfg, 1, id);
            st.dev_model_idx = id;
            let disp: &[u8] = match id {
                0 => b"UV-K1",
                1 => b"UV-K5 V3",
                _ => subslice(cfg, 28, 40),
            };
            copy_trimmed(&mut st.str_fields[4], disp);
        }
        let crc = api.crc32(subslice(cfg, 0, 76)).to_le_bytes();
        copy_bytes(cfg, 76, &crc);
        if api.write_file(b"/pref/system.cfg", cfg) {
            let _ = api.reload_config();
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
