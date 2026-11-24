//! P0 Leaf 5: Open Native OptionSheet for Shortcut Scope or Target Slot.
//! Zero stack buffers & zero SPI Flash I/O: uses cached `SharedState::sc()` app list.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, krx_entry, nth_packed_slice};
use shared::{
    MODAL_SHORTCUT_PICK, MODAL_SHORTCUT_SCOPE, SharedState, copy_bytes, cstr_slice,
    get_slot_action, set_byte, subslice,
};

const SLOT_LABELS: &[u8] =
    b"S1\0S1+\0S2\0S2+\0M+\0F+1\0F+2\0F+3\0F+4\0F+5\0F+6\0F+7\0F+8\0F+9\0F+0\0F+*\0F+M\0F+S1\0F+S2\0";
const CTX_NAMES: &[u8] = b"Global\0VFO\0Home\0Settings\0Apps\0";
const BUILTIN_NAMES: &[u8] = b"None\0Flashlight\0Settings\0Lock Keypad\0Home\0Sleep\0";

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    if st.sc_sel == 0 {
        let len = copy_bytes(&mut st.sheet_buf, 0, CTX_NAMES);
        let tlen = copy_bytes(&mut st.sheet_title, 0, b"Scope");
        api.show_sheet(
            MODAL_SHORTCUT_SCOPE,
            subslice(&st.sheet_title, 0, tlen),
            subslice(&st.sheet_buf, 0, len),
            st.sc_scope,
            true,
        );
    } else if st.sc_sel >= 2 {
        let slot = st.sc_sel - 2;
        st.sc_slot = slot;
        let sc = unsafe { &*(core::ptr::addr_of!(st.detail_sc.sc)) };
        let cur_act = get_slot_action(&sc.keys_cfg, slot, st.sc_scope);
        let mut sel = if cur_act < 6 { cur_act as u8 } else { 0u8 };
        let mut pos = copy_bytes(&mut st.sheet_buf, 0, BUILTIN_NAMES);
        let app_n = (sc.app_n as usize).min(6);
        let mut i = 0usize;
        let mut tcnt = 6usize;
        while i < app_n {
            let name = cstr_slice(&sc.app_names[i]);
            if !name.is_empty() && pos + name.len() + 1 < st.sheet_buf.len() {
                if sc.app_hashes[i] == cur_act {
                    sel = tcnt as u8;
                }
                pos = copy_bytes(&mut st.sheet_buf, pos, name);
                set_byte(&mut st.sheet_buf, pos, 0);
                pos += 1;
                tcnt += 1;
            }
            i += 1;
        }
        st.sc_target_count = tcnt as u8;
        let tlen = copy_bytes(&mut st.sheet_title, 0, nth_packed_slice(SLOT_LABELS, slot));
        api.show_sheet(
            MODAL_SHORTCUT_PICK,
            subslice(&st.sheet_title, 0, tlen),
            subslice(&st.sheet_buf, 0, pos),
            sel,
            true,
        );
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
