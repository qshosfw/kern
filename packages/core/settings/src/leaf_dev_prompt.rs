//! P0 Leaf 10: Open Native Model OptionSheet or Owner/Tag/Radio ID TextInput Modal.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, format_u16_into, krx_entry};
use shared::{
    CMD_DEV_MODEL_SHEET, MODAL_MODEL_PICK, MODAL_TEXT_MODEL, MODAL_TEXT_OWNER, MODAL_TEXT_RADIO_ID,
    MODAL_TEXT_TAG, SharedState, copy_bytes, subslice, trimmed_slice,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    if st.cmd == CMD_DEV_MODEL_SHEET {
        let len = copy_bytes(&mut st.sheet_buf, 0, b"UV-K1\0UV-K5 V3\0Custom...\0");
        let tlen = copy_bytes(&mut st.sheet_title, 0, b"Model");
        api.show_sheet(
            MODAL_MODEL_PICK,
            subslice(&st.sheet_title, 0, tlen),
            subslice(&st.sheet_buf, 0, len),
            st.dev_model_idx.min(2),
            true,
        );
    } else {
        let mut raw_n = MaybeUninit::<[u8; 16]>::uninit();
        let num = unsafe { &mut *raw_n.as_mut_ptr() };
        match st.arg0 {
            MODAL_TEXT_OWNER => api.prompt_text(MODAL_TEXT_OWNER, b"Owner", trimmed_slice(&st.dev_owner), 12),
            MODAL_TEXT_TAG => api.prompt_text(MODAL_TEXT_TAG, b"Tag", trimmed_slice(&st.dev_tag), 8),
            MODAL_TEXT_RADIO_ID => {
                let s = format_u16_into(st.dev_radio_id as u16, 0, b"", num);
                api.prompt_text(MODAL_TEXT_RADIO_ID, b"Radio ID", s, 8);
            }
            _ => api.prompt_text(MODAL_TEXT_MODEL, b"Model", trimmed_slice(&st.dev_model), 12),
        }
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
