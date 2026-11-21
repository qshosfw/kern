//! P0 Leaf 3: Render App Detail Scrollable Body (Profile & Services/Actions, Capabilities,
//! Interactive `[Launch]` / `[Remove]` Capsule Buttons, Status Bar Clip & Scrollbar).
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{
    ALIGN_LEFT, COLOR_CLEAR, COLOR_FILL, FONT_TOMTHUMB, KRX_LEAF_KIND_DONE, KrxApiTable,
    KrxLeafContext, icon, krx_entry, nth_packed_slice,
};
use shared::{SharedState, copy_bytes, set_byte, subslice};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &*(ctx.arg as *const SharedState) };
    let m = st.meta();
    let sy = st.detail_scroll;
    let hy = 10 - sy;

    let mut raw_b = MaybeUninit::<[u8; 24]>::uninit();
    let buf = unsafe { &mut *raw_b.as_mut_ptr() };

    // Profile & Declared Service/Action
    let y2 = hy + 38;
    api.draw_icon(3, y2, icon::APPS, COLOR_FILL);
    let prof = nth_packed_slice(
        b"N0 Fixed\0P0 Paged\0N1 PIC\0P1 Paged\0",
        m.profile.wrapping_sub(0x10).min(3),
    );
    let mut sp = copy_bytes(buf, 0, prof);
    if m.action_len > 0 {
        sp = copy_bytes(buf, sp, b" | ");
        sp = copy_bytes(buf, sp, subslice(&m.action_label, 0, m.action_len as usize));
    }
    api.draw_text(14, y2 + 6, subslice(buf, 0, sp), FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);

    // Capabilities
    let y3 = y2 + 11;
    api.draw_icon(3, y3, icon::SYSTEM, COLOR_FILL);
    const CAP_NAMES: &[u8] = b"RF\0Aud\0Disp\0Key\0Pwr\0Stor\0Ser\0Pref\0Task\0";
    const CAP_BITS: [u8; 9] = [0, 3, 4, 5, 6, 8, 10, 18, 25];
    let mut cp = 0usize;
    let mut ci = 0usize;
    while ci < 9 {
        let bit = unsafe { *CAP_BITS.get_unchecked(ci) };
        if (m.caps & (1u32 << bit)) != 0 {
            if cp > 0 && cp + 1 < 24 {
                set_byte(buf, cp, b',');
                cp += 1;
            }
            cp = copy_bytes(buf, cp, nth_packed_slice(CAP_NAMES, ci as u8));
        }
        ci += 1;
    }
    if cp == 0 {
        cp = copy_bytes(buf, 0, b"Sandboxed");
    }
    api.draw_text(14, y3 + 6, subslice(buf, 0, cp), FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);

    // Interactive Manage Buttons
    let yb = y3 + 13;
    api.draw_capsule_button(4, yb, 56, 12, b"Launch", st.detail_btn == 0);
    api.draw_capsule_button(64, yb, 56, 12, b"Remove", st.detail_btn == 1);

    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    api.draw_scrollbar(125, 9, 52, sy.max(0) as u16, 88, 54);

    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
