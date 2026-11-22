//! P0 Leaf 5: Open Native OptionSheet for Shortcut Scope or Target Slot.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{
    KRX_LEAF_KIND_DONE, KrxApiTable, KrxDirEntry, KrxLeafContext, krx_entry, nth_packed_slice,
};
use shared::{
    MODAL_SHORTCUT_PICK, MODAL_SHORTCUT_SCOPE, SharedState, copy_bytes, cstr_slice, set_byte,
    strip_apps_prefix, subslice,
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
        api.show_sheet(MODAL_SHORTCUT_SCOPE, subslice(&st.sheet_title, 0, tlen), subslice(&st.sheet_buf, 0, len), st.sc_scope, true);
    } else if st.sc_sel >= 2 {
        let slot = st.sc_sel - 2;
        st.sc_slot = slot;
        let mut pos = copy_bytes(&mut st.sheet_buf, 0, BUILTIN_NAMES);
        let mut tcnt = 6usize;
        let mut raw_e = MaybeUninit::<KrxDirEntry>::uninit();
        let ent = unsafe { &mut *raw_e.as_mut_ptr() };
        let mut off = 0u32;
        while api.list_dir_page(b"/apps", off, core::slice::from_mut(ent)) > 0 {
            off += 1;
            let short = strip_apps_prefix(cstr_slice(&ent.name));
            if pos + short.len() + 1 < st.sheet_buf.len() {
                pos = copy_bytes(&mut st.sheet_buf, pos, short);
                set_byte(&mut st.sheet_buf, pos, 0);
                pos += 1;
                tcnt += 1;
            }
        }
        st.sc_target_count = tcnt.min(255) as u8;
        let tlen = copy_bytes(&mut st.sheet_title, 0, nth_packed_slice(SLOT_LABELS, slot));
        api.show_sheet(MODAL_SHORTCUT_PICK, subslice(&st.sheet_title, 0, tlen), subslice(&st.sheet_buf, 0, pos), 0, true);
    }
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
