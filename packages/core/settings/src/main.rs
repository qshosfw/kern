//! Kern OS Settings — External P0-Paged Native `.krx` Application (Resident Root).
//!
//! Coordinates 9 declarative categories (with dynamic Security visibility), submenus,
//! stateful windowed kinetic scrolling, K1/K5-aware range editing & long-press repeat,
//! native OS `OptionSheet` pickers, and zero-stack-buffer `1 KB` P0 leaf overlays.
#![no_std]
#![no_main]

mod schema;
mod shared;
mod ui;

use core::mem::offset_of;
use kern_sdk::{
    KRX_EV_ENCODER, KRX_EV_KEY, KRX_EV_MODAL, KRX_EV_TICK, KRX_OP_EVENT, KRX_OP_EXIT,
    KRX_OP_RENDER, KRX_OP_START, KrxApiTable, KrxEvent, MODAL_RESULT_CONFIRMED,
    MODAL_RESULT_SHEET_SELECTED, MODAL_RESULT_TEXT_DONE, Op, SEC_FLAG_AVAILABLE, SEC_FLAG_IS_SET,
    format_u16_into, key, krx_entry, prop,
};

use schema::{
    ACT_ASSET_SPLASH, ACT_DEV_RADIO_ID, ACT_RESET_SETTINGS, ACT_SEC_DIGITS, ACT_SEC_EXPOSE,
    ACT_SEC_MAX_TRIES, ACT_SEC_PASSCODE, ACT_SUB_BATT_TWEAKS, ACT_SUB_SPLASH, ACT_SYS_INFO,
    ACT_SYS_STORAGE, CAT_COUNT, CAT_LANGUAGE, CAT_SECURITY, CH_BATT_K1, Kind, PROP_BATT_TWEAK,
    SUB_BATT_TWEAKS_COUNT, SUB_BATT_TWEAKS_START, SUB_BTN_TWEAKS_COUNT, SUB_BTN_TWEAKS_START,
    SUB_SPLASH_COUNT, SUB_SPLASH_START, batt_idx_to_type, cat_name, category, choice_options,
    format_value, item_at_slice, item_label, prop_bounds, resolve_choice,
};
use shared::{
    CMD_APPS_DO_DEL, CMD_APPS_LAUNCH, CMD_APPS_REFRESH, CMD_APPS_REQ_DEL, CMD_BP_LOAD,
    CMD_BP_RENDER, CMD_BP_TOGGLE_TX, CMD_DEV_BATT_TWEAK_SAVE, CMD_DEV_MODEL_PICK,
    CMD_DEV_MODEL_SHEET, CMD_DEV_TEXT_APPLY, CMD_DEV_TEXT_PROMPT, CMD_LANG_OPEN,
    CMD_SC_APPLY_PICK, CMD_SC_LOAD, CMD_SC_RENDER, CMD_SC_TOGGLE_DRAWER, CMD_SEC_ACTION,
    CMD_SEC_OPEN, CMD_SEC_VERIFY, CMD_SYS_DO_RESET, CMD_SYS_LAUNCH_MOD, CMD_SYS_REQ_REBOOT,
    CMD_SYS_REQ_RESET, CMD_SYS_SEC_TOGGLE, CMD_SYS_SEC_TRIES_OPEN, CMD_SYS_SEC_TRIES_PICK,
    LEAF_APP_HDR, LEAF_APP_LOAD, LEAF_APPS_OPS, LEAF_APPS_RENDER, LEAF_APPS_SCAN,
    LEAF_ASSET_APPLY, LEAF_ASSET_OPEN, LEAF_BANDPLANS, LEAF_DEV_APPLY, LEAF_DEV_LOAD,
    LEAF_DEV_PROMPT, LEAF_INFO_RENDER, LEAF_LANG_APPLY, LEAF_LANG_OPEN, LEAF_SC_APPLY,
    LEAF_SC_OPEN, LEAF_SC_RENDER, LEAF_SEC, LEAF_SEC_PIN, LEAF_SYS_INFO, LEAF_SYS_OPS,
    MODAL_ASSET_PICK, MODAL_CHOICE, MODAL_DELETE_APP, MODAL_REBOOT_RADIO, MODAL_RESET_SETTINGS, MODAL_SEC_ACTION,
    MODAL_SEC_CONFIRM_PIN, MODAL_SEC_VERIFY, MODAL_SHORTCUT_SCOPE, MODAL_TEXT_MODEL,
    MODAL_TEXT_OWNER, MODAL_TEXT_RADIO_ID, MODAL_TEXT_TAG, SharedState, cstr_slice,
};
use ui::KineticScroll;

/// Pack `(leaf: 1..=31, cmd: 0..=7)` into a single Thumb-1 `movs #imm8` byte!
#[inline(always)]
const fn lc(leaf: u8, cmd: u8) -> u8 {
    (leaf & 31) | ((cmd & 7) << 5)
}

#[inline(never)]
fn pget(api: &KrxApiTable, id: u8) -> u8 {
    api.prop_get(id)
}

#[inline(never)]
fn pset(api: &KrxApiTable, id: u8, val: u8) -> bool {
    api.pref_set(id, val)
}

/// Map navigation key to `-1` (up / left `<`), `+1` (down / right `>`), or `0`.
/// Matches resident `gfx::gui::modules::menu::{get_list_nav_dir, get_edit_change_dir}`:
/// - K1 (`nav == 0`): `UP`/`KEY2`/`KEY4` -> `-1`, `DOWN`/`KEY8`/`KEY6` -> `+1`.
/// - K5 (`nav != 0`): in edit mode `UP`/`KEY2`/`KEY6` -> `+1`, `DOWN`/`KEY8`/`KEY4` -> `-1`.
#[inline(never)]
fn key_dir(k: u8, nav: u8, editing: bool) -> i8 {
    let flip = editing && nav != 0;
    if k == key::UP || k == key::KEY2 {
        if flip { 1 } else { -1 }
    } else if k == key::DOWN || k == key::KEY8 {
        if flip { -1 } else { 1 }
    } else if editing || nav == 0 {
        if k == key::KEY4 { -1 } else if k == key::KEY6 { 1 } else { 0 }
    } else {
        0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum View {
    Home = 0,
    Category = 1,
    Submenu = 2,
    InstalledApps = 3,
    AppDetail = 4,
    Shortcuts = 5,
    BattInfo = 6,
    SysInfo = 7,
    Bandplans = 8,
}

#[repr(C, align(4))]
struct SettingsState {
    // 0..4: aligned 4-byte zero block in `on_start`
    cat_sel: u8,
    cat_top: u8,
    item_sel: u8,
    item_top: u8,
    // 4..8: aligned 4-byte zero block in `on_start`
    editing: bool,
    dirty: bool,
    batt_tweak_dirty: bool,
    /// `0` = no key held; otherwise `key_code + 1` (keeps `STATE` 100% zero-init `.bss`).
    held_key_p1: u8,
    // 8..16: other hot u8 scalars
    view: View,
    sub_start: u8,
    sub_count: u8,
    sub_sel: u8,
    sub_kind: u8,
    repeat_steps: u8,
    tick_ctr: u8,
    slide_x: i8,
    repeat_ms: u16,
    scroller: KineticScroll,
    sh: SharedState,
}

impl SettingsState {
    const fn new() -> Self {
        Self {
            view: View::Home,
            cat_sel: 0,
            cat_top: 0,
            item_sel: 0,
            item_top: 0,
            sub_start: 0,
            sub_count: 0,
            sub_sel: 0,
            sub_kind: 0,
            editing: false,
            dirty: false,
            batt_tweak_dirty: false,
            held_key_p1: 0,
            repeat_steps: 0,
            repeat_ms: 0,
            tick_ctr: 0,
            slide_x: 0,
            scroller: KineticScroll::new(),
            sh: SharedState::new(),
        }
    }

    #[inline(never)]
    fn repeat_delay(&self, api: &KrxApiTable, initial: bool) -> u16 {
        const DELAYS_CS: [u8; 8] = [30, 50, 70, 100, 5, 8, 12, 16];
        let pid = if initial { prop::HOLD_DELAY } else { prop::REPEAT_RATE };
        let idx = ((pget(api, pid) & 3) + if initial { 0 } else { 4 }) as usize;
        let mut ms = (unsafe { *DELAYS_CS.get_unchecked(idx) } as u16) * 10;
        if !initial && self.repeat_steps > 6 && pget(api, prop::FAST_SCROLL) != 0 {
            ms = ms.min(45);
        }
        ms
    }

    #[inline(always)]
    fn visible_cat_count(&self) -> u8 {
        if (self.sh.sec_status & SEC_FLAG_AVAILABLE) != 0 {
            CAT_COUNT as u8
        } else {
            (CAT_COUNT - 1) as u8
        }
    }

    #[inline(never)]
    fn cat_item_count(&self, cat: u8) -> u8 {
        if cat == CAT_SECURITY {
            if (self.sh.sec_status & SEC_FLAG_IS_SET) != 0 { 5 } else { 1 }
        } else {
            category(cat).count
        }
    }

    #[inline(never)]
    fn view_sel_ptr(&mut self, v: View) -> *mut u8 {
        const OFF: [u16; 9] = [
            offset_of!(SettingsState, cat_sel) as u16,
            offset_of!(SettingsState, item_sel) as u16,
            offset_of!(SettingsState, sub_sel) as u16,
            offset_of!(SettingsState, sh.app_sel) as u16,
            offset_of!(SettingsState, sh.detail_btn) as u16,
            offset_of!(SettingsState, sh.sc_sel) as u16,
            offset_of!(SettingsState, sh.info_sel) as u16,
            offset_of!(SettingsState, sh.info_sel) as u16,
            offset_of!(SettingsState, sh.bp_sel) as u16,
        ];
        unsafe { (self as *mut Self as *mut u8).add(*OFF.get_unchecked(v as usize) as usize) }
    }

    #[inline(never)]
    fn view_count(&self, v: View) -> u8 {
        match v {
            View::Home => self.visible_cat_count(),
            View::Category => self.cat_item_count(self.cat_sel),
            View::Submenu => self.sub_count,
            View::InstalledApps => self.sh.app_count,
            View::Shortcuts => 21,
            View::BattInfo | View::SysInfo => 5,
            View::Bandplans => self.sh.bp_count,
            View::AppDetail => 1,
        }
    }

    #[inline(never)]
    fn call_leaf(&mut self, api: &KrxApiTable, leaf_cmd: u8, arg0: u8) {
        self.sh.cmd = leaf_cmd >> 5;
        self.sh.arg0 = arg0;
        self.sh.scroll_y = self.scroller.cur_y;
        let _ = api.call_leaf(leaf_cmd & 31, &mut self.sh as *mut SharedState as u32);
    }

    #[inline(always)]
    fn current_slice(&self) -> (u8, u8) {
        if self.view == View::Submenu {
            (self.sub_start, self.sub_sel)
        } else {
            (category(self.cat_sel).start, self.item_sel)
        }
    }

    #[inline(never)]
    fn sync_status_bar(&mut self, api: &KrxApiTable) {
        const VIEW_LBL: [u8; 9] = [0, 0, 19, 31, 0, 14, 23, 37, 29];
        let (ptr, len): (*const u8, usize) = match self.view {
            View::Home => (b"Settings".as_ptr(), 8),
            View::Category => {
                let s = cat_name(self.cat_sel);
                (s.as_ptr(), s.len())
            }
            View::AppDetail => {
                let m = self.sh.meta();
                (m.name.as_ptr(), (m.name_len as usize).min(20))
            }
            v => {
                let lbl = if v == View::Submenu && self.sub_kind == ACT_SUB_SPLASH {
                    30
                } else if v == View::InstalledApps && self.sh.modules_only != 0 {
                    40
                } else {
                    unsafe { *VIEW_LBL.get_unchecked(v as usize) }
                };
                let s = item_label(lbl);
                (s.as_ptr(), s.len())
            }
        };
        ui::set_title(api, &mut self.sh.sheet_buf, unsafe {
            core::slice::from_raw_parts(ptr, len)
        });
    }

    #[inline(never)]
    fn flush_if_dirty(&mut self, api: &KrxApiTable) {
        if self.batt_tweak_dirty {
            self.call_leaf(api, lc(LEAF_DEV_PROMPT, CMD_DEV_BATT_TWEAK_SAVE), 0);
            self.batt_tweak_dirty = false;
        }
        if self.dirty {
            let _ = api.flush();
            self.dirty = false;
        }
    }

    #[inline(never)]
    fn on_start(&mut self, api: &KrxApiTable) {
        self.cat_sel = 0;
        self.cat_top = 0;
        self.item_sel = 0;
        self.item_top = 0;
        self.editing = false;
        self.dirty = false;
        self.batt_tweak_dirty = false;
        self.held_key_p1 = 0;
        self.sh.sec_status = api.sec_status();
        self.call_leaf(api, lc(LEAF_DEV_LOAD, 0), 0);
        self.view = View::Home;
        self.enter_view(api, View::Home, 0);
    }

    #[inline(never)]
    fn step_dir(&mut self, api: &KrxApiTable, dir: i8, is_repeat: bool) {
        if dir == 0 {
            return;
        }
        if self.view == View::AppDetail {
            let s = self.sh.detail_scroll;
            if dir < 0 && s > 0 {
                self.sh.detail_scroll = (s - 10).max(0);
            } else if dir > 0 && s < 34 {
                self.sh.detail_scroll = (s + 10).min(34);
            } else {
                self.sh.detail_btn ^= 1;
            }
            return;
        }

        let mut moved = false;
        if (self.view == View::Category || self.view == View::Submenu) && self.editing {
            let (st, row) = self.current_slice();
            let item = item_at_slice(st, row);
            if item.prop_id == PROP_BATT_TWEAK {
                let next = (self.sh.batt_tweak_mv + if dir > 0 { 10 } else { -10 }).clamp(-500, 500);
                if next != self.sh.batt_tweak_mv {
                    self.sh.batt_tweak_mv = next;
                    self.batt_tweak_dirty = true;
                    moved = true;
                }
            } else if let Some((min, max, step)) = prop_bounds(item.prop_id) {
                let cur = pget(api, item.prop_id);
                let next = if dir > 0 {
                    cur.saturating_add(step).min(max)
                } else {
                    cur.saturating_sub(step).max(min)
                };
                if next != cur && pset(api, item.prop_id, next) {
                    self.dirty = true;
                    moved = true;
                }
            }
        } else {
            let count = self.view_count(self.view);
            if count > 0 {
                let sel = unsafe { &mut *self.view_sel_ptr(self.view) };
                let prev = *sel;
                let wrapped = if dir < 0 {
                    if *sel > 0 { *sel -= 1; false } else { *sel = count - 1; true }
                } else if *sel + 1 < count {
                    *sel += 1; false
                } else {
                    *sel = 0; true
                };
                if *sel != prev {
                    moved = true;
                    let new_sel = *sel;
                    self.scroller.adjust(new_sel, count, !wrapped && pget(api, prop::UI_ANIMATIONS) != 0);
                    if wrapped && is_repeat {
                        self.repeat_ms = self.repeat_delay(api, true);
                    }
                }
            }
        }
        if moved && is_repeat && pget(api, prop::KEY_BEEP) != 0 {
            api.play_beep(5);
        }
    }

    #[inline(never)]
    fn enter_view(&mut self, api: &KrxApiTable, v: View, sel_row: u8) {
        let prev = self.view as u8;
        let next = v as u8;
        if prev == 0 && next != 0 {
            self.cat_top = self.scroller.top_row;
        } else if prev == 1 && next > 1 {
            self.item_top = self.scroller.top_row;
        }
        let saved_top = if next == 0 {
            self.cat_top
        } else if next == 1 && prev > 1 {
            self.item_top
        } else {
            0
        };
        self.view = v;
        let total = self.view_count(v);
        self.scroller.jump(sel_row, total, saved_top);
        self.slide_x = if prev != next && pget(api, prop::UI_ANIMATIONS) != 0 {
            if next > prev { 28 } else { -28 }
        } else {
            0
        };
        self.sync_status_bar(api);
    }

    #[inline(never)]
    fn on_confirm(&mut self, api: &KrxApiTable) {
        self.held_key_p1 = 0;
        match self.view {
            View::Home => {
                if self.cat_sel == CAT_LANGUAGE {
                    self.call_leaf(api, lc(LEAF_LANG_OPEN, CMD_LANG_OPEN), 0);
                    return;
                }
                if self.cat_sel == CAT_SECURITY {
                    self.sh.sec_status = api.sec_status();
                }
                self.item_sel = 0;
                self.editing = false;
                self.enter_view(api, View::Category, 0);
            }
            View::Category | View::Submenu => {
                let (st, row) = self.current_slice();
                let item = item_at_slice(st, row);
                match item.kind {
                    Kind::Toggle => {
                        if pset(api, item.prop_id, u8::from(pget(api, item.prop_id) == 0)) {
                            self.dirty = true;
                        }
                    }
                    Kind::Num => {
                        self.editing = !self.editing;
                        if !self.editing {
                            self.flush_if_dirty(api);
                        }
                    }
                    Kind::Choice => {
                        let raw = pget(api, item.prop_id);
                        let (tbl, sel) = resolve_choice(item.meta, raw, pget(api, prop::NAV_LAYOUT) == 0);
                        api.show_sheet(MODAL_CHOICE, item_label(item.label_idx), choice_options(tbl), sel, true);
                    }
                    Kind::Action => {
                        let m = item.meta;
                        if m >= ACT_RESET_SETTINGS {
                            const fn p(leaf: u8, cmd: u8, arg: u8) -> u16 {
                                (lc(leaf, cmd) as u16) | ((arg as u16) << 8)
                            }
                            const LEAF_ACTS: [u16; 18] = [
                                p(LEAF_SYS_OPS, CMD_SYS_REQ_RESET, 0),
                                p(LEAF_SYS_OPS, CMD_SYS_REQ_REBOOT, 0),
                                p(LEAF_ASSET_OPEN, 0, 0),
                                p(LEAF_ASSET_OPEN, 0, 1),
                                p(LEAF_ASSET_OPEN, 0, 2),
                                p(LEAF_LANG_OPEN, CMD_LANG_OPEN, 0),
                                p(LEAF_DEV_PROMPT, CMD_DEV_MODEL_SHEET, 0),
                                p(LEAF_DEV_PROMPT, CMD_DEV_TEXT_PROMPT, MODAL_TEXT_OWNER),
                                p(LEAF_DEV_PROMPT, CMD_DEV_TEXT_PROMPT, MODAL_TEXT_TAG),
                                p(LEAF_DEV_PROMPT, CMD_DEV_TEXT_PROMPT, MODAL_TEXT_RADIO_ID),
                                p(LEAF_SEC, CMD_SEC_OPEN, 0),
                                p(LEAF_SYS_OPS, CMD_SYS_SEC_TOGGLE, 1),
                                p(LEAF_SYS_OPS, CMD_SYS_SEC_TOGGLE, 2),
                                p(LEAF_SYS_OPS, CMD_SYS_SEC_TOGGLE, 3),
                                p(LEAF_SYS_OPS, CMD_SYS_SEC_TRIES_OPEN, 0),
                                p(LEAF_SYS_OPS, CMD_SYS_LAUNCH_MOD, 1),
                                p(LEAF_SYS_OPS, CMD_SYS_LAUNCH_MOD, 2),
                                p(LEAF_SYS_OPS, CMD_SYS_LAUNCH_MOD, 3),
                            ];
                            let v = unsafe {
                                *LEAF_ACTS.get_unchecked(((m - ACT_RESET_SETTINGS) as usize).min(17))
                            };
                            self.call_leaf(api, v as u8, (v >> 8) as u8);
                        } else if m >= ACT_SUB_SPLASH && m <= ACT_SUB_BATT_TWEAKS {
                            const SUBS: [(u8, u8); 3] = [
                                (SUB_SPLASH_START, SUB_SPLASH_COUNT),
                                (SUB_BTN_TWEAKS_START, SUB_BTN_TWEAKS_COUNT),
                                (SUB_BATT_TWEAKS_START, SUB_BATT_TWEAKS_COUNT),
                            ];
                            let (s, c) = unsafe { *SUBS.get_unchecked((m - ACT_SUB_SPLASH) as usize) };
                            self.sub_kind = m;
                            self.sub_start = s;
                            self.sub_count = c;
                            self.sub_sel = 0;
                            self.enter_view(api, View::Submenu, 0);
                        } else if m >= 1 && m <= ACT_SYS_INFO {
                            const DRILL: [(u8, u8, View); 6] = [
                                (lc(LEAF_APPS_SCAN, CMD_APPS_REFRESH), 0, View::InstalledApps),
                                (lc(LEAF_APPS_SCAN, CMD_APPS_REFRESH), 1, View::InstalledApps),
                                (lc(LEAF_SC_APPLY, CMD_SC_LOAD), 0, View::Shortcuts),
                                (lc(LEAF_BANDPLANS, CMD_BP_LOAD), 0, View::Bandplans),
                                (0, 0, View::BattInfo),
                                (lc(LEAF_SYS_OPS, CMD_SYS_LAUNCH_MOD), 0, View::SysInfo),
                            ];
                            let (lcmd, arg, v) = unsafe { *DRILL.get_unchecked((m - 1) as usize) };
                            if lcmd != 0 {
                                self.call_leaf(api, lcmd, arg);
                                if m == ACT_SYS_INFO && self.sh.result != 0 {
                                    return;
                                }
                            }
                            let sel_ptr = self.view_sel_ptr(v);
                            if m == 3 || m >= 5 {
                                unsafe { *sel_ptr = 0; }
                            }
                            let sel = unsafe { *sel_ptr };
                            self.enter_view(api, v, sel);
                        }
                    }
                }
            }
            View::InstalledApps if self.sh.app_count > 0 => {
                if self.sh.modules_only != 0 {
                    self.call_leaf(api, lc(LEAF_APPS_OPS, CMD_APPS_LAUNCH), 0);
                } else {
                    self.sh.detail_scroll = 0;
                    self.sh.detail_btn = 0;
                    self.call_leaf(api, lc(LEAF_APP_LOAD, 0), 0);
                    self.enter_view(api, View::AppDetail, 0);
                }
            }
            View::AppDetail => {
                let cmd = if self.sh.detail_btn == 0 { CMD_APPS_LAUNCH } else { CMD_APPS_REQ_DEL };
                self.call_leaf(api, lc(LEAF_APPS_OPS, cmd), 0);
            }
            View::Shortcuts => {
                let target = if self.sh.sc_sel == 1 { lc(LEAF_SC_APPLY, CMD_SC_TOGGLE_DRAWER) } else { lc(LEAF_SC_OPEN, 0) };
                self.call_leaf(api, target, 0);
            }
            View::Bandplans => self.call_leaf(api, lc(LEAF_BANDPLANS, CMD_BP_TOGGLE_TX), 0),
            _ => {}
        }
    }

    #[inline(never)]
    fn on_back(&mut self, api: &KrxApiTable) -> bool {
        self.held_key_p1 = 0;
        self.flush_if_dirty(api);
        if self.editing {
            self.editing = false;
            return true;
        }
        match self.view {
            View::Home => false,
            View::Category => {
                self.enter_view(api, View::Home, self.cat_sel);
                true
            }
            View::AppDetail => {
                self.call_leaf(api, lc(LEAF_APPS_SCAN, CMD_APPS_REFRESH), self.sh.modules_only);
                self.enter_view(api, View::InstalledApps, self.sh.app_sel);
                true
            }
            _ => {
                self.enter_view(api, View::Category, self.item_sel);
                true
            }
        }
    }

    #[inline(never)]
    fn on_modal(&mut self, api: &KrxApiTable, ev: &KrxEvent) {
        self.held_key_p1 = 0;
        let k = ev.key;
        let idx = ev.payload as u8;
        match ev.flags {
            MODAL_RESULT_SHEET_SELECTED => {
                if k == MODAL_CHOICE {
                    let (st, row) = self.current_slice();
                    let item = item_at_slice(st, row);
                    let val = if item.meta == CH_BATT_K1 {
                        batt_idx_to_type(idx, pget(api, prop::NAV_LAYOUT) == 0)
                    } else {
                        idx
                    };
                    if pset(api, item.prop_id, val) {
                        self.dirty = true;
                        self.flush_if_dirty(api);
                        if item.prop_id == prop::KEY_BEEP && val != 0 {
                            api.play_beep(val);
                        }
                    }
                } else if k == MODAL_SHORTCUT_SCOPE {
                    self.sh.sc_scope = idx.min(4);
                } else if k >= MODAL_ASSET_PICK && k <= MODAL_SEC_ACTION {
                    const SHEET_LEAVES: [u8; 6] = [
                        lc(LEAF_ASSET_APPLY, 0),
                        lc(LEAF_LANG_APPLY, 0),
                        lc(LEAF_DEV_APPLY, CMD_DEV_MODEL_PICK),
                        lc(LEAF_SC_APPLY, CMD_SC_APPLY_PICK),
                        lc(LEAF_SYS_OPS, CMD_SYS_SEC_TRIES_PICK),
                        lc(LEAF_SEC, CMD_SEC_ACTION),
                    ];
                    let target = unsafe { *SHEET_LEAVES.get_unchecked((k - MODAL_ASSET_PICK) as usize) };
                    self.call_leaf(api, target, idx);
                    if k == MODAL_SEC_ACTION && self.sh.result != 0 {
                        self.item_sel = 0;
                        self.scroller.jump(0, self.cat_item_count(self.cat_sel), 0);
                    }
                }
            }
            MODAL_RESULT_TEXT_DONE => {
                if k >= MODAL_TEXT_OWNER && k <= MODAL_TEXT_MODEL {
                    self.call_leaf(api, lc(LEAF_DEV_APPLY, CMD_DEV_TEXT_APPLY), k);
                } else if k >= MODAL_SEC_VERIFY && k <= MODAL_SEC_CONFIRM_PIN {
                    self.call_leaf(api, lc(LEAF_SEC_PIN, CMD_SEC_VERIFY + (k - MODAL_SEC_VERIFY)), 0);
                }
            }
            MODAL_RESULT_CONFIRMED => {
                if k == MODAL_DELETE_APP {
                    self.call_leaf(api, lc(LEAF_APPS_OPS, CMD_APPS_DO_DEL), 0);
                    if self.sh.result != 0 && self.view == View::AppDetail {
                        self.enter_view(api, View::InstalledApps, self.sh.app_sel);
                    }
                } else if k == MODAL_RESET_SETTINGS {
                    self.call_leaf(api, lc(LEAF_SYS_OPS, CMD_SYS_DO_RESET), 0);
                    self.call_leaf(api, lc(LEAF_DEV_LOAD, 0), 0);
                    self.sh.sec_status = api.sec_status();
                } else if k == MODAL_REBOOT_RADIO {
                    self.flush_if_dirty(api);
                    api.reboot(0);
                }
            }
            _ => {}
        }
    }

    #[inline(never)]
    fn on_event(&mut self, api: &KrxApiTable, ev: &KrxEvent) -> u32 {
        match ev.kind {
            KRX_EV_MODAL => {
                self.on_modal(api, ev);
                1
            }
            KRX_EV_ENCODER => {
                let delta = ev.payload as i32;
                if delta != 0 {
                    self.step_dir(api, if delta > 0 { 1 } else { -1 }, true);
                    1
                } else {
                    0
                }
            }
            KRX_EV_TICK => {
                self.tick_ctr = self.tick_ctr.wrapping_add(1);
                let mut dirty_ui = self.scroller.tick();
                if self.slide_x != 0 {
                    self.slide_x += if self.slide_x > 0 { -4 } else { 4 };
                    dirty_ui = true;
                }
                if self.held_key_p1 != 0 {
                    let dt = if ev.payload > 0 { (ev.payload as u16).min(200) } else { 50 };
                    if self.repeat_ms > dt {
                        self.repeat_ms -= dt;
                    } else {
                        let dir = key_dir(self.held_key_p1 - 1, pget(api, prop::NAV_LAYOUT), self.editing);
                        self.repeat_steps = self.repeat_steps.saturating_add(1);
                        self.repeat_ms = self.repeat_delay(api, false);
                        self.step_dir(api, dir, true);
                        dirty_ui = true;
                    }
                }
                let v = self.view as u8;
                let live_telem = (v & !1) == 6 && (self.tick_ctr & 15) == 0;
                let anim = v <= 3 && (self.tick_ctr & 3) == 0 && pget(api, prop::UI_ANIMATIONS) != 0;
                u32::from(dirty_ui || anim || live_telem)
            }
            KRX_EV_KEY => {
                let k = ev.key;
                if ev.payload != key::STATE_PRESSED {
                    if ev.payload == key::STATE_RELEASED && k.wrapping_add(1) == self.held_key_p1 {
                        self.held_key_p1 = 0;
                    }
                    return u32::from(k == key::MENU || k == key::KEY5);
                }
                self.slide_x = 0;
                let dir = key_dir(k, pget(api, prop::NAV_LAYOUT), self.editing);
                if dir != 0 {
                    self.held_key_p1 = k.wrapping_add(1);
                    self.repeat_steps = 0;
                    self.repeat_ms = self.repeat_delay(api, true);
                    self.step_dir(api, dir, false);
                    return 1;
                }
                if k == key::MENU || k == key::KEY5 {
                    self.on_confirm(api);
                    1
                } else if k == key::EXIT {
                    u32::from(self.on_back(api))
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    /// Unified list renderer for `View::Home` (`start == 0xFF`), `View::Category`, and `View::Submenu`.
    #[inline(never)]
    fn render_list(&mut self, api: &KrxApiTable, start: u8, count: u8, sel: u8, sy: i16, anim_tick: u16) {
        let is_k1 = pget(api, prop::NAV_LAYOUT) == 0;
        let sec = self.sh.sec_status;
        let mut idx = 0u8;
        while idx < count {
            let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - sy;
            if y > -ui::ROW_STEP_Y && y < 64 {
                let is_sel = idx == sel;
                let mut flags = u8::from(is_sel);
                let mut row_icon = 0u8;
                let (lbl, vstr): (&[u8], &[u8]) = if start == 0xFF {
                    row_icon = category(idx).icon_id;
                    let v: &[u8] = if idx == CAT_LANGUAGE {
                        cstr_slice(&self.sh.str_fields[3])
                    } else {
                        if is_sel { flags |= ui::FLAG_CHEVRON; }
                        b""
                    };
                    (cat_name(idx), v)
                } else {
                    let item = item_at_slice(start, idx);
                    let fbuf = unsafe { &mut *(self.sh.fmt_buf.as_mut_ptr() as *mut [u8; 16]) };
                    let vstr: &[u8] = match item.kind {
                        Kind::Action => {
                            let m = item.meta;
                            if m >= ACT_SEC_EXPOSE && m <= ACT_SEC_DIGITS {
                                let on = (sec & (1u32 << (m - ACT_SEC_PASSCODE))) != 0;
                                flags |= ui::FLAG_TOGGLE | if on { ui::FLAG_TOGGLE_ON } else { 0 };
                            } else if is_sel && (m <= ACT_SUB_BATT_TWEAKS || m >= ACT_SYS_STORAGE) {
                                flags |= ui::FLAG_CHEVRON;
                            }
                            if m >= ACT_ASSET_SPLASH && m <= ACT_DEV_RADIO_ID {
                                cstr_slice(unsafe { self.sh.str_fields.get_unchecked((m - ACT_ASSET_SPLASH) as usize) })
                            } else if m == ACT_SEC_MAX_TRIES {
                                format_u16_into(((sec >> 16) & 0xFF) as u16, 0, b"", fbuf)
                            } else if m == ACT_SEC_PASSCODE {
                                if (sec & SEC_FLAG_IS_SET) != 0 { b"Set" } else { b"None" }
                            } else {
                                b""
                            }
                        }
                        Kind::Toggle => {
                            let on = pget(api, item.prop_id) != 0;
                            flags |= ui::FLAG_TOGGLE | if on { ui::FLAG_TOGGLE_ON } else { 0 };
                            b""
                        }
                        _ => {
                            let val: i16 = if item.prop_id == PROP_BATT_TWEAK {
                                self.sh.batt_tweak_mv
                            } else {
                                pget(api, item.prop_id) as i16
                            };
                            if self.editing && is_sel {
                                flags |= ui::FLAG_OUTLINE;
                                let (lo, hi) = if item.prop_id == PROP_BATT_TWEAK {
                                    (-500i16, 500i16)
                                } else if let Some((min, max, _)) = prop_bounds(item.prop_id) {
                                    (min as i16, max as i16)
                                } else {
                                    (0, 0)
                                };
                                if val > lo { flags |= ui::FLAG_ARROW_LEFT; }
                                if val < hi { flags |= ui::FLAG_ARROW_RIGHT; }
                            }
                            format_value(item, val, is_k1, fbuf)
                        }
                    };
                    (item_label(item.label_idx), vstr)
                };
                ui::draw_row(
                    api,
                    y,
                    lbl,
                    vstr,
                    (row_icon as u16) | ((flags as u16) << 8),
                    anim_tick,
                );
            }
            idx += 1;
        }
        ui::finish_list_frame(api, sy, count);
    }

    #[inline(never)]
    fn render(&mut self, api: &KrxApiTable) {
        api.clear();
        let sy = self.scroller.cur_y;
        let anim_tick = if pget(api, prop::UI_ANIMATIONS) != 0 {
            (((self.tick_ctr >> 2) as u16) << 1) | 1
        } else {
            0
        };
        self.sh.anim_tick = anim_tick;
        match self.view {
            View::Home => self.render_list(api, 0xFF, self.visible_cat_count(), self.cat_sel, sy, anim_tick),
            View::Category => {
                let c = category(self.cat_sel);
                self.render_list(api, c.start, self.cat_item_count(self.cat_sel), self.item_sel, sy, anim_tick);
            }
            View::Submenu => self.render_list(api, self.sub_start, self.sub_count, self.sub_sel, sy, anim_tick),
            v => {
                const VIEW_LEAVES: [u8; 6] = [
                    lc(LEAF_APPS_RENDER, 0),
                    lc(LEAF_APP_HDR, 0),
                    lc(LEAF_SC_RENDER, CMD_SC_RENDER),
                    lc(LEAF_INFO_RENDER, 0),
                    lc(LEAF_SYS_INFO, 0),
                    lc(LEAF_BANDPLANS, CMD_BP_RENDER),
                ];
                let target = unsafe { *VIEW_LEAVES.get_unchecked((v as usize) - 3) };
                self.call_leaf(api, target, 0);
            }
        }
        if self.slide_x != 0 {
            let _ = api.call_op(Op::GfxDraw, abi::sub::gfx_draw::SCROLL as usize, (self.slide_x as u8) as usize, 0);
        }
    }
}

static mut STATE: SettingsState = SettingsState::new();

fn app_entry(op: u32, api: &KrxApiTable, arg: *const ()) -> u32 {
    let state = unsafe { &mut *core::ptr::addr_of_mut!(STATE) };
    match op {
        KRX_OP_START => {
            state.on_start(api);
            1
        }
        KRX_OP_EVENT => {
            if arg.is_null() {
                return 0;
            }
            let ev = unsafe { &*(arg as *const KrxEvent) };
            state.on_event(api, ev)
        }
        KRX_OP_RENDER => {
            state.render(api);
            1
        }
        KRX_OP_EXIT => {
            state.flush_if_dirty(api);
            1
        }
        _ => 0,
    }
}

krx_entry!(app_entry);
