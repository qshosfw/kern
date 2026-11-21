#![no_std]
#![no_main]

use kern_sdk::{
    ALIGN_CENTER, COLOR_FILL, FONT_TOMTHUMB, KRX_OP_EVENT, KRX_OP_RENDER, KRX_OP_START,
    KrxApiTable, KrxEvent, div_rem_10, krx_entry, prop, rel_strings,
};

rel_strings!(static LABELS: [2; 34] = [b"HELLO FROM RUST", b"MENU: Read Contrast"];);

static mut CONTRAST: u8 = 0;

fn handle(op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    match op {
        KRX_OP_START => {
            unsafe {
                CONTRAST = api.prop_get(prop::CONTRAST);
            }
            0
        }
        KRX_OP_EVENT => {
            if !arg.is_null() {
                let ev = unsafe { &*(arg as *const KrxEvent) };
                if ev.kind == kern_sdk::KRX_EV_KEY && ev.flags == 0 {
                    unsafe {
                        CONTRAST = api.prop_get(prop::CONTRAST);
                    }
                    return 1;
                }
            }
            0
        }
        KRX_OP_RENDER => {
            api.clear();
            api.draw_rect(4, 10, 120, 48, COLOR_FILL);
            api.draw_text(64, 24, LABELS.get(0), FONT_TOMTHUMB, COLOR_FILL, ALIGN_CENTER);
            api.draw_text(64, 38, LABELS.get(1), FONT_TOMTHUMB, COLOR_FILL, ALIGN_CENTER);
            let c = unsafe { CONTRAST };
            let (tens_all, ones) = div_rem_10(c);
            let (_, tens) = div_rem_10(tens_all);
            let digits = [b'0' + tens, b'0' + ones, b'%'];
            api.draw_text(64, 50, &digits, FONT_TOMTHUMB, COLOR_FILL, ALIGN_CENTER);
            api.commit();
            0
        }
        _ => 0,
    }
}

krx_entry!(handle);
