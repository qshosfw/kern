//! P0 Leaf 10: Device Identity Prompts (Model / Owner / Tag / Radio ID) & Battery Calib Tweak Save.
//! Zero stack buffers.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, TEXT_FLAG_DIGITS_ONLY, krx_entry};
use shared::{
    CMD_DEV_BATT_TWEAK_SAVE, CMD_DEV_MODEL_SHEET, MODAL_MODEL_PICK, MODAL_TEXT_MODEL,
    MODAL_TEXT_OWNER, MODAL_TEXT_RADIO_ID, MODAL_TEXT_TAG, SharedState, copy_bytes, cstr_slice,
    set_byte, subslice, subslice_mut,
};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    match st.cmd {
        CMD_DEV_MODEL_SHEET => {
            let len = copy_bytes(&mut st.sheet_buf, 0, b"UV-K1\0UV-K5 V3\0Custom...\0");
            let tlen = copy_bytes(&mut st.sheet_title, 0, b"Model");
            api.show_sheet(
                MODAL_MODEL_PICK,
                subslice(&st.sheet_title, 0, tlen),
                subslice(&st.sheet_buf, 0, len),
                st.dev_model_idx.min(2),
                true,
            );
        }
        CMD_DEV_BATT_TWEAK_SAVE => {
            let cfg = subslice_mut(&mut st.sheet_buf, 0, 20);
            if api.read_file(b"/pref/battery.cfg", cfg) >= 20 {
                let mv = st.batt_tweak_mv.clamp(-500, 500).to_le_bytes();
                set_byte(cfg, 6, mv[0]);
                set_byte(cfg, 7, mv[1]);
                let crc = api.crc32(subslice(cfg, 0, 16)).to_le_bytes();
                copy_bytes(cfg, 16, &crc);
                if api.write_file(b"/pref/battery.cfg", cfg) {
                    let _ = api.reload_config();
                }
            }
        }
        _ => match st.arg0 {
            MODAL_TEXT_OWNER => {
                api.prompt_text(MODAL_TEXT_OWNER, b"Owner", cstr_slice(&st.str_fields[5]), 12)
            }
            MODAL_TEXT_TAG => {
                api.prompt_text(MODAL_TEXT_TAG, b"Tag", cstr_slice(&st.str_fields[6]), 8)
            }
            MODAL_TEXT_RADIO_ID => api.prompt_text_flags(
                MODAL_TEXT_RADIO_ID,
                b"Radio ID",
                cstr_slice(&st.str_fields[7]),
                8,
                TEXT_FLAG_DIGITS_ONLY,
            ),
            _ => api.prompt_text(MODAL_TEXT_MODEL, b"Model", cstr_slice(&st.str_fields[4]), 12),
        },
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
