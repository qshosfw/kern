//! Shared Choice Pool Hydrator & `OptionSheet` Opener (`leaf_choices_a` / `leaf_choices_b`).
#![allow(dead_code)]

use crate::schema::{CHOICE_SRCS, ITEMS, Kind, resolve_choice};
use crate::shared::{CMD_CHOICE_OPEN, MODAL_CHOICE, SharedState, copy_bytes, subslice};
use kern_sdk::{KRX_LEAF_KIND_DONE, KrxApiTable, KrxLeafContext, nth_packed_slice, prop};

pub const fn pool_len(ch_lo: usize, ch_hi: usize) -> usize {
    let mut sum = 0usize;
    let mut i = ch_lo;
    while i < ch_hi {
        sum += CHOICE_SRCS[i].len();
        i += 1;
    }
    sum
}

pub const fn build_pool<const N: usize>(ch_lo: usize, ch_hi: usize) -> [u8; N] {
    let mut out = [0u8; N];
    let mut cur = 0usize;
    let mut t = ch_lo;
    while t < ch_hi {
        let s = CHOICE_SRCS[t];
        let mut j = 0usize;
        while j < s.len() {
            out[cur + j] = s[j];
            j += 1;
        }
        cur += s.len();
        t += 1;
    }
    out
}

pub const fn build_offs<const M: usize>(ch_lo: usize, ch_hi: usize) -> [u16; M] {
    let mut out = [0u16; M];
    let mut cur = 0usize;
    let mut t = ch_lo;
    while t < ch_hi {
        out[t - ch_lo] = cur as u16;
        cur += CHOICE_SRCS[t].len();
        t += 1;
    }
    out[ch_hi - ch_lo] = cur as u16;
    out
}

pub const fn build_item_ch<const K: usize>(item_lo: usize) -> [[u8; 2]; K] {
    let mut out = [[0u8, 0xFFu8]; K];
    let mut i = 0usize;
    while i < K {
        let it = ITEMS[item_lo + i];
        if matches!(it.kind, Kind::Choice) {
            out[i] = [it.prop_id, it.meta];
        }
        i += 1;
    }
    out
}

#[inline(always)]
fn table_slice<'a>(pool: &'a [u8], offs: &[u16], ch_lo: u8, tbl: u8) -> &'a [u8] {
    let i = (tbl.saturating_sub(ch_lo) as usize).min(offs.len() - 2);
    let s = unsafe { *offs.get_unchecked(i) } as usize;
    let e = unsafe { *offs.get_unchecked(i + 1) } as usize;
    unsafe { core::slice::from_raw_parts(pool.as_ptr().add(s), e - s) }
}

#[inline(always)]
pub fn dispatch<const HAS_BATT: bool>(
    api: &KrxApiTable,
    arg: *const (),
    item_lo: u8,
    item_ch: &[[u8; 2]],
    ch_lo: u8,
    pool: &[u8],
    offs: &[u16],
) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };
    let is_k1 = if HAS_BATT { api.prop_get(prop::NAV_LAYOUT) == 0 } else { false };
    let base = st.cat_start.saturating_sub(item_lo) as usize;

    let is_open = st.cmd == CMD_CHOICE_OPEN;
    let mut r = if is_open { st.arg0 as usize } else { 0 };
    let end = if is_open { r + 1 } else { st.cat_count as usize };
    let mut wi = 104usize;
    while r < end && base + r < item_ch.len() {
        let [pid, meta] = unsafe { *item_ch.get_unchecked(base + r) };
        if meta != 0xFF {
            let raw = api.prop_get(pid);
            let (tbl, eff) = if HAS_BATT {
                resolve_choice(meta, raw, is_k1)
            } else {
                (meta, raw)
            };
            let opts = table_slice(pool, offs, ch_lo, tbl);
            if is_open {
                let lbl = nth_packed_slice(unsafe { &st.detail_sc.bp_tail[..104] }, st.arg0);
                let tlen = copy_bytes(&mut st.sheet_title, 0, lbl);
                let olen = copy_bytes(&mut st.sheet_buf, 0, opts);
                api.show_sheet(
                    MODAL_CHOICE,
                    subslice(&st.sheet_title, 0, tlen),
                    subslice(&st.sheet_buf, 0, olen),
                    eff,
                    true,
                );
            } else {
                let val = nth_packed_slice(opts, eff);
                let dst = unsafe { &mut st.detail_sc.bp_tail };
                let mut j = 0usize;
                while j < val.len() && wi < 143 {
                    dst[wi] = unsafe { *val.get_unchecked(j) };
                    wi += 1;
                    j += 1;
                }
                if wi < 144 {
                    dst[wi] = 0;
                    wi += 1;
                }
            }
        }
        r += 1;
    }
    if !is_open && wi < 144 {
        unsafe { st.detail_sc.bp_tail[wi] = 0 };
    }

    ctx.continuation.kind = KRX_LEAF_KIND_DONE;
    0
}

#[macro_export]
macro_rules! define_choices_leaf {
    ($item_lo:expr, $item_hi:expr, $ch_lo:expr, $ch_hi:expr, $has_batt:expr) => {
        const ITEM_K: usize = $item_hi - $item_lo;
        const P_LEN: usize = $crate::choices_leaf::pool_len($ch_lo, $ch_hi);
        const O_LEN: usize = ($ch_hi - $ch_lo) + 1;

        static ITEM_CH: [[u8; 2]; ITEM_K] =
            $crate::choices_leaf::build_item_ch::<ITEM_K>($item_lo);
        static CHOICE_POOL: [u8; P_LEN] =
            $crate::choices_leaf::build_pool::<P_LEN>($ch_lo, $ch_hi);
        static CHOICE_OFFS: [u16; O_LEN] =
            $crate::choices_leaf::build_offs::<O_LEN>($ch_lo, $ch_hi);

        fn leaf_entry(_op: u32, api: &kern_sdk::KrxApiTable, arg: *const ()) -> u32 {
            $crate::choices_leaf::dispatch::<$has_batt>(
                api,
                arg,
                $item_lo as u8,
                &ITEM_CH,
                $ch_lo as u8,
                &CHOICE_POOL,
                &CHOICE_OFFS,
            )
        }

        kern_sdk::krx_entry!(leaf_entry);
    };
}
