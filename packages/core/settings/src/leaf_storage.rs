//! P0 Leaf 18: KernFS Storage & Flash Health Telemetry (KiB, %, Progress Bar, Sectors, Wear).
#![no_std]
#![no_main]

mod shared;

use core::mem::MaybeUninit;
use kern_sdk::{
    COLOR_CLEAR, COLOR_FILL, KRX_LEAF_KIND_DONE, KRX_WIDGET_PROGRESS_BAR, KrxApiTable, KrxFsReq,
    KrxLeafContext, KrxWidget, Op, krx_entry,
};
use shared::{SharedState, byte_at, set_byte, subslice};

#[inline(never)]
fn put_s(dst: &mut [u8], mut pos: usize, s: &[u8]) -> usize {
    let mut i = 0usize;
    while i < s.len() && pos < dst.len() {
        set_byte(dst, pos, byte_at(s, i));
        pos += 1;
        i += 1;
    }
    pos
}

#[inline(never)]
fn put_u16(dst: &mut [u8], mut pos: usize, mut v: u16, sfx: &[u8]) -> usize {
    let mut rev_u = MaybeUninit::<[u8; 6]>::uninit();
    let rev = unsafe { &mut *rev_u.as_mut_ptr() };
    let mut n = 0usize;
    loop {
        let mut q = 0u16;
        while v >= 10 {
            v = core::hint::black_box(v - 10);
            q += 1;
        }
        set_byte(rev, n, b'0' + v as u8);
        n += 1;
        v = q;
        if v == 0 || n >= 6 {
            break;
        }
    }
    while n > 0 && pos < dst.len() {
        n -= 1;
        set_byte(dst, pos, byte_at(rev, n));
        pos += 1;
    }
    put_s(dst, pos, sfx)
}

#[inline(never)]
fn draw_line(api: &KrxApiTable, y: i16, s: &[u8]) {
    api.draw_text(4, y, s, 0, COLOR_FILL, 0);
}

fn u16_le(b: &[u8; 16], o: usize) -> u16 {
    unsafe { u16::from_le_bytes([*b.get_unchecked(o), *b.get_unchecked(o + 1)]) }
}

fn leaf_entry(_op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let _st = unsafe { &*(ctx.arg as *const SharedState) };

    let mut raw_u = MaybeUninit::<[u8; 16]>::uninit();
    let raw = unsafe { &mut *raw_u.as_mut_ptr() };
    let req = KrxFsReq {
        path: core::ptr::null(),
        buf: raw.as_mut_ptr(),
        offset: 0,
        len: 16,
        path_len: 0,
        extra: 0,
    };
    if api.call_op(Op::FsRead, 5, &raw const req as usize, 0) < 16 {
        draw_line(api, 28, b"Storage unavailable");
    } else {
        let total = u16_le(raw, 0);
        let used = u16_le(raw, 2);
        let free = u16_le(raw, 4);
        let recl = u16_le(raw, 6);
        let files = u16_le(raw, 8);
        let wear = u32::from_le_bytes([raw[12], raw[13], raw[14], raw[15]]).min(65535) as u16;

        let mut pct = 0u8;
        if total > 0 {
            let mut num = (used as u32) * 100;
            let den = total as u32;
            while num >= den && pct < 100 {
                num = core::hint::black_box(num - den);
                pct += 1;
            }
        }

        let mut ln_u = MaybeUninit::<[u8; 32]>::uninit();
        let ln = unsafe { &mut *ln_u.as_mut_ptr() };
        let p = put_u16(ln, 0, used << 2, b"/");
        let p = put_u16(ln, p, total << 2, b" KiB (");
        let p = put_u16(ln, p, pct as u16, b"%)");
        draw_line(api, 15, subslice(ln, 0, p));

        let bar = KrxWidget {
            p0: core::ptr::null(), p1: core::ptr::null(),
            x: 4, y: 18, len0: 0, len1: 0, extra: 0,
            w: 120, h: 7, a: pct, b: 0, _pad: [0; 2],
        };
        let _ = api.call_op(Op::UiWidget, KRX_WIDGET_PROGRESS_BAR as usize, &raw const bar as usize, 0);

        let p = put_s(ln, 0, b"Free: ");
        let p = put_u16(ln, p, free.saturating_add(recl) << 2, b" KiB  Files: ");
        let p = put_u16(ln, p, files, b"");
        draw_line(api, 34, subslice(ln, 0, p));

        let p = put_s(ln, 0, b"Sec U:");
        let p = put_u16(ln, p, used, b" F:");
        let p = put_u16(ln, p, free, b" R:");
        let p = put_u16(ln, p, recl, b"");
        draw_line(api, 45, subslice(ln, 0, p));

        let p = put_s(ln, 0, b"Flash Wear: ");
        let p = put_u16(ln, p, wear, b" erases");
        draw_line(api, 56, subslice(ln, 0, p));
    }

    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

krx_entry!(leaf_entry);
