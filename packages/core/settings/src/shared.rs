//! Shared `#[repr(C)]` State & Leaf Dispatch Contract for `settings.krx` (`P0` Paged).
//!
//! All temporary I/O, formatting, and status-bar scratch buffers live in `SharedState`
//! (resident SRAM inside `TRANSIENT_ARENA`) rather than on the Cortex-M0+ stack, keeping
//! root and leaf call frames minimal and immune to stack exhaustion.
#![allow(dead_code)]

use kern_sdk::{KrxApiTable, KrxDirEntry, KrxStatusBar, hash_app_action};

pub const LEAF_APP_LOAD: u8 = 1;
pub const LEAF_APP_HDR: u8 = 2;
pub const LEAF_APP_BODY: u8 = 3;
pub const LEAF_SC_RENDER: u8 = 4;
pub const LEAF_SC_OPEN: u8 = 5;
pub const LEAF_SC_APPLY: u8 = 6;
pub const LEAF_ASSET_OPEN: u8 = 7;
pub const LEAF_ASSET_APPLY: u8 = 8;
pub const LEAF_DEV_LOAD: u8 = 9;
pub const LEAF_DEV_PROMPT: u8 = 10;
pub const LEAF_DEV_APPLY: u8 = 11;
pub const LEAF_APPS_RENDER: u8 = 12;
pub const LEAF_APPS_OPS: u8 = 13;
pub const LEAF_INFO_RENDER: u8 = 14;
pub const LEAF_SYS_OPS: u8 = 15;
pub const LEAF_APP_ICON: u8 = 16;
pub const LEAF_BANDPLANS: u8 = 17;
pub const LEAF_SEC: u8 = 18;
pub const LEAF_LANG_OPEN: u8 = 19;
pub const LEAF_LANG_APPLY: u8 = 20;
pub const LEAF_BOOT_ASSETS: u8 = 21;
pub const LEAF_APPS_SCAN: u8 = 22;
pub const LEAF_SEC_PIN: u8 = 23;
pub const LEAF_SYS_INFO: u8 = 24;
pub const LEAF_LABELS: u8 = 25;
pub const LEAF_CHOICES_A: u8 = 26;
pub const LEAF_CHOICES_B: u8 = 27;
pub const LEAF_CAT_RENDER: u8 = 28;

pub const CMD_CHOICE_HYDRATE: u8 = 0;
pub const CMD_CHOICE_OPEN: u8 = 1;

// Modal tags shared across root and leaves (31..=36 are contiguous sheet-leaf tags)
pub const MODAL_CHOICE: u8 = 30;
pub const MODAL_ASSET_PICK: u8 = 31;
pub const MODAL_LANG_PICK: u8 = 32;
pub const MODAL_MODEL_PICK: u8 = 33;
pub const MODAL_SHORTCUT_PICK: u8 = 34;
pub const MODAL_SEC_TRIES: u8 = 35;
pub const MODAL_SEC_ACTION: u8 = 36;
pub const MODAL_SHORTCUT_SCOPE: u8 = 37;
pub const MODAL_TEXT_OWNER: u8 = 38;
pub const MODAL_TEXT_TAG: u8 = 39;
pub const MODAL_TEXT_RADIO_ID: u8 = 40;
pub const MODAL_TEXT_MODEL: u8 = 41;
pub const MODAL_DELETE_APP: u8 = 42;
pub const MODAL_RESET_SETTINGS: u8 = 43;
pub const MODAL_REBOOT_RADIO: u8 = 44;
pub const MODAL_SEC_VERIFY: u8 = 45;
pub const MODAL_SEC_NEW_PIN: u8 = 46;
pub const MODAL_SEC_CONFIRM_PIN: u8 = 47;

// Asset domains
pub const ASSET_SPLASH: u8 = 0;
pub const ASSET_ROGER: u8 = 1;
pub const ASSET_JINGLE: u8 = 2;
pub const ASSET_LANG: u8 = 3;

pub const CMD_BP_LOAD: u8 = 1;
pub const CMD_BP_RENDER: u8 = 2;
pub const CMD_BP_TOGGLE_TX: u8 = 3;

// Leaf command opcodes
pub const CMD_SC_LOAD: u8 = 1;
pub const CMD_SC_RENDER: u8 = 2;
pub const CMD_SC_APPLY_PICK: u8 = 1;
pub const CMD_SC_TOGGLE_DRAWER: u8 = 2;

pub const CMD_DEV_MODEL_SHEET: u8 = 1;
pub const CMD_DEV_TEXT_PROMPT: u8 = 2;
pub const CMD_DEV_MODEL_PICK: u8 = 1;
pub const CMD_DEV_TEXT_APPLY: u8 = 2;
pub const CMD_DEV_BATT_TWEAK_SAVE: u8 = 3;

pub const CMD_APPS_REFRESH: u8 = 1;
pub const CMD_APPS_LAUNCH: u8 = 2;
pub const CMD_APPS_REQ_DEL: u8 = 3;
pub const CMD_APPS_DO_DEL: u8 = 4;

pub const CMD_INFO_BATT: u8 = 1;
pub const CMD_INFO_SYS: u8 = 2;

pub const CMD_SYS_REQ_RESET: u8 = 1;
pub const CMD_SYS_REQ_REBOOT: u8 = 2;
pub const CMD_SYS_DO_RESET: u8 = 3;
pub const CMD_SYS_SEC_TOGGLE: u8 = 4;
pub const CMD_SYS_SEC_TRIES_OPEN: u8 = 5;
pub const CMD_SYS_SEC_TRIES_PICK: u8 = 6;
pub const CMD_SYS_LAUNCH_MOD: u8 = 7;

/// Language open/load commands (`leaf_lang_open`).
pub const CMD_LANG_OPEN: u8 = 0;
pub const CMD_LANG_LOAD: u8 = 1;

/// Security PIN commands (`CMD_SEC_OPEN`/`CMD_SEC_ACTION` run on `LEAF_SEC`,
/// `CMD_SEC_VERIFY..=CMD_SEC_CONFIRM_PIN` run on `LEAF_SEC_PIN`).
pub const CMD_SEC_OPEN: u8 = 1;
pub const CMD_SEC_ACTION: u8 = 2;
pub const CMD_SEC_VERIFY: u8 = 1;
pub const CMD_SEC_NEW_PIN: u8 = 2;
pub const CMD_SEC_CONFIRM_PIN: u8 = 3;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AppMeta {
    pub path: [u8; 32],
    pub path_len: u8,
    pub name: [u8; 20],
    pub name_len: u8,
    pub author: [u8; 12],
    pub author_len: u8,
    pub action_label: [u8; 16],
    pub action_len: u8,
    pub action_scope: u8,
    pub ver_major: u8,
    pub ver_minor: u8,
    pub ver_patch: u8,
    pub exec_type: u8,
    pub profile: u8,
    pub has_icon16: u8,
    pub icon_id: u8,
    pub size: u32,
    pub caps: u32,
    pub icon_16: [u8; 32],
}

/// Cached `/pref/keys.cfg` + up to 6 installed app hashes/names for zero-Flash-IO Shortcuts rendering & picker.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScBuf {
    pub keys_cfg: [u8; 64],
    pub app_hashes: [u16; 6],
    pub app_names: [[u8; 10]; 6],
    pub app_n: u8,
}

pub const APPS_MAX: usize = 11;

/// Cached Packages / Services list for zero-Flash-IO rendering (up to 11 entries, exact 144 B).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AppListCache {
    pub names: [[u8; 9]; APPS_MAX],
    pub sizes: [u8; APPS_MAX],
    pub icons: [u8; APPS_MAX],
    pub types: [u8; APPS_MAX],
    pub dir_idx: [u8; APPS_MAX],
    pub apps_n: u8,
}

/// Cached Bandplans list for zero-Flash-IO rendering (up to 8 entries).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct BpCache {
    pub names: [[u8; 8]; 8],
    pub tx_mask: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct SecBuf {
    pub pin: [u8; 16],
    pub pin_len: u8,
}

#[repr(C)]
pub union DetailOrSc {
    pub app_meta: AppMeta,
    pub sc: ScBuf,
    pub apps: AppListCache,
    pub bp: BpCache,
    pub sec: SecBuf,
    pub bp_tail: [u8; 144],
}

#[repr(C, align(4))]
pub struct SharedState {
    // Hot scalars placed at offsets 0..32 so Cortex-M0+ Thumb-1 `ldrb`/`ldrh`/`ldr`
    // reach every field in a single 2-byte instruction (saves hundreds of bytes across root + leaves).
    pub cmd: u8,
    pub arg0: u8,
    pub result: u8,
    pub app_count: u8,
    pub app_sel: u8,
    pub modules_only: u8,
    pub sc_sel: u8,
    pub bp_sel: u8,
    pub bp_count: u8,
    pub info_sel: u8,
    pub detail_btn: u8,
    pub sc_scope: u8,
    pub sc_slot: u8,
    pub sc_target_count: u8,
    pub asset_domain: u8,
    pub asset_count: u8,
    pub dev_model_idx: u8,
    pub cat_start: u8,

    pub batt_tweak_mv: i16,
    pub scroll_y: i16,
    pub anim_tick: u16,
    pub detail_scroll: i16,
    pub cat_count: u8,
    pub cat_editing: u8,

    pub sec_status: u32,

    // Contiguous 272 B 4-byte-aligned scratch (`sheet_buf[128]` + `detail_sc[144]`):
    // holds `OptionSheet` options (when open), `KrxStatusBar` (64 B during `set_title`),
    // `KrxSysInfo` (128 B), or `/user/bandplans.bnd` (272 B) with zero stack allocation.
    pub sheet_buf: [u8; 128],
    pub detail_sc: DetailOrSc,
    pub fmt_buf: [u8; 20],
    /// NUL-terminated display strings: `0..4` = assets (splash/roger/jingle/lang),
    /// `4..8` = device fields (model/owner/tag/radio_id).
    pub str_fields: [[u8; 12]; 8],
    pub sheet_title: [u8; 16],
    /// Resident pointer to `schema::ITEMS` in root SRAM (avoids duplicating 236 B in render leaf).
    pub items_ptr: *const u8,
}

const _: () = assert!(
    core::mem::offset_of!(SharedState, sheet_buf) == 32
        && core::mem::offset_of!(SharedState, detail_sc) == 160
        && core::mem::size_of::<DetailOrSc>() == 144
        && core::mem::size_of::<KrxDirEntry>() <= 32
        && core::mem::size_of::<KrxStatusBar>() == 64
);

impl SharedState {
    pub const fn new() -> Self {
        Self {
            cmd: 0,
            arg0: 0,
            result: 0,
            app_count: 0,
            app_sel: 0,
            modules_only: 0,
            sc_sel: 0,
            bp_sel: 0,
            bp_count: 0,
            info_sel: 0,
            detail_btn: 0,
            sc_scope: 0,
            sc_slot: 0,
            sc_target_count: 0,
            asset_domain: 0,
            asset_count: 0,
            dev_model_idx: 0,
            cat_start: 0,
            batt_tweak_mv: 0,
            scroll_y: 0,
            anim_tick: 0,
            detail_scroll: 0,
            cat_count: 0,
            cat_editing: 0,
            sec_status: 0,
            sheet_buf: [0; 128],
            detail_sc: DetailOrSc { bp_tail: [0; 144] },
            fmt_buf: [0; 20],
            str_fields: [[0; 12]; 8],
            sheet_title: [0; 16],
            items_ptr: core::ptr::null(),
        }
    }

    #[inline(always)]
    pub fn meta(&self) -> &AppMeta {
        unsafe { &self.detail_sc.app_meta }
    }

    #[inline(always)]
    pub fn meta_mut(&mut self) -> &mut AppMeta {
        unsafe { &mut self.detail_sc.app_meta }
    }

    #[inline(always)]
    pub fn sc(&self) -> &ScBuf {
        unsafe { &self.detail_sc.sc }
    }

    #[inline(always)]
    pub fn sc_mut(&mut self) -> &mut ScBuf {
        unsafe { &mut self.detail_sc.sc }
    }

    #[inline(always)]
    pub fn apps(&self) -> &AppListCache {
        unsafe { &self.detail_sc.apps }
    }

    #[inline(always)]
    pub fn apps_mut(&mut self) -> &mut AppListCache {
        unsafe { &mut self.detail_sc.apps }
    }

    #[inline(always)]
    pub fn bp(&self) -> &BpCache {
        unsafe { &self.detail_sc.bp }
    }

    #[inline(always)]
    pub fn bp_mut(&mut self) -> &mut BpCache {
        unsafe { &mut self.detail_sc.bp }
    }

    #[inline(always)]
    pub fn sec(&self) -> &SecBuf {
        unsafe { &self.detail_sc.sec }
    }

    #[inline(always)]
    pub fn sec_mut(&mut self) -> &mut SecBuf {
        unsafe { &mut self.detail_sc.sec }
    }

    #[inline(always)]
    pub fn bp_raw_mut(&mut self) -> &mut [u8; 272] {
        unsafe { &mut *(core::ptr::addr_of_mut!(self.sheet_buf) as *mut [u8; 272]) }
    }
}

/// Inspect a KRX header for `(exec_type, icon_id)` using caller-supplied `buf` (`>= 64 B`).
#[inline(never)]
pub fn read_krx_meta(api: &KrxApiTable, full: &[u8], buf: &mut [u8]) -> (u8, u8) {
    let n = api.read_file_at(full, 0, buf);
    let mut exec_type = 1u8;
    let mut icon_id = 0u8;
    if n < 28 {
        return (exec_type, icon_id);
    }
    let p = buf.as_ptr();
    let hlen = unsafe { (*p.add(8) as usize) | ((*p.add(9) as usize) << 8) }.min(n);
    let mut off = 24usize;
    while off + 4 <= hlen {
        let sid = unsafe { (*p.add(off) as u16) | ((*p.add(off + 1) as u16) << 8) };
        let slen = unsafe { (*p.add(off + 2) as usize) | ((*p.add(off + 3) as usize) << 8) };
        if slen < 4 || off + slen > hlen {
            break;
        }
        if sid == 0x0001 && slen >= 29 {
            let ic = unsafe { (*p.add(off + 27) as u16) | ((*p.add(off + 28) as u16) << 8) };
            if ic > 0 && ic < 255 {
                icon_id = ic as u8;
            }
        } else if sid == 0x0009 && slen >= 5 {
            exec_type = unsafe { *p.add(off + 4) };
        }
        off += (slen + 3) & !3;
    }
    (exec_type, icon_id)
}

#[inline(always)]
pub fn ends_with_4(name: &[u8], sfx: &[u8; 4]) -> bool {
    let n = name.len();
    n >= 4
        && byte_at(name, n - 4) == sfx[0]
        && byte_at(name, n - 3) == sfx[1]
        && byte_at(name, n - 2) == sfx[2]
        && byte_at(name, n - 1) == sfx[3]
}

#[inline(always)]
pub fn is_mod_name(name: &[u8]) -> bool {
    ends_with_4(name, b".mod")
}

/// Returns `true` if `name` is an eligible asset file for `dom`.
#[inline(never)]
pub fn asset_matches(dom: u8, name: &[u8]) -> bool {
    let n = name.len();
    if n < 5 || unsafe { *name.get_unchecked(0) } == b'.' {
        return false;
    }
    let p = unsafe { name.as_ptr().add(n - 4) };
    let ext = unsafe {
        (*p as u32)
            | ((*p.add(1) as u32) << 8)
            | ((*p.add(2) as u32) << 16)
            | ((*p.add(3) as u32) << 24)
    };
    const RLE: u32 = u32::from_le_bytes(*b".rle");
    const IMG: u32 = u32::from_le_bytes(*b".img");
    const BIN: u32 = u32::from_le_bytes(*b".bin");
    const TON: u32 = u32::from_le_bytes(*b".ton");
    const RGR: u32 = u32::from_le_bytes(*b".rgr");
    const LNG: u32 = u32::from_le_bytes(*b".lng");
    match dom {
        ASSET_SPLASH => ext == RLE || ext == IMG || ext == BIN,
        ASSET_ROGER => ext == TON || ext == RGR,
        ASSET_JINGLE => ext == TON,
        ASSET_LANG => {
            if ext != LNG {
                return false;
            }
            let mut i = 0usize;
            while i < n - 4 {
                if unsafe { *name.get_unchecked(i) } == b'_' {
                    return false;
                }
                i += 1;
            }
            true
        }
        _ => true,
    }
}

/// Canonical asset pretty-name formatter shared across load, picker, and apply:
/// `"/user/rogers/01_mini_kong.ton"` -> `"Mini Kong"`.
#[inline(never)]
pub fn format_pretty_asset(raw: &[u8], out: &mut [u8]) -> usize {
    let rp = raw.as_ptr();
    let op = out.as_mut_ptr();
    let mut s = 0usize;
    let mut end = raw.len();
    let mut i = 0usize;
    while i < raw.len() {
        let b = unsafe { *rp.add(i) };
        if b == b'/' {
            s = i + 1;
            end = raw.len();
        } else if b == b'.' {
            end = i;
        }
        i += 1;
    }
    let mut d = s;
    while d < end {
        let b = unsafe { *rp.add(d) };
        if b < b'0' || b > b'9' {
            if d > s && (b == b'_' || b == b'-') {
                s = d + 1;
            }
            break;
        }
        d += 1;
    }
    let mut o = 0usize;
    let mut cap = true;
    while s < end && o < out.len() {
        let mut b = unsafe { *rp.add(s) };
        if b == b'_' || b == b'-' {
            b = b' ';
            cap = true;
        } else {
            if cap && b >= b'a' && b <= b'z' {
                b -= 32;
            }
            cap = false;
        }
        unsafe { *op.add(o) = b };
        o += 1;
        s += 1;
    }
    o
}

/// Read the self-describing `__LANG_ASCII__` display label from a `.lng` (`LNG1`) pack via
/// binary search on its sorted 8-byte `LangEntry` index (zero stack arrays; `s16` must be 4B-aligned).
#[inline(never)]
pub fn read_lng_ascii_name(api: &KrxApiTable, full: &[u8], s16: &mut [u8; 16], out: &mut [u8]) -> usize {
    const KEY_HASH: u32 = {
        let b = b"__LANG_ASCII__";
        let mut h = 0x811C_9DC5u32;
        let mut i = 0usize;
        while i < b.len() {
            h ^= b[i] as u32;
            h = h.wrapping_mul(0x0100_0193);
            i += 1;
        }
        h
    };
    let bp = s16.as_mut_ptr();
    if api.read_file_at(full, 0, s16) < 16
        || unsafe { *(bp as *const u32) } != u32::from_le_bytes(*b"LNG1")
    {
        return 0;
    }
    let count = unsafe { *(bp.add(6) as *const u16) } as u32;
    let pool_off = 16 + (count << 3);
    let mut lo = 0u32;
    let mut hi = count;
    while lo < hi {
        let mid = (lo + hi) >> 1;
        let e8 = unsafe { core::slice::from_raw_parts_mut(bp, 8) };
        if api.read_file_at(full, 16 + (mid << 3), e8) < 8 {
            return 0;
        }
        let kh = unsafe { *(bp as *const u32) };
        if kh == KEY_HASH {
            let soff = unsafe { (*bp.add(4) as u32) | ((*bp.add(5) as u32) << 8) | ((*bp.add(6) as u32) << 16) };
            let slen = (unsafe { *bp.add(7) } as usize).min(out.len());
            if slen == 0 {
                return 0;
            }
            return api.read_file_at(full, pool_off + soff, unsafe {
                core::slice::from_raw_parts_mut(out.as_mut_ptr(), slen)
            });
        }
        if kh < KEY_HASH { lo = mid + 1; } else { hi = mid; }
    }
    0
}

/// Format an unsigned 32-bit integer into `out` via `Op::SysDiv` (supports full 8-digit Radio IDs).
#[inline(never)]
pub fn format_u32_into<'a>(api: &KrxApiTable, mut val: u32, out: &'a mut [u8]) -> &'a [u8] {
    let mut n = 0usize;
    let max = out.len().min(10);
    while n < max {
        let mut rem = 0u32;
        val = api.call_op(kern_sdk::Op::SysDiv, val as usize, 10, &raw mut rem as usize) as u32;
        set_byte(out, n, b'0' + (rem as u8));
        n += 1;
        if val == 0 {
            break;
        }
    }
    let mut i = 0usize;
    while i < (n >> 1) {
        let a = byte_at(out, i);
        let b = byte_at(out, n - 1 - i);
        set_byte(out, i, b);
        set_byte(out, n - 1 - i, a);
        i += 1;
    }
    subslice(out, 0, n)
}

/// Resolve the bound target ID for shortcut `slot` in `scope` (`0` = Global) and whether overridden.
#[inline(always)]
pub fn get_slot_action_override(cfg: &[u8; 64], slot: u8, scope: u8) -> (u16, bool) {
    if scope != 0 {
        let mut i = 0usize;
        while i < 3 {
            let o = 48 + i * 4;
            if byte_at(cfg, o) == slot && byte_at(cfg, o + 1) == scope {
                let act = (byte_at(cfg, o + 2) as u16) | ((byte_at(cfg, o + 3) as u16) << 8);
                return (act, true);
            }
            i += 1;
        }
    }
    let o = 10 + (slot as usize) * 2;
    let act = (byte_at(cfg, o) as u16) | ((byte_at(cfg, o + 1) as u16) << 8);
    (act, false)
}

#[inline(always)]
pub fn get_slot_action(cfg: &[u8; 64], slot: u8, scope: u8) -> u16 {
    get_slot_action_override(cfg, slot, scope).0
}

#[inline(always)]
pub fn subslice(s: &[u8], start: usize, end: usize) -> &[u8] {
    let e = end.min(s.len());
    let st = start.min(e);
    unsafe { core::slice::from_raw_parts(s.as_ptr().add(st), e - st) }
}

#[inline(always)]
pub fn subslice_mut(s: &mut [u8], start: usize, end: usize) -> &mut [u8] {
    let e = end.min(s.len());
    let st = start.min(e);
    unsafe { core::slice::from_raw_parts_mut(s.as_mut_ptr().add(st), e - st) }
}

#[inline(always)]
pub fn byte_at(s: &[u8], i: usize) -> u8 {
    if i < s.len() { unsafe { *s.get_unchecked(i) } } else { 0 }
}

#[inline(always)]
pub fn set_byte(s: &mut [u8], i: usize, v: u8) {
    if i < s.len() { unsafe { *s.get_unchecked_mut(i) = v }; }
}

#[inline(never)]
pub fn fill_bytes(s: &mut [u8], v: u8) {
    let mut i = 0usize;
    while i < s.len() {
        unsafe { *s.get_unchecked_mut(i) = v };
        i += 1;
    }
}

#[inline(never)]
pub fn copy_bytes(dst: &mut [u8], pos: usize, src: &[u8]) -> usize {
    let n = src.len().min(dst.len().saturating_sub(pos));
    let mut i = 0usize;
    while i < n {
        unsafe { *dst.get_unchecked_mut(pos + i) = *src.get_unchecked(i) };
        i += 1;
    }
    pos + n
}

#[inline(always)]
pub fn cstr_slice(buf: &[u8]) -> &[u8] {
    let mut n = 0usize;
    while n < buf.len() && unsafe { *buf.get_unchecked(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(buf.as_ptr(), n) }
}

/// Copy `src` into `dst`, trimming trailing spaces/NULs and zero-padding the remainder.
/// Preserves embedded spaces (e.g. `"UV-K5 V3"`, `"John Doe"`).
#[inline(never)]
pub fn copy_trimmed(dst: &mut [u8], src: &[u8]) {
    fill_bytes(dst, 0);
    let mut n = src.len().min(dst.len());
    while n > 0 {
        let b = byte_at(src, n - 1);
        if b != 0 && b != b' ' {
            break;
        }
        n -= 1;
    }
    let _ = copy_bytes(dst, 0, subslice(src, 0, n));
}

#[inline(always)]
pub fn strip_apps_prefix(name: &[u8]) -> &[u8] {
    if name.len() > 6 && byte_at(name, 0) == b'/' && byte_at(name, 5) == b'/' {
        subslice(name, 6, name.len())
    } else {
        name
    }
}

#[inline(always)]
pub fn hash_app_path(path: &[u8]) -> u16 {
    hash_app_action(path, 0)
}
