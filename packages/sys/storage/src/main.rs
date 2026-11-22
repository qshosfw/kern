//! Kern Storage & Flash Health Manager (`storage.mod` — Declarative System Module).
//!
//! Displays live KernFS capacity (KiB, %, progress bar), sector distribution graph
//! (Used / Reclaimable / Free), flash wear cycles & health gauge, and Hold-to-confirm
//! safe user-data format (`Op::FsMeta(FORMAT, 0)` preserving factory calibration).
#![no_std]
#![no_main]

use core::mem::MaybeUninit;
use kern_sdk::{
    COLOR_CLEAR, COLOR_FILL, KRX_EV_ENCODER, KRX_EV_KEY, KRX_EV_MODAL, KRX_OP_EVENT, KRX_OP_EXIT,
    KRX_OP_RENDER, KRX_OP_START, KRX_STATUS_CUSTOM_TITLE, KRX_STATUS_DIVIDER, KRX_STATUS_VISIBLE,
    KRX_WIDGET_KEY_BADGE, KRX_WIDGET_PROGRESS_BAR, KrxApiTable, KrxEvent, KrxFsReq, KrxStatusBar,
    KrxWidget, MODAL_RESULT_CONFIRMED, Op, key, krx_entry,
};

const MODAL_FORMAT: u8 = 51;

#[derive(Clone, Copy)]
struct FsStats {
    ok: bool,
    total: u16,
    used: u16,
    free: u16,
    recl: u16,
    files: u16,
    wear: u32,
}

impl FsStats {
    const fn empty() -> Self {
        Self {
            ok: false,
            total: 0,
            used: 0,
            free: 0,
            recl: 0,
            files: 0,
            wear: 0,
        }
    }
}

struct StorageState {
    page: u8,
    stats: FsStats,
}

impl StorageState {
    const fn new() -> Self {
        Self {
            page: 0,
            stats: FsStats::empty(),
        }
    }

    #[inline(never)]
    fn refresh(&mut self, api: &KrxApiTable) {
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
        if api.call_op(Op::FsRead, 5, &raw const req as usize, 0) >= 16 {
            let u16_at = |o: usize| unsafe {
                u16::from_le_bytes([*raw.get_unchecked(o), *raw.get_unchecked(o + 1)])
            };
            self.stats = FsStats {
                ok: true,
                total: u16_at(0),
                used: u16_at(2),
                free: u16_at(4),
                recl: u16_at(6),
                files: u16_at(8),
                wear: unsafe {
                    u32::from_le_bytes([
                        *raw.get_unchecked(12),
                        *raw.get_unchecked(13),
                        *raw.get_unchecked(14),
                        *raw.get_unchecked(15),
                    ])
                },
            };
        } else {
            self.stats.ok = false;
        }
    }

    #[inline(never)]
    fn set_title(&self, api: &KrxApiTable) {
        let mut words = [0u32; 16];
        let ptr = words.as_mut_ptr() as *mut u8;
        let title: &[u8] = if self.page == 0 {
            b"Storage (1/2)"
        } else {
            b"Flash Health 2/2"
        };
        unsafe {
            *ptr = KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER | KRX_STATUS_CUSTOM_TITLE;
            let mut i = 0usize;
            while i < title.len() {
                *ptr.add(12 + i) = *title.get_unchecked(i);
                i += 1;
            }
            api.set_status_bar(&*(ptr as *const KrxStatusBar));
        }
    }

    #[inline(never)]
    fn render(&self, api: &KrxApiTable) {
        api.clear();
        if !self.stats.ok {
            api.draw_text(4, 28, b"Storage unavailable", 0, COLOR_FILL, 0);
            api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
            return;
        }
        let s = self.stats;
        let mut ln_u = MaybeUninit::<[u8; 36]>::uninit();
        let ln = unsafe { &mut *ln_u.as_mut_ptr() };

        if self.page == 0 {
            let pct = div_pct(s.used as u32, s.total as u32);
            let p = put_u32(ln, 0, (s.used as u32) << 2, b"/");
            let p = put_u32(ln, p, (s.total as u32) << 2, b" KiB (");
            let p = put_u32(ln, p, pct as u32, b"%)");
            api.draw_text(4, 15, subslice(ln, 0, p), 0, COLOR_FILL, 0);

            draw_bar(api, 4, 18, 120, 7, pct);

            let p = put_s(ln, 0, b"Free: ");
            let p = put_u32(ln, p, (s.free.saturating_add(s.recl) as u32) << 2, b" KiB  Files: ");
            let p = put_u32(ln, p, s.files as u32, b"");
            api.draw_text(4, 32, subslice(ln, 0, p), 0, COLOR_FILL, 0);

            let p = put_s(ln, 0, b"Sec U:");
            let p = put_u32(ln, p, s.used as u32, b" F:");
            let p = put_u32(ln, p, s.free as u32, b" R:");
            let p = put_u32(ln, p, s.recl as u32, b"");
            api.draw_text(4, 41, subslice(ln, 0, p), 0, COLOR_FILL, 0);

            // Visual 120px sector distribution graph: solid=used, dither=reclaimable, empty=free
            api.draw_rect(4, 45, 120, 7, COLOR_FILL);
            if s.total > 0 {
                let u_px = scale_px(s.used as u32, s.total as u32, 118);
                let r_px = scale_px(s.recl as u32, s.total as u32, 118).min(118 - u_px);
                if u_px > 0 {
                    api.fill_rect(5, 46, u_px, 5, COLOR_FILL);
                }
                if r_px > 0 {
                    let mut x = 5 + u_px as i16;
                    let end = x + r_px as i16;
                    while x < end {
                        api.fill_rect(x, 46, 1, 5, COLOR_FILL);
                        x += 2;
                    }
                }
            }
            draw_key_badge(api, 4, 56, b"v", b"Health");
        } else {
            // Page 1: Flash Wear & Endurance + Hold-to-confirm Format
            let wear_pct = div_pct(s.wear, 100_000);
            let health = 100u8.saturating_sub(wear_pct);
            let p = put_s(ln, 0, b"Flash Health: ");
            let p = put_u32(ln, p, health as u32, b"%");
            api.draw_text(4, 15, subslice(ln, 0, p), 0, COLOR_FILL, 0);

            draw_bar(api, 4, 18, 120, 7, health);

            let p = put_s(ln, 0, b"Erase Cycles: ");
            let p = put_u32(ln, p, s.wear, b"");
            api.draw_text(4, 33, subslice(ln, 0, p), 0, COLOR_FILL, 0);

            let p = put_s(ln, 0, b"Sectors: ");
            let p = put_u32(ln, p, s.total as u32, b" x 4 KiB");
            api.draw_text(4, 43, subslice(ln, 0, p), 0, COLOR_FILL, 0);

            draw_key_badge(api, 4, 56, b"^", b"Usage");
            draw_key_badge(api, 64, 56, b"M", b"Format");
        }
        api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    }
}

#[inline(never)]
fn draw_bar(api: &KrxApiTable, x: i16, y: i16, w: u8, h: u8, pct: u8) {
    let bar = KrxWidget {
        p0: core::ptr::null(),
        p1: core::ptr::null(),
        x,
        y,
        len0: 0,
        len1: 0,
        extra: 0,
        w,
        h,
        a: pct,
        b: 0,
        _pad: [0; 2],
    };
    let _ = api.call_op(Op::UiWidget, KRX_WIDGET_PROGRESS_BAR as usize, &raw const bar as usize, 0);
}

#[inline(never)]
fn draw_key_badge(api: &KrxApiTable, x: i16, y: i16, key_s: &[u8], lbl: &[u8]) {
    let d = KrxWidget {
        p0: key_s.as_ptr(),
        p1: lbl.as_ptr(),
        x,
        y,
        len0: key_s.len() as u16,
        len1: lbl.len() as u16,
        extra: 0,
        w: 0,
        h: 0,
        a: 0,
        b: 0,
        _pad: [0; 2],
    };
    let _ = api.call_op(Op::UiWidget, KRX_WIDGET_KEY_BADGE as usize, &raw const d as usize, 0);
}

fn scale_px(num: u32, den: u32, max_px: u8) -> u8 {
    if den == 0 || num == 0 {
        return 0;
    }
    let mut rem = num.saturating_mul(max_px as u32);
    let mut q = 0u8;
    while rem >= den && q < max_px {
        rem = core::hint::black_box(rem - den);
        q += 1;
    }
    q
}

fn div_pct(num: u32, den: u32) -> u8 {
    scale_px(num, den, 100)
}

#[inline(never)]
fn put_s(dst: &mut [u8], mut pos: usize, s: &[u8]) -> usize {
    let mut i = 0usize;
    while i < s.len() && pos < dst.len() {
        unsafe { *dst.get_unchecked_mut(pos) = *s.get_unchecked(i) };
        pos += 1;
        i += 1;
    }
    pos
}

#[inline(never)]
fn put_u32(dst: &mut [u8], mut pos: usize, mut v: u32, sfx: &[u8]) -> usize {
    let mut rev_u = MaybeUninit::<[u8; 10]>::uninit();
    let rev = unsafe { &mut *rev_u.as_mut_ptr() };
    let mut n = 0usize;
    loop {
        let mut rem = 0u32;
        let q = api_div10(v, &mut rem);
        unsafe { *rev.get_unchecked_mut(n) = b'0' + rem as u8 };
        n += 1;
        v = q;
        if v == 0 || n >= 10 {
            break;
        }
    }
    while n > 0 && pos < dst.len() {
        n -= 1;
        unsafe { *dst.get_unchecked_mut(pos) = *rev.get_unchecked(n) };
        pos += 1;
    }
    put_s(dst, pos, sfx)
}

#[inline(always)]
fn api_div10(v: u32, rem: &mut u32) -> u32 {
    // Fast reciprocal division by 10 avoiding __aeabi_uidivmod in PIC .mod
    let q = (((v as u64) * 0xCCCC_CCCDu64) >> 35) as u32;
    *rem = v - q * 10;
    q
}

#[inline(always)]
fn subslice(buf: &[u8], start: usize, end: usize) -> &[u8] {
    let len = end.saturating_sub(start).min(buf.len().saturating_sub(start));
    unsafe { core::slice::from_raw_parts(buf.as_ptr().add(start), len) }
}

static mut STATE: StorageState = StorageState::new();

fn app_entry(op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    let st = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    match op {
        KRX_OP_START => {
            st.page = 0;
            st.refresh(api);
            st.set_title(api);
            1
        }
        KRX_OP_EVENT => {
            if arg.is_null() {
                return 0;
            }
            let ev = unsafe { &*(arg as *const KrxEvent) };
            match ev.kind {
                KRX_EV_ENCODER if ev.payload != 0 => {
                    st.page ^= 1;
                    st.set_title(api);
                    1
                }
                KRX_EV_KEY if ev.payload == key::STATE_PRESSED => match ev.key {
                    key::UP | key::DOWN | key::KEY2 | key::KEY8 => {
                        st.page ^= 1;
                        st.set_title(api);
                        1
                    }
                    key::MENU | key::KEY5 if st.page == 1 => {
                        api.show_confirm(
                            MODAL_FORMAT,
                            b"Format Storage?",
                            b"Erases /user & /pref.\nCalib & /apps kept.",
                            b"Hold",
                            b"Back",
                            1200,
                        );
                        1
                    }
                    key::EXIT => 0,
                    _ => 0,
                },
                KRX_EV_MODAL if ev.key == MODAL_FORMAT && ev.flags == MODAL_RESULT_CONFIRMED => {
                    let rc = api.call_op(Op::FsMeta, 4, 0, 0);
                    st.refresh(api);
                    api.show_toast(if rc == 0 { b"Storage formatted" } else { b"Format failed" }, 1500);
                    1
                }
                _ => 0,
            }
        }
        KRX_OP_RENDER => {
            st.render(api);
            1
        }
        KRX_OP_EXIT => 0,
        _ => 0,
    }
}

krx_entry!(app_entry);
