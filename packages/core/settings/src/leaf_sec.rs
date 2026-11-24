//! P0 Leaf 18: Security Passcode Open & Action Sheet Handler (`Op::SysSec`).
//! Zero stack buffers.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, SEC_FLAG_DIGITS_ONLY, SEC_FLAG_EXPOSE_LEN,
    SEC_FLAG_IS_SET, TEXT_FLAG_DIGITS_ONLY, TEXT_FLAG_EXPOSE_LEN, TEXT_FLAG_PASSCODE, krx_entry,
};
use shared::{CMD_SEC_OPEN, MODAL_SEC_NEW_PIN, MODAL_SEC_VERIFY, SharedState};

#[inline(always)]
fn digits_flag(status: u32) -> u8 {
    if (status & SEC_FLAG_IS_SET) == 0 || (status & SEC_FLAG_DIGITS_ONLY) != 0 {
        TEXT_FLAG_DIGITS_ONLY
    } else {
        0
    }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    st.result = 0;

    if st.cmd == CMD_SEC_OPEN {
        let s = api.sec_status();
        st.sec_status = s;
        if (s & SEC_FLAG_IS_SET) == 0 {
            api.prompt_text_flags(
                MODAL_SEC_NEW_PIN,
                b"New Passcode",
                b"",
                16,
                TEXT_FLAG_PASSCODE | TEXT_FLAG_DIGITS_ONLY,
            );
        } else {
            let mut f = TEXT_FLAG_PASSCODE | digits_flag(s);
            let mut max_len = 16u8;
            if (s & SEC_FLAG_EXPOSE_LEN) != 0 {
                f |= TEXT_FLAG_EXPOSE_LEN;
                let plen = ((s >> 8) & 0xFF) as u8;
                if plen > 0 && plen <= 16 {
                    max_len = plen;
                }
            }
            api.prompt_text_flags(MODAL_SEC_VERIFY, b"Enter Passcode", b"", max_len, f);
        }
    } else if st.arg0 == 0 {
        api.prompt_text_flags(
            MODAL_SEC_NEW_PIN,
            b"New Passcode",
            b"",
            16,
            TEXT_FLAG_PASSCODE | digits_flag(st.sec_status),
        );
    } else {
        let ok = api.sec_set_passcode(b"", 0, 5);
        st.sec_status = api.sec_status();
        st.result = u8::from(ok);
        api.show_toast(if ok { b"Passcode Off" } else { b"Failed" }, 1200);
    }

    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
