//! P0 Leaf 3: Render Package Detail Body (aligned Path + Profile, Capabilities,
//! and immediately visible `[Launch]` / `[Remove]` Capsule Buttons).
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    ALIGN_LEFT, ALIGN_RIGHT, COLOR_CLEAR, COLOR_FILL, FONT_TOMTHUMB, KRX_LEAF_KIND_DONE,
    KrxApiTable, KrxLeafContext, krx_entry, nth_packed_slice,
};
use shared::{SharedState, copy_bytes, set_byte, subslice};

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let detail_btn = st.detail_btn;
    let m = unsafe { &*(&st.detail_sc.app_meta as *const shared::AppMeta) };
    let buf = shared::subslice_mut(&mut st.sheet_buf, 0, 32);

    // Row 1 (y = 35): File path on left, execution profile right-aligned
    api.draw_text(
        4,
        36,
        subslice(&m.path, 0, m.path_len as usize),
        FONT_TOMTHUMB,
        COLOR_FILL,
        ALIGN_LEFT,
    );
    let prof = nth_packed_slice(
        b"N0 Fixed\0P0 Paged\0N1 PIC\0P1 Paged\0",
        m.profile.wrapping_sub(0x10).min(3),
    );
    api.draw_text(124, 36, prof, FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    // Row 2 (y = 44): Capabilities summary (or declared Action if present)
    const CAP_NAMES: &[u8] = b"RF\0Aud\0Disp\0Key\0Pwr\0Stor\0Ser\0Pref\0Task\0";
    const CAP_BITS: [u8; 9] = [0, 3, 4, 5, 6, 8, 10, 18, 25];
    let mut cp = copy_bytes(buf, 0, b"Caps: ");
    let cap_start = cp;
    let mut ci = 0usize;
    while ci < 9 {
        let bit = unsafe { *CAP_BITS.get_unchecked(ci) };
        if (m.caps & (1u32 << bit)) != 0 {
            if cp > cap_start && cp + 1 < 31 {
                set_byte(buf, cp, b',');
                cp += 1;
            }
            cp = copy_bytes(buf, cp, nth_packed_slice(CAP_NAMES, ci as u8));
        }
        ci += 1;
    }
    if cp == cap_start {
        cp = copy_bytes(buf, cap_start, b"Sandboxed");
    }
    api.draw_text(4, 44, subslice(buf, 0, cp), FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);

    // Interactive Action Capsule Buttons immediately visible at bottom (`y = 49..61`)
    let launch_lbl: &[u8] = if m.exec_type >= 2 { b"Run" } else { b"Launch" };
    api.draw_capsule_button(4, 49, 58, 12, launch_lbl, detail_btn == 0);
    api.draw_capsule_button(66, 49, 58, 12, b"Remove", detail_btn == 1);

    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);

    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
