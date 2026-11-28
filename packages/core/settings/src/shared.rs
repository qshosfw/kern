//! Shared `#[repr(C, align(4))]` State, Constants & Zero-Relocation Helpers for `settings.krx`.
//!
//! Resident in Zone B `[0..cache_base_off)` so every P1 v2 2 KiB domain block executes with
//! 0 heap, minimal Cortex-M0+ stack frames, and guaranteed-live modal/OptionSheet buffers.
#![allow(dead_code)]

use kern_sdk::{
    COLOR_CLEAR, KRX_STATUS_CUSTOM_TITLE, KRX_STATUS_DIVIDER, KRX_STATUS_VISIBLE,
    KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxStatusBar, KrxWidget, Op, div_rem_10_u16, hash_app_action,
    sub,
};

// ── P1 v2 Demand-Paged Domain Blocks (0-based `SEC_BLOCK_MAP` indices) ───────
pub const PAGE_SCHEMA: u8 = 0;
pub const PAGE_PACKAGES: u8 = 1;
pub const PAGE_SHORTCUTS: u8 = 2;
pub const PAGE_ASSETS: u8 = 3;
pub const PAGE_SYSTEM: u8 = 4;

// ── Views ────────────────────────────────────────────────────────────────────
pub const VIEW_HOME: u8 = 0;
pub const VIEW_CATEGORY: u8 = 1;
pub const VIEW_SUBMENU: u8 = 2;
pub const VIEW_PACKAGES: u8 = 3;
pub const VIEW_PKG_DETAIL: u8 = 4;
pub const VIEW_SHORTCUTS: u8 = 5;
pub const VIEW_BANDPLANS: u8 = 6;
pub const VIEW_BATT_INFO: u8 = 7;
pub const VIEW_SYS_INFO: u8 = 8;

// ── Top-level Categories ─────────────────────────────────────────────────────
pub const CAT_COUNT: u8 = 9;
pub const CAT_LANGUAGE: u8 = 6;
pub const CAT_SECURITY: u8 = 8;

// ── Submenu Ranges & Action IDs ──────────────────────────────────────────────
pub const ACT_INSTALLED_APPS: u8 = 1;
pub const ACT_SYS_MODULES: u8 = 2;
pub const ACT_SHORTCUTS: u8 = 3;
pub const ACT_BANDPLANS: u8 = 4;
pub const ACT_SUB_BATT_INFO: u8 = 5;
pub const ACT_SYS_INFO: u8 = 6;
pub const ACT_SUB_SPLASH: u8 = 7;
pub const ACT_SUB_BTN_TWEAKS: u8 = 8;
pub const ACT_SUB_BATT_TWEAKS: u8 = 9;
pub const ACT_SUB_RUNTIME: u8 = 10;

pub const ACT_RESET_SETTINGS: u8 = 11;
pub const ACT_REBOOT_RADIO: u8 = 12;
pub const ACT_ASSET_SPLASH: u8 = 13;
pub const ACT_ASSET_ROGER: u8 = 14;
pub const ACT_ASSET_JINGLE: u8 = 15;
pub const ACT_DEV_MODEL: u8 = 17;
pub const ACT_DEV_OWNER: u8 = 18;
pub const ACT_DEV_TAG: u8 = 19;
pub const ACT_DEV_RADIO_ID: u8 = 20;
pub const ACT_SEC_PASSCODE: u8 = 21;
pub const ACT_SEC_EXPOSE: u8 = 22;
pub const ACT_SEC_STEALTH: u8 = 23;
pub const ACT_SEC_DIGITS: u8 = 24;
pub const ACT_SEC_MAX_TRIES: u8 = 25;
pub const ACT_SYS_STORAGE: u8 = 26;
pub const ACT_SYS_CALIB: u8 = 27;
pub const ACT_SYS_DFU: u8 = 28;

// ── Page Command Opcodes ─────────────────────────────────────────────────────
// Page 0: Schema & Category/Submenu engine
pub const CMD_SCHEMA_RENDER: u8 = 0;
pub const CMD_SCHEMA_ACTIVATE: u8 = 1;
pub const CMD_SCHEMA_ADJUST: u8 = 2;
pub const CMD_SCHEMA_CHOICE_APPLY: u8 = 3;

// Page 1: Packages & Services browser + Detail card
pub const CMD_PKG_SCAN: u8 = 0;
pub const CMD_PKG_RENDER: u8 = 1;
pub const CMD_PKG_LOAD_DETAIL: u8 = 2;
pub const CMD_PKG_LAUNCH: u8 = 3;
pub const CMD_PKG_REQ_DEL: u8 = 4;
pub const CMD_PKG_DO_DEL: u8 = 5;

// Page 2: Shortcuts & Bandplans
pub const CMD_SC_LOAD: u8 = 0;
pub const CMD_SC_RENDER: u8 = 1;
pub const CMD_SC_OPEN: u8 = 2;
pub const CMD_SC_APPLY_PICK: u8 = 3;
pub const CMD_SC_TOGGLE_DRAWER: u8 = 4;
pub const CMD_BP_LOAD: u8 = 5;
pub const CMD_BP_RENDER: u8 = 6;
pub const CMD_BP_TOGGLE: u8 = 7;

// Page 3: Assets, Language & Device Identity
pub const CMD_ASSETS_BOOT: u8 = 0;
pub const CMD_ASSET_OPEN: u8 = 1;
pub const CMD_ASSET_APPLY: u8 = 2;
pub const CMD_DEV_MODEL_SHEET: u8 = 3;
pub const CMD_DEV_TEXT_PROMPT: u8 = 4;
pub const CMD_DEV_MODEL_PICK: u8 = 5;
pub const CMD_DEV_TEXT_APPLY: u8 = 6;
pub const CMD_DEV_BATT_TWEAK_SAVE: u8 = 7;

// Page 4: Telemetry, System Ops & Security Enclave
pub const CMD_SYS_RENDER_BATT: u8 = 0;
pub const CMD_SYS_RENDER_INFO: u8 = 1;
pub const CMD_SYS_LAUNCH_MOD: u8 = 2;
pub const CMD_SYS_REQ_RESET: u8 = 3;
pub const CMD_SYS_DO_RESET: u8 = 4;
pub const CMD_SYS_REQ_REBOOT: u8 = 5;
pub const CMD_SYS_SEC_OPEN: u8 = 6;
pub const CMD_SYS_SEC_ACTION: u8 = 7;
pub const CMD_SYS_SEC_TOGGLE: u8 = 8;
pub const CMD_SYS_SEC_TRIES_OPEN: u8 = 9;
pub const CMD_SYS_SEC_TRIES_PICK: u8 = 10;
pub const CMD_SYS_SEC_PIN_MODAL: u8 = 11;

// ── Modal Tags ───────────────────────────────────────────────────────────────
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

// ── Asset Domains & Synthetic Property IDs ───────────────────────────────────
pub const ASSET_SPLASH: u8 = 0;
pub const ASSET_ROGER: u8 = 1;
pub const ASSET_JINGLE: u8 = 2;
pub const ASSET_LANG: u8 = 3;

pub const PROP_BATT_TWEAK: u8 = 0xFF;

// ── UI Layout & `KRX_WIDGET_MENU_ITEM` Flag Bits ─────────────────────────────
pub const VISIBLE_ROWS: u8 = 4;
pub const ROW_START_Y: i16 = 9;
pub const ROW_STEP_Y: i16 = 13;

pub const FLAG_SELECTED: u8 = 1 << 0;
pub const FLAG_CHEVRON: u8 = 1 << 1;
pub const FLAG_OUTLINE: u8 = 1 << 2;
pub const FLAG_TOGGLE: u8 = 1 << 3;
pub const FLAG_TOGGLE_ON: u8 = 1 << 4;
pub const FLAG_ARROW_LEFT: u8 = 1 << 5;
pub const FLAG_ARROW_RIGHT: u8 = 1 << 6;
pub const FLAG_SHORTCUT: u8 = 1 << 7;

pub const APPS_MAX: usize = 12;
pub const MAX_BP: usize = 8;

// ── Mutually-Exclusive View Caches (288 B union) ─────────────────────────────

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AppListCache {
    pub names: [[u8; 10]; APPS_MAX],
    pub sizes: [u8; APPS_MAX],
    pub icons: [u8; APPS_MAX],
    pub types: [u8; APPS_MAX],
    pub dir_idx: [u8; APPS_MAX],
    pub apps_n: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AppMeta {
    pub path: [u8; 32],
    pub name: [u8; 20],
    pub author: [u8; 12],
    pub icon_16: [u8; 32],
    pub caps: u32,
    pub size: u32,
    pub path_len: u8,
    pub name_len: u8,
    pub author_len: u8,
    pub ver_major: u8,
    pub ver_minor: u8,
    pub ver_patch: u8,
    pub exec_type: u8,
    pub profile: u8,
    pub has_icon16: u8,
    pub icon_id: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ShortcutCache {
    pub keys_cfg: [u8; 64],
    pub app_hashes: [u16; 8],
    pub app_names: [[u8; 10]; 8],
    pub app_n: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct BandplanCache {
    pub names: [[u8; 8]; MAX_BP],
    pub tx_mask: u8,
}

#[repr(C, align(4))]
#[derive(Clone, Copy)]
pub union ViewCache {
    pub apps: AppListCache,
    pub meta: AppMeta,
    pub sc: ShortcutCache,
    pub bp: BandplanCache,
    pub raw: [u8; 288],
}

/// Resident shared state (`~660 B`, 4-byte aligned, permanently live in Zone B root).
#[repr(C, align(4))]
pub struct SharedState {
    pub sec_status: u32,
    pub cur_y: i16,
    pub target_y: i16,
    pub batt_tweak_mv: i16,
    pub anim_tick: u16,
    pub repeat_ms: u16,
    pub view: u8,
    pub cat_idx: u8,
    pub slice_start: u8,
    pub slice_count: u8,
    pub sel: u8,
    pub top_row: u8,
    pub saved_home_sel: u8,
    pub saved_home_top: u8,
    pub saved_cat_sel: u8,
    pub saved_cat_top: u8,
    pub editing: u8,
    pub choice_prop: u8,
    pub choice_meta: u8,
    pub dirty: u8,
    pub held_key: u8,
    pub repeat_steps: u8,
    pub tick_ctr: u8,
    pub slide_x: i8,
    pub dev_model_idx: u8,
    pub asset_domain: u8,
    pub asset_count: u8,
    pub mod_only: u8,
    pub sc_scope: u8,
    pub sc_slot: u8,
    pub sc_target_count: u8,
    pub app_count: u8,
    pub app_sel: u8,
    pub detail_btn: u8,
    pub bp_count: u8,
    pub bp_sel: u8,
    pub info_sel: u8,
    pub result: u8,
    pub cmd: u8,
    pub arg0: u8,
    pub pin_len: u8,
    /// `[0]=Splash, [1]=Roger, [2]=Jingle, [3]=Lang, [4]=Model, [5]=Owner, [6]=Tag, [7]=RadioID`
    pub str_fields: [[u8; 12]; 8],
    pub sheet_title: [u8; 20],
    pub sheet_buf: [u8; 148],
    pub confirm_ok: [u8; 8],
    pub confirm_cancel: [u8; 8],
    pub fmt_buf: [u8; 20],
    pub pin_buf: [u8; 16],
    pub cache: ViewCache,
}

impl SharedState {
    pub const fn new() -> Self {
        Self {
            sec_status: 0,
            cur_y: 0,
            target_y: 0,
            batt_tweak_mv: 0,
            anim_tick: 0,
            repeat_ms: 0,
            view: VIEW_HOME,
            cat_idx: 0,
            slice_start: 0,
            slice_count: 0,
            sel: 0,
            top_row: 0,
            saved_home_sel: 0,
            saved_home_top: 0,
            saved_cat_sel: 0,
            saved_cat_top: 0,
            editing: 0,
            choice_prop: 0,
            choice_meta: 0,
            dirty: 0,
            held_key: 0,
            repeat_steps: 0,
            tick_ctr: 0,
            slide_x: 0,
            dev_model_idx: 0,
            asset_domain: 0,
            asset_count: 0,
            mod_only: 0,
            sc_scope: 0,
            sc_slot: 0,
            sc_target_count: 0,
            app_count: 0,
            app_sel: 0,
            detail_btn: 0,
            bp_count: 0,
            bp_sel: 0,
            info_sel: 0,
            result: 0,
            cmd: 0,
            arg0: 0,
            pin_len: 0,
            str_fields: [[0; 12]; 8],
            sheet_title: [0; 20],
            sheet_buf: [0; 148],
            confirm_ok: [0; 8],
            confirm_cancel: [0; 8],
            fmt_buf: [0; 20],
            pin_buf: [0; 16],
            cache: ViewCache { raw: [0; 288] },
        }
    }

    #[inline(always)]
    pub fn is_k1(&self) -> bool {
        if self.dev_model_idx == 0 {
            true
        } else if self.dev_model_idx == 1 {
            false
        } else {
            let m = &self.str_fields[4];
            (m[0] == b'U' && m[1] == b'V' && m[2] == b'-' && m[3] == b'K' && m[4] == b'1')
                || ((m[0] | 32) == b'k' && m[1] == b'1')
        }
    }

    #[inline(always)]
    pub fn apps(&self) -> &AppListCache {
        unsafe { &self.cache.apps }
    }

    #[inline(always)]
    pub fn apps_mut(&mut self) -> &mut AppListCache {
        unsafe { &mut self.cache.apps }
    }

    #[inline(always)]
    pub fn meta(&self) -> &AppMeta {
        unsafe { &self.cache.meta }
    }

    #[inline(always)]
    pub fn meta_mut(&mut self) -> &mut AppMeta {
        unsafe { &mut self.cache.meta }
    }

    #[inline(always)]
    pub fn sc(&self) -> &ShortcutCache {
        unsafe { &self.cache.sc }
    }

    #[inline(always)]
    pub fn sc_mut(&mut self) -> &mut ShortcutCache {
        unsafe { &mut self.cache.sc }
    }

    #[inline(always)]
    pub fn bp(&self) -> &BandplanCache {
        unsafe { &self.cache.bp }
    }

    #[inline(always)]
    pub fn bp_mut(&mut self) -> &mut BandplanCache {
        unsafe { &mut self.cache.bp }
    }

    #[inline(always)]
    pub fn raw_mut(&mut self) -> &mut [u8; 288] {
        unsafe { &mut self.cache.raw }
    }
}

// ── Zero-memcpy / Zero-relocation Helpers ────────────────────────────────────

#[inline(always)]
pub fn byte_at(s: &[u8], i: usize) -> u8 {
    if i < s.len() {
        unsafe { *s.get_unchecked(i) }
    } else {
        0
    }
}

#[inline(always)]
pub fn set_byte(s: &mut [u8], i: usize, v: u8) {
    if i < s.len() {
        unsafe { *s.get_unchecked_mut(i) = v };
    }
}

#[inline(always)]
pub fn subslice(s: &[u8], start: usize, end: usize) -> &[u8] {
    let a = start.min(s.len());
    let b = end.min(s.len()).max(a);
    unsafe { core::slice::from_raw_parts(s.as_ptr().add(a), b - a) }
}

#[inline(always)]
pub fn subslice_mut(s: &mut [u8], start: usize, end: usize) -> &mut [u8] {
    let a = start.min(s.len());
    let b = end.min(s.len()).max(a);
    unsafe { core::slice::from_raw_parts_mut(s.as_mut_ptr().add(a), b - a) }
}

#[inline(never)]
pub fn fill_bytes(dst: &mut [u8], val: u8) {
    let p = dst.as_mut_ptr();
    let mut i = 0usize;
    while i < dst.len() {
        unsafe { core::ptr::write_volatile(p.add(i), val) };
        i += 1;
    }
}

#[inline(never)]
pub fn copy_bytes(dst: &mut [u8], start: usize, src: &[u8]) -> usize {
    let cap = dst.len().saturating_sub(start);
    let n = src.len().min(cap);
    let dp = dst.as_mut_ptr();
    let sp = src.as_ptr();
    let mut i = 0usize;
    while i < n {
        unsafe { core::ptr::write_volatile(dp.add(start + i), *sp.add(i)) };
        i += 1;
    }
    start + n
}

#[inline(never)]
pub fn copy_trimmed(dst: &mut [u8], src: &[u8]) {
    fill_bytes(dst, 0);
    let mut end = src.len();
    while end > 0 {
        let b = byte_at(src, end - 1);
        if b == 0 || b == b' ' {
            end -= 1;
        } else {
            break;
        }
    }
    let _ = copy_bytes(dst, 0, subslice(src, 0, end));
}

#[inline(never)]
pub fn cstr_slice(s: &[u8]) -> &[u8] {
    let mut i = 0usize;
    while i < s.len() && byte_at(s, i) != 0 {
        i += 1;
    }
    subslice(s, 0, i)
}

#[inline(always)]
pub fn rd16(s: &[u8], o: usize) -> u16 {
    (byte_at(s, o) as u16) | ((byte_at(s, o + 1) as u16) << 8)
}

#[inline(never)]
pub fn strip_apps_prefix(name: &[u8]) -> &[u8] {
    if name.len() > 6
        && byte_at(name, 0) == b'/'
        && byte_at(name, 1) == b'a'
        && byte_at(name, 5) == b'/'
    {
        subslice(name, 6, name.len())
    } else {
        name
    }
}

#[inline(never)]
pub fn ends_with_ci(s: &[u8], suf: &[u8]) -> bool {
    if s.len() < suf.len() {
        return false;
    }
    let off = s.len() - suf.len();
    let mut i = 0usize;
    while i < suf.len() {
        if (byte_at(s, off + i) | 32) != (byte_at(suf, i) | 32) {
            return false;
        }
        i += 1;
    }
    true
}

#[inline(never)]
pub fn is_mod_name(s: &[u8]) -> bool {
    ends_with_ci(s, b".mod")
}

#[inline(never)]
pub fn asset_matches(domain: u8, name: &[u8]) -> bool {
    if domain == ASSET_SPLASH {
        ends_with_ci(name, b".rle") || ends_with_ci(name, b".img")
    } else if domain == ASSET_ROGER || domain == ASSET_JINGLE {
        ends_with_ci(name, b".ton")
    } else {
        if !ends_with_ci(name, b".lng") {
            return false;
        }
        // Skip app-scoped translations (`en_vfo.lng`) in OS Language picker.
        let mut i = 0usize;
        while i < name.len() {
            if byte_at(name, i) == b'_' {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// Convert `/user/rogers/01_classic.ton` -> `Classic` into `out`.
#[inline(never)]
pub fn format_pretty_asset(path: &[u8], out: &mut [u8]) -> usize {
    let mut start = 0usize;
    let mut i = 0usize;
    while i < path.len() {
        if byte_at(path, i) == b'/' {
            start = i + 1;
        }
        i += 1;
    }
    let mut end = path.len();
    i = start;
    while i < path.len() {
        if byte_at(path, i) == b'.' {
            end = i;
        }
        i += 1;
    }
    // Strip leading numeric prefix like `01_`
    let mut p = start;
    while p < end && byte_at(path, p) >= b'0' && byte_at(path, p) <= b'9' {
        p += 1;
    }
    if p > start && p < end && byte_at(path, p) == b'_' {
        start = p + 1;
    }
    let mut w = 0usize;
    let mut cap_next = true;
    i = start;
    while i < end && w < out.len() {
        let mut ch = byte_at(path, i);
        if ch == b'_' || ch == b'-' {
            ch = b' ';
            cap_next = true;
        } else if cap_next {
            if ch >= b'a' && ch <= b'z' {
                ch -= 32;
            }
            cap_next = false;
        }
        set_byte(out, w, ch);
        w += 1;
        i += 1;
    }
    w
}

/// Read self-describing `__LANG_ASCII__` value from a `.lng` binary pack (if present).
#[inline(never)]
pub fn read_lng_ascii_name(
    api: &KrxApiTable,
    path: &[u8],
    hdr16: &mut [u8; 16],
    out: &mut [u8],
) -> usize {
    if api.read_file_at(path, 0, hdr16) < 12 {
        return 0;
    }
    // Check `LNG1` / `LNG2` header
    if byte_at(hdr16, 0) != b'L' || byte_at(hdr16, 1) != b'N' || byte_at(hdr16, 2) != b'G' {
        return 0;
    }
    let count = rd16(hdr16, 4) as u32;
    let str_base = (12 + count * 8) as u32;
    // FNV-1a 32-bit hash of `__LANG_ASCII__`
    let target = fnv1a_32(b"__LANG_ASCII__");
    let mut lo = 0u32;
    let mut hi = count;
    while lo < hi {
        let mid = (lo + hi) >> 1;
        if api.read_file_at(path, 12 + mid * 8, subslice_mut(hdr16, 0, 8)) < 8 {
            return 0;
        }
        let h = u32::from_le_bytes([
            byte_at(hdr16, 0),
            byte_at(hdr16, 1),
            byte_at(hdr16, 2),
            byte_at(hdr16, 3),
        ]);
        if h == target {
            let soff = rd16(hdr16, 4) as u32;
            let slen = (rd16(hdr16, 6) as usize).min(out.len());
            return api.read_file_at(path, str_base + soff, subslice_mut(out, 0, slen));
        } else if h < target {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    0
}

#[inline(never)]
pub fn fnv1a_32(bytes: &[u8]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= byte_at(bytes, i) as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

#[inline(always)]
pub fn hash_app_path(path: &[u8]) -> u16 {
    hash_app_action(path, 0)
}

/// Resolve `(action, is_overridden)` for shortcut `slot` (`0..19`) in `scope` (`0=Global, 1..4=Ctx`).
#[inline(never)]
pub fn get_slot_action_override(keys_cfg: &[u8; 64], slot: u8, scope: u8) -> (u16, bool) {
    if scope != 0 {
        let mut i = 0usize;
        while i < 3 {
            let o = 48 + i * 4;
            if byte_at(keys_cfg, o) == slot && byte_at(keys_cfg, o + 1) == scope {
                return (rd16(keys_cfg, o + 2), true);
            }
            i += 1;
        }
    }
    let o = 10 + (slot as usize) * 2;
    (rd16(keys_cfg, o), false)
}

#[inline(never)]
pub fn format_u32_into(api: &KrxApiTable, mut v: u32, dst: &mut [u8]) -> usize {
    if dst.is_empty() {
        return 0;
    }
    if v == 0 {
        set_byte(dst, 0, b'0');
        return 1;
    }
    let mut tmp = [0u8; 10];
    let mut n = 0usize;
    while v > 0 && n < 10 {
        let mut rem = 0u32;
        v = api.udivmod_rem(v, 10, &mut rem);
        set_byte(&mut tmp, n, b'0' + (rem as u8));
        n += 1;
    }
    let out_n = n.min(dst.len());
    let mut i = 0usize;
    while i < out_n {
        set_byte(dst, i, byte_at(&tmp, n - 1 - i));
        i += 1;
    }
    out_n
}

#[inline(never)]
pub fn push_u16_dec(buf: &mut [u8], mut pos: usize, mut val: u16, dec: u8) -> usize {
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
        set_byte(buf, pos, byte_at(&tmp, n));
        pos += 1;
    }
    pos
}

/// Draw a standard 13px menu row via `KRX_WIDGET_MENU_ITEM`.
#[inline(never)]
pub fn draw_row(
    api: &KrxApiTable,
    y: i16,
    label: &[u8],
    value: &[u8],
    icon_flags: u16,
    anim_tick: u16,
) {
    if y <= -ROW_STEP_Y || y >= 64 {
        return;
    }
    let desc = KrxWidget {
        p0: label.as_ptr(),
        p1: value.as_ptr(),
        x: 2,
        y,
        len0: label.len() as u16,
        len1: value.len() as u16,
        extra: anim_tick,
        w: 120,
        h: 13,
        a: (icon_flags >> 8) as u8,
        b: icon_flags as u8,
        _pad: [0; 2],
    };
    let _ = api.call_op(
        Op::UiWidget,
        KRX_WIDGET_MENU_ITEM as usize,
        &raw const desc as usize,
        0,
    );
}

#[inline(never)]
pub fn finish_list_frame(api: &KrxApiTable, scroll_px: i16, total: u8) {
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    if total > VISIBLE_ROWS {
        let pos = scroll_px.max(0) as u16;
        let tot = (total as u16) * (ROW_STEP_Y as u16);
        let vis = (VISIBLE_ROWS as u16) * (ROW_STEP_Y as u16);
        api.draw_scrollbar(125, 9, 52, pos, tot, vis);
    }
}

/// Set status bar title using caller's resident `SharedState::sheet_buf` (0 stack cost).
#[inline(never)]
pub fn set_status_title(api: &KrxApiTable, st: &mut SharedState, title: &[u8]) {
    let sb = subslice_mut(&mut st.sheet_buf, 0, 64);
    fill_bytes(sb, 0);
    set_byte(sb, 0, KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER | KRX_STATUS_CUSTOM_TITLE);
    let _ = copy_bytes(sb, 12, subslice(title, 0, 16));
    unsafe {
        api.set_status_bar(&*(sb.as_ptr() as *const KrxStatusBar));
    }
}

/// Show a confirm dialog using resident `SharedState` buffers for `ok`/`cancel` lifetimes.
#[inline(never)]
pub fn show_confirm_res(
    api: &KrxApiTable,
    st: &mut SharedState,
    tag: u8,
    title: &[u8],
    msg: &[u8],
    ok: &[u8],
    cancel: &[u8],
    hold_ms: u16,
) {
    fill_bytes(&mut st.confirm_ok, 0);
    fill_bytes(&mut st.confirm_cancel, 0);
    let ol = copy_bytes(&mut st.confirm_ok, 0, ok);
    let cl = copy_bytes(&mut st.confirm_cancel, 0, cancel);
    api.show_confirm(
        tag,
        title,
        msg,
        subslice(&st.confirm_ok, 0, ol),
        subslice(&st.confirm_cancel, 0, cl),
        hold_ms,
    );
}

/// Invoke a P1 v2 2 KiB domain block via `Op::ProcExec(PAGED_FAULT)` (12-cycle `.data.ramfunc` hit).
#[inline(never)]
pub fn call_page(api: &KrxApiTable, page_id: u8, st: &mut SharedState, cmd: u8, arg: u8) -> u32 {
    st.cmd = cmd;
    st.arg0 = arg;
    let addr = api.call_op(
        Op::ProcExec,
        sub::proc_exec::PAGED_FAULT as usize,
        page_id as usize,
        0,
    );
    if addr == 0 {
        return 0;
    }
    let f: unsafe extern "C" fn(u32, &KrxApiTable, *mut SharedState) -> u32 =
        unsafe { core::mem::transmute(addr) };
    unsafe { f(cmd as u32, api, st as *mut SharedState) }
}
