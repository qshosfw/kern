//! P0 Leaf 2: Render App Detail Header (16x16 Icon Left, Name + Version + Author + Type Badge
//! Right, Divider, File Path & Size) then chain via P0 continuation to `leaf_app_body`.
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{
    ALIGN_LEFT, ALIGN_RIGHT, COLOR_FILL, FONT_MUMATRIX, FONT_TOMTHUMB, KRX_LEAF_KIND_CALL_LEAF,
    KrxApiTable, KrxLeafContext, div_rem_10_u16, icon, krx_entry,
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
    let st = unsafe { &*(ctx.arg as *const SharedState) };
    let m = st.meta();
    let hy = 10 - st.detail_scroll;

    let mut raw_b = MaybeUninit::<[u8; 24]>::uninit();
    let buf = unsafe { &mut *raw_b.as_mut_ptr() };

    api.draw_rect(3, hy, 20, 20, COLOR_FILL);
    if m.has_icon16 != 0 {
        api.draw_bitmap(5, hy + 2, 16, 16, &m.icon_16);
    } else {
        api.draw_icon(9, hy + 6, icon::APPS, COLOR_FILL);
    }

    api.draw_text(27, hy + 8, subslice(&m.name, 0, m.name_len as usize), FONT_MUMATRIX, COLOR_FILL, ALIGN_LEFT);

    set_byte(buf, 0, b'v');
    let mut vp = push_u16(buf, 1, m.ver_major as u16, 0);
    set_byte(buf, vp, b'.');
    vp = push_u16(buf, vp + 1, m.ver_minor as u16, 0);
    set_byte(buf, vp, b'.');
    vp = push_u16(buf, vp + 1, m.ver_patch as u16, 0);
    if m.author_len > 0 {
        set_byte(buf, vp, b' ');
        vp = copy_bytes(buf, vp + 1, subslice(&m.author, 0, m.author_len as usize));
    }
    api.draw_text(27, hy + 18, subslice(buf, 0, vp), FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);

    let tb: &[u8] = match m.exec_type {
        2 => b"SVC",
        3 => b"DMN",
        4 => b"DRV",
        5 => b"MOD",
        _ => b"APP",
    };
    api.draw_tag_badge(104, hy + 1, tb, true);
    api.draw_line(2, hy + 23, 122, hy + 23, COLOR_FILL);

    let y1 = hy + 27;
    api.draw_icon(3, y1, icon::DEVICE, COLOR_FILL);
    api.draw_text(14, y1 + 6, subslice(&m.path, 0, m.path_len as usize), FONT_TOMTHUMB, COLOR_FILL, ALIGN_LEFT);
    let is_b = m.size < 1024;
    let v: u16 = if is_b {
        m.size as u16
    } else {
        ((m.size >> 10) as u16) * 10 + ((((m.size & 1023) * 10) >> 10) as u16)
    };
    let mut sp = push_u16(buf, 0, v, u8::from(!is_b));
    set_byte(buf, sp, if is_b { b'B' } else { b'K' });
    sp += 1;
    api.draw_text(122, y1 + 6, subslice(buf, 0, sp), FONT_TOMTHUMB, COLOR_FILL, ALIGN_RIGHT);

    ctx.continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
    ctx.continuation.target_or_code = LEAF_APP_BODY;
    ctx.continuation.payload = ctx.arg;
    0
}

krx_entry!(leaf_entry);
