//! Shared `#[repr(C)]` State & Leaf Dispatch Contract for `settings.krx` (`P0` Paged).

use kern_sdk::KrxDirEntry;

pub const MAX_APPS: usize = 6;
pub const MAX_ASSETS: usize = 6;

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
pub const LEAF_STORAGE: u8 = 18;

// Modal tags shared across root and leaves
pub const MODAL_CHOICE: u8 = 30;
pub const MODAL_ASSET_PICK: u8 = 31;
pub const MODAL_MODEL_PICK: u8 = 32;
pub const MODAL_TEXT_OWNER: u8 = 33;
pub const MODAL_TEXT_TAG: u8 = 34;
pub const MODAL_TEXT_RADIO_ID: u8 = 35;
pub const MODAL_TEXT_MODEL: u8 = 36;
pub const MODAL_SHORTCUT_SCOPE: u8 = 37;
pub const MODAL_SHORTCUT_PICK: u8 = 38;
pub const MODAL_DELETE_APP: u8 = 40;
pub const MODAL_RESET_SETTINGS: u8 = 41;
pub const MODAL_REBOOT_RADIO: u8 = 42;

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

pub const CMD_APPS_REFRESH: u8 = 1;
pub const CMD_APPS_LAUNCH: u8 = 2;
pub const CMD_APPS_REQ_DEL: u8 = 3;
pub const CMD_APPS_DO_DEL: u8 = 4;

pub const CMD_INFO_BATT: u8 = 1;
pub const CMD_INFO_SYS: u8 = 2;

pub const CMD_SYS_REQ_RESET: u8 = 1;
pub const CMD_SYS_REQ_REBOOT: u8 = 2;
pub const CMD_SYS_DO_RESET: u8 = 3;

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
    pub _pad: u8,
    pub size: u32,
    pub caps: u32,
    pub icon_16: [u8; 32],
}

#[repr(C)]
pub union SharedPool {
    pub app_entries: [KrxDirEntry; MAX_APPS],
    pub asset_paths: [[u8; 32]; MAX_ASSETS],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ScBuf {
    pub keys_cfg: [u8; 64],
    pub target_ids: [u16; 12],
}

#[repr(C)]
pub union DetailOrSc {
    pub app_meta: AppMeta,
    pub sc: ScBuf,
}

#[repr(C)]
pub struct SharedState {
    pub cmd: u8,
    pub arg0: u8,
    pub info_sel: u8,
    pub result: u8,
    pub scroll_y: i16,
    pub anim_tick: u16,

    pub dev_model_idx: u8,
    pub dev_owner: [u8; 12],
    pub dev_tag: [u8; 8],
    pub dev_model: [u8; 12],
    pub dev_radio_id: u32,

    pub splash_name: [u8; 12],
    pub roger_name: [u8; 12],
    pub jingle_name: [u8; 12],
    pub lang_name: [u8; 12],

    pub sheet_buf: [u8; 88],
    pub sheet_len: u16,
    pub sheet_title: [u8; 16],
    pub sheet_title_len: u8,
    pub asset_domain: u8,
    pub asset_count: u8,
    pub sc_scope: u8,
    pub sc_sel: u8,
    pub sc_slot: u8,
    pub sc_target_count: u8,
    pub app_count: u8,
    pub app_sel: u8,
    pub modules_only: u8,
    pub detail_btn: u8,
    pub bp_count: u8,
    pub bp_sel: u8,
    pub detail_scroll: i16,
    pub pool: SharedPool,
    pub detail_sc: DetailOrSc,
}

impl SharedState {
    pub const fn new() -> Self {
        Self {
            cmd: 0,
            arg0: 0,
            info_sel: 0,
            result: 0,
            scroll_y: 0,
            anim_tick: 0,
            dev_model_idx: 0,
            dev_owner: [0; 12],
            dev_tag: [0; 8],
            dev_model: [0; 12],
            dev_radio_id: 1,
            splash_name: [0; 12],
            roger_name: [0; 12],
            jingle_name: [0; 12],
            lang_name: [0; 12],
            sheet_buf: [0; 88],
            sheet_len: 0,
            sheet_title: [0; 16],
            sheet_title_len: 0,
            asset_domain: 0,
            asset_count: 0,
            sc_scope: 0,
            sc_sel: 0,
            sc_slot: 0,
            sc_target_count: 0,
            app_count: 0,
            app_sel: 0,
            modules_only: 0,
            detail_btn: 0,
            bp_count: 0,
            bp_sel: 0,
            detail_scroll: 0,
            pool: SharedPool {
                app_entries: [KrxDirEntry {
                    name: [0; 24],
                    size: 0,
                    is_dir: 0,
                    reserved: [0; 3],
                }; MAX_APPS],
            },
            detail_sc: DetailOrSc {
                sc: ScBuf {
                    keys_cfg: [0; 64],
                    target_ids: [0; 12],
                },
            },
        }
    }

    #[inline(always)]
    pub fn apps(&self) -> &[KrxDirEntry; MAX_APPS] {
        unsafe { &self.pool.app_entries }
    }

    #[inline(always)]
    pub fn apps_mut(&mut self) -> &mut [KrxDirEntry; MAX_APPS] {
        unsafe { &mut self.pool.app_entries }
    }

    #[inline(always)]
    pub fn assets(&self) -> &[[u8; 32]; MAX_ASSETS] {
        unsafe { &self.pool.asset_paths }
    }

    #[inline(always)]
    pub fn assets_mut(&mut self) -> &mut [[u8; 32]; MAX_ASSETS] {
        unsafe { &mut self.pool.asset_paths }
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

#[inline(always)]
pub fn fill_bytes(s: &mut [u8], v: u8) {
    let mut i = 0usize;
    while i < s.len() {
        unsafe { *s.get_unchecked_mut(i) = v };
        i += 1;
    }
}

#[inline(always)]
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

#[inline(always)]
pub fn trimmed_slice(buf: &[u8]) -> &[u8] {
    let mut n = 0usize;
    while n < buf.len() {
        let b = unsafe { *buf.get_unchecked(n) };
        if b == 0 || b == b' ' { break; }
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(buf.as_ptr(), n) }
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
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < path.len() {
        h ^= unsafe { *path.get_unchecked(i) } as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    ((h ^ (h >> 16)) as u16) | 0x8000
}
