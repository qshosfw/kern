//! P0 Leaf 23: Passcode PIN Verify / New / Confirm Text Modal Handler (`Op::SysSec`).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, SEC_FLAG_DIGITS_ONLY, SEC_FLAG_IS_SET,
    TEXT_FLAG_DIGITS_ONLY, TEXT_FLAG_PASSCODE, krx_entry,
};
use shared::{
    CMD_SEC_NEW_PIN, CMD_SEC_VERIFY, MODAL_SEC_ACTION, MODAL_SEC_CONFIRM_PIN, SharedState, byte_at,
    copy_bytes, fill_bytes, subslice, subslice_mut,
};

#[inline(never)]
fn toast(api: &KrxApiTable, msg: &[u8]) {
    api.show_toast(msg, 1200);
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    st.result = 0;

    let n = api.get_modal_text(subslice_mut(&mut st.sheet_buf, 0, 16));
    if st.cmd == CMD_SEC_VERIFY {
        let res = api.sec_verify(subslice(&st.sheet_buf, 0, n));
        st.sec_status = api.sec_status();
        if res == 0 {
            let tlen = copy_bytes(&mut st.sheet_title, 0, b"Passcode");
            let olen = copy_bytes(&mut st.sheet_buf, 0, b"Change\0Disable\0");
            api.show_sheet(
                MODAL_SEC_ACTION,
                subslice(&st.sheet_title, 0, tlen),
                subslice(&st.sheet_buf, 0, olen),
                0,
                false,
            );
            ctx.continuation.kind = KRX_LEAF_KIND_DONE;
            return 0;
        } else {
            toast(api, b"Wrong Passcode");
        }
    } else if st.cmd == CMD_SEC_NEW_PIN {
        if n < 4 {
            toast(api, b"Min 4 chars");
        } else {
            let pin = unsafe { core::slice::from_raw_parts(st.sheet_buf.as_ptr(), n) };
            copy_bytes(&mut st.sec_mut().pin, 0, pin);
            st.sec_mut().pin_len = n as u8;
            let df = if (st.sec_status & SEC_FLAG_IS_SET) == 0 || (st.sec_status & SEC_FLAG_DIGITS_ONLY) != 0 {
                TEXT_FLAG_DIGITS_ONLY
            } else {
                0
            };
            api.prompt_text_flags(MODAL_SEC_CONFIRM_PIN, b"Confirm PIN", b"", 16, TEXT_FLAG_PASSCODE | df);
        }
    } else {
        let plen = st.sec().pin_len as usize;
        let mut eq = n == plen && n >= 4;
        let mut all_digits = true;
        let mut i = 0usize;
        while i < n {
            let b = byte_at(&st.sheet_buf, i);
            if b < b'0' || b > b'9' {
                all_digits = false;
            }
            if eq && b != byte_at(&st.sec().pin, i) {
                eq = false;
            }
            i += 1;
        }
        if eq {
            let was_set = (st.sec_status & SEC_FLAG_IS_SET) != 0;
            let mut opts = if was_set { (st.sec_status as u8) & 0x0E } else { SEC_FLAG_DIGITS_ONLY as u8 };
            if !all_digits {
                opts &= !(SEC_FLAG_DIGITS_ONLY as u8);
            }
            let tries = if was_set { ((st.sec_status >> 16) & 0xFF) as u8 } else { 5 };
            let ok = api.sec_set_passcode(subslice(&st.sheet_buf, 0, n), opts, tries);
            st.sec_status = api.sec_status();
            toast(api, if ok { b"Passcode Saved" } else { b"Save Failed" });
        } else {
            toast(api, b"Mismatch!");
        }
        fill_bytes(&mut st.sec_mut().pin, 0);
        st.sec_mut().pin_len = 0;
    }
    fill_bytes(subslice_mut(&mut st.sheet_buf, 0, 16), 0);
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
