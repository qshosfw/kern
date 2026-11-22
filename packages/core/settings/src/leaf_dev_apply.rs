//! P0 Leaf 11: Apply Model/Owner/Tag/Radio ID Changes to `/pref/system.cfg` + CRC32.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry};
use shared::{
    CMD_DEV_MODEL_PICK, MODAL_TEXT_MODEL, MODAL_TEXT_OWNER, MODAL_TEXT_RADIO_ID, MODAL_TEXT_TAG,
    SharedState, byte_at, copy_bytes, fill_bytes, set_byte, subslice, subslice_mut, trimmed_slice,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    if st.cmd == CMD_DEV_MODEL_PICK && st.arg0 >= 2 {
        api.prompt_text(MODAL_TEXT_MODEL, b"Model", trimmed_slice(&st.dev_model), 12);
        ctx.continuation.kind = KRX_LEAF_KIND_DONE;
        return 0;
    }

    let base = st.sheet_buf.as_mut_ptr();
    let cfg = unsafe { core::slice::from_raw_parts_mut(base, 80) };
    if api.read_file(b"/pref/system.cfg", cfg) >= 80 {
        if st.cmd == CMD_DEV_MODEL_PICK {
            set_byte(cfg, 1, st.arg0);
            st.dev_model_idx = st.arg0;
            fill_bytes(&mut st.dev_model, 0);
            copy_bytes(&mut st.dev_model, 0, if st.arg0 == 0 { b"UV-K1" } else { b"UV-K5 V3" });
        } else {
            let txt = unsafe { core::slice::from_raw_parts_mut(base.add(80), 16) };
            let n = api.get_modal_text(txt).min(12);
            match st.arg0 {
                MODAL_TEXT_OWNER => {
                    fill_bytes(subslice_mut(cfg, 8, 20), b' ');
                    copy_bytes(subslice_mut(cfg, 8, 20), 0, subslice(txt, 0, n));
                    copy_bytes(&mut st.dev_owner, 0, subslice(cfg, 8, 20));
                }
                MODAL_TEXT_TAG => {
                    fill_bytes(subslice_mut(cfg, 20, 28), b' ');
                    copy_bytes(subslice_mut(cfg, 20, 28), 0, subslice(txt, 0, n.min(8)));
                    copy_bytes(&mut st.dev_tag, 0, subslice(cfg, 20, 28));
                }
                MODAL_TEXT_RADIO_ID => {
                    let mut v = 0u32;
                    let mut i = 0usize;
                    while i < n {
                        let c = byte_at(txt, i);
                        if c >= b'0' && c <= b'9' {
                            v = v.saturating_mul(10).saturating_add((c - b'0') as u32);
                        }
                        i += 1;
                    }
                    st.dev_radio_id = v;
                    copy_bytes(subslice_mut(cfg, 4, 8), 0, &v.to_le_bytes());
                }
                _ => {
                    set_byte(cfg, 1, 2);
                    fill_bytes(subslice_mut(cfg, 28, 40), b' ');
                    copy_bytes(subslice_mut(cfg, 28, 40), 0, subslice(txt, 0, n));
                    st.dev_model_idx = 2;
                    copy_bytes(&mut st.dev_model, 0, subslice(cfg, 28, 40));
                }
            }
        }
        let crc = api.crc32(subslice(cfg, 0, 76)).to_le_bytes();
        copy_bytes(subslice_mut(cfg, 76, 80), 0, &crc);
        if api.write_file(b"/pref/system.cfg", cfg) {
            let _ = api.reload_config();
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
