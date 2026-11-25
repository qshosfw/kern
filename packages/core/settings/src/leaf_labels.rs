//! P0 Leaf 25: Hydrates active category/submenu labels into `st.detail_sc.bp_tail[0..104]`
//! once on `enter_view`, then tail-chains to `LEAF_CHOICES` (`CMD_CHOICE_HYDRATE`).
#![no_std]
#![no_main]

mod schema;
mod shared;

use kern_sdk::{KRX_LEAF_KIND_CALL_LEAF, KrxApiTable, KrxLeafContext, krx_entry};
use schema::ITEM_LABELS;
use shared::{CMD_CHOICE_HYDRATE, LEAF_CHOICES_A, LEAF_CHOICES_B, SharedState};

const LBL_POOL_LEN: usize = {
    let mut sum = 0usize;
    let mut i = 0usize;
    while i < ITEM_LABELS.len() {
        sum += ITEM_LABELS[i].len() + 1;
        i += 1;
    }
    sum
};

static LBL_POOL: [u8; LBL_POOL_LEN] = {
    let mut out = [0u8; LBL_POOL_LEN];
    let mut cur = 0usize;
    let mut i = 0usize;
    while i < ITEM_LABELS.len() {
        let s = ITEM_LABELS[i];
        let mut j = 0usize;
        while j < s.len() {
            out[cur + j] = s[j];
            j += 1;
        }
        cur += s.len() + 1;
        i += 1;
    }
    out
};

fn leaf_entry(_op: u32, _api: &KrxApiTable, arg: *const ()) -> u32 {
    if arg.is_null() {
        return 1;
    }
    let ctx = unsafe { &mut *(arg as *mut KrxLeafContext) };
    let st = unsafe { &mut *(ctx.arg as *mut SharedState) };

    // Skip `st.cat_start` NUL-terminated entries in `LBL_POOL`.
    let mut src = 0usize;
    let mut skip = st.cat_start;
    while skip > 0 && src < LBL_POOL_LEN {
        if unsafe { *LBL_POOL.get_unchecked(src) } == 0 {
            skip -= 1;
        }
        src += 1;
    }

    // Copy `st.cat_count` NUL-terminated labels into `bp_tail[0..104]`.
    let dst = unsafe { &mut st.detail_sc.bp_tail };
    let mut di = 0usize;
    let mut rem = st.cat_count;
    while rem > 0 && src < LBL_POOL_LEN && di < 103 {
        let b = unsafe { *LBL_POOL.get_unchecked(src) };
        src += 1;
        dst[di] = b;
        di += 1;
        if b == 0 {
            rem -= 1;
        }
    }
    dst[di] = 0;

    st.cmd = CMD_CHOICE_HYDRATE;
    ctx.continuation.kind = KRX_LEAF_KIND_CALL_LEAF;
    ctx.continuation.target_or_code = if st.cat_start < 25 {
        LEAF_CHOICES_A
    } else {
        LEAF_CHOICES_B
    };
    ctx.continuation.payload = ctx.arg;
    0
}

krx_entry!(leaf_entry);
