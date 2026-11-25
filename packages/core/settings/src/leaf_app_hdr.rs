//! P0 Leaf 2: Render Package Detail Hero Header (borderless icon, Title + Type Pill,
//! Version + Size + Author subtitle, dashed divider) then chain to `leaf_app_body`.
#![no_std]
#![no_main]

mod shared;

use kern_sdk::{
    ALIGN_LEFT, COLOR_FILL, FONT_MUMATRIX, FONT_TOMTHUMB, KRX_LEAF_KIND_CALL_LEAF, KrxApiTable,
    KrxLeafContext, div_rem_10_u16, icon, krx_entry,
};
use shared::{LEAF_APP_BODY, SharedState, copy_bytes, set_byte, subslice};

#[inline(never)]
fn push_u16(buf: &mut [u8], mut pos: usize, mut val: u16, dec: u8) -> usize {
    let mut tmp = [0u8; 6];
    let mut n = 0usize;
    while n < 6 {
        let (q, r) = div_rem_10_u16(val);
        set_byte(&mut tmp, n, b'0' + r);
        n += 1;
        val = q;
        if val == 0 && n > (dec as usize) {
            break;
        }
    }
    while n > 0 && pos < buf.len() {
        if dec > 0 && n == (dec as usize) {
            set_byte(buf, pos, b'.');
            pos += 1;
        }
        n -= 1;
        set_byte(buf, pos, shared::byte_at(&tmp, n));
        pos += 1;
    }
    pos
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let hy = 10i16;
    let m = unsafe { &*(&st.detail_sc.app_meta as *const shared::AppMeta) };
    let buf = shared::subslice_mut(&mut st.sheet_buf, 0, 32);

    // Borderless icon (16x16 package asset or native 8x8 icon centered cleanly)
    let text_x: i16 = if m.has_icon16 != 0 {
        api.draw_bitmap(4, hy, 16, 16, &m.icon_16);
        24
    } else {
        let ic = if m.icon_id != 0 {
            m.icon_id
        } else if m.exec_type <= 1 {
            icon::APPS
        } else if m.exec_type >= 4 {
            icon::DEVICE
        } else {
            icon::SYSTEM
        };
        api.draw_icon(5, hy + 4, ic, COLOR_FILL);
        17
    };

    // Title + compact Type pill on top-right
    api.draw_text(
        text_x,
        hy + 7,
        subslice(&m.name, 0, m.name_len as usize),
        FONT_MUMATRIX,
        COLOR_FILL,
        ALIGN_LEFT,
    );
    let tb: &[u8] = match m.exec_type {
        2 => b"SVC",
        3 => b"DMN",
        4 => b"DRV",
        5 => b"MOD",
        _ => b"APP",
    };
    api.draw_tag_badge(106, hy, tb, true);

    // Subtitle: `v1.0.0  27.0K  Author`
    set_byte(buf, 0, b'v');
    let mut vp = push_u16(buf, 1, m.ver_major as u16, 0);
    set_byte(buf, vp, b'.');
    vp = push_u16(buf, vp + 1, m.ver_minor as u16, 0);
    set_byte(buf, vp, b'.');
    vp = push_u16(buf, vp + 1, m.ver_patch as u16, 0);
    vp = copy_bytes(buf, vp, b"  ");
    let is_b = m.size < 1024;
    let v: u16 = if is_b {
        m.size as u16
    } else {
        ((m.size >> 10) as u16) * 10 + ((((m.size & 1023) * 10) >> 10) as u16)
    };
    vp = push_u16(buf, vp, v, u8::from(!is_b));
    set_byte(buf, vp, if is_b { b'B' } else { b'K' });
    vp += 1;
    if m.author_len > 0 && vp + 2 < 32 {
        vp = copy_bytes(buf, vp, b" ");
        vp = copy_bytes(buf, vp, subslice(&m.author, 0, m.author_len as usize));
    }
    api.draw_text(text_x, hy + 15, subslice(buf, 0, vp), FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);

    // Clean dashed horizontal divider
    let mut x = 4i16;
    while x < 124 {
        api.fill_rect(x, hy + 18, 2, 1, COLOR_FILL);
        x += 4;
    }

    ctx.continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
    ctx.continuation.target_or_code = LEAF_APP_BODY;
    ctx.continuation.payload = ctx.arg;
    0
}

krx_entry!(leaf_entry);
