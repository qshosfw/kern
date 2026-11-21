//! Kern OS Settings — External P0-Paged Native `.krx` Application (Resident Root).
//!
//! Coordinates 7 full categories, submenus, kinetic scrolling, long-press repeat,
//! native OS `OptionSheet` pickers, and dispatches heavy views/editors to 11 `1 KB` P0 leaves.
#![no_std]
#![no_main]

mod schema;
mod shared;
mod ui;

use kern_sdk::{
    KRX_EV_ENCODER, KRX_EV_KEY, KRX_EV_MODAL, KRX_EV_TICK, KRX_OP_EVENT, KRX_OP_EXIT,
    KRX_OP_RENDER, KRX_OP_START, KrxApiTable, KrxEvent, MODAL_RESULT_CONFIRMED,
    MODAL_RESULT_SHEET_SELECTED, MODAL_RESULT_TEXT_DONE, format_u16_into, key, krx_entry, prop,
};

use schema::{
    ACT_ASSET_JINGLE, ACT_ASSET_LANG, ACT_ASSET_ROGER, ACT_ASSET_SPLASH, ACT_BANDPLANS,
    ACT_DEV_MODEL, ACT_DEV_OWNER, ACT_DEV_RADIO_ID, ACT_DEV_TAG, ACT_INSTALLED_APPS,
    ACT_REBOOT_RADIO, ACT_RESET_SETTINGS, ACT_SHORTCUTS, ACT_SUB_BATT_INFO, ACT_SUB_BATT_TWEAKS,
    ACT_SUB_BTN_TWEAKS, ACT_SUB_SPLASH, ACT_SYS_INFO, ACT_SYS_MODULES, CAT_COUNT, CH_BATT_K1, Kind,
    SUB_BATT_TWEAKS_COUNT, SUB_BATT_TWEAKS_START, SUB_BTN_TWEAKS_COUNT, SUB_BTN_TWEAKS_START,
    SUB_SPLASH_COUNT, SUB_SPLASH_START, batt_idx_to_type, batt_type_to_idx, cat_name, category,
    choice_options, format_value, item_at_slice, item_label,
};
use shared::{
    CMD_APPS_DO_DEL, CMD_APPS_LAUNCH, CMD_APPS_REFRESH, CMD_APPS_REQ_DEL, CMD_BP_LOAD,
    CMD_BP_RENDER, CMD_BP_TOGGLE_TX, CMD_DEV_MODEL_PICK, CMD_DEV_MODEL_SHEET, CMD_DEV_TEXT_APPLY,
    CMD_DEV_TEXT_PROMPT, CMD_INFO_BATT, CMD_INFO_SYS, CMD_SC_APPLY_PICK, CMD_SC_LOAD,
    CMD_SC_RENDER, CMD_SC_TOGGLE_DRAWER, CMD_SYS_DO_RESET, CMD_SYS_REQ_REBOOT, CMD_SYS_REQ_RESET,
    LEAF_APP_HDR, LEAF_APP_LOAD, LEAF_APPS_OPS, LEAF_APPS_RENDER, LEAF_ASSET_APPLY,
    LEAF_ASSET_OPEN, LEAF_BANDPLANS, LEAF_DEV_APPLY, LEAF_DEV_LOAD, LEAF_DEV_PROMPT,
    LEAF_INFO_RENDER, LEAF_SC_APPLY, LEAF_SC_OPEN, LEAF_SC_RENDER, LEAF_SYS_OPS, MODAL_ASSET_PICK,
    MODAL_CHOICE, MODAL_DELETE_APP, MODAL_MODEL_PICK, MODAL_REBOOT_RADIO, MODAL_RESET_SETTINGS,
    MODAL_SHORTCUT_PICK, MODAL_SHORTCUT_SCOPE, MODAL_TEXT_MODEL, MODAL_TEXT_OWNER,
    MODAL_TEXT_RADIO_ID, MODAL_TEXT_TAG, SharedState, cstr_slice, subslice, trimmed_slice,
};
use ui::KineticScroll;

const REPEAT_DELAY_TICKS: u8 = 7;
const REPEAT_RATE_TICKS: u8 = 2;

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

struct SettingsState {
    view: View,
    cat_sel: u8,
    item_sel: u8,
    sub_start: u8,
    sub_count: u8,
    sub_sel: u8,
    sub_kind: u8,
    editing: bool,
    dirty: bool,
    held_key: u8,
    repeat_cd: u8,
    tick_ctr: u8,
    scroller: KineticScroll,
    sh: SharedState,
}

impl SettingsState {
    const fn new() -> Self {
        Self {
            view: View::Home,
            cat_sel: 0,
            item_sel: 0,
            sub_start: 0,
            sub_count: 0,
            sub_sel: 0,
            sub_kind: 0,
            editing: false,
            dirty: false,
            held_key: 0xFF,
            repeat_cd: 0,
            tick_ctr: 0,
            scroller: KineticScroll::new(),
            sh: SharedState::new(),
        }
    }

    #[inline(never)]
    fn call_leaf(&mut self, api: &KrxApiTable, leaf_id: u8, cmd: u8, arg0: u8) {
        self.sh.cmd = cmd;
        self.sh.arg0 = arg0;
        self.sh.scroll_y = self.scroller.cur_y;
        let ptr = &mut self.sh as *mut SharedState as u32;
        let _ = api.call_leaf(leaf_id, ptr);
    }

    #[inline(always)]
    fn current_slice(&self) -> (u8, u8, u8) {
        if self.view == View::Submenu {
            (self.sub_start, self.sub_count, self.sub_sel)
        } else {
            let c = category(self.cat_sel);
            (c.start, c.count, self.item_sel)
        }
    }

    fn sync_status_bar(&self, api: &KrxApiTable) {
        const VT: &[u8] = b"Settings\0Tweaks\0Installed Apps\0Modules\0Shortcuts\0Battery Info\0System Info\0Boot Splash\0Bandplans\0";
        let title: &[u8] = match self.view {
            View::Home => kern_sdk::nth_packed_slice(VT, 0),
            View::Category => cat_name(self.cat_sel),
            View::Submenu => kern_sdk::nth_packed_slice(VT, if self.sub_kind == ACT_SUB_SPLASH { 7 } else { 1 }),
            View::InstalledApps => kern_sdk::nth_packed_slice(VT, if self.sh.modules_only != 0 { 3 } else { 2 }),
            View::AppDetail => subslice(&self.sh.meta().name, 0, self.sh.meta().name_len as usize),
            View::Shortcuts => kern_sdk::nth_packed_slice(VT, 4),
            View::BattInfo => kern_sdk::nth_packed_slice(VT, 5),
            View::SysInfo => kern_sdk::nth_packed_slice(VT, 6),
            View::Bandplans => kern_sdk::nth_packed_slice(VT, 8),
        };
        ui::set_title(api, title);
    }

    fn flush_if_dirty(&mut self, api: &KrxApiTable) {
        if self.dirty {
            let _ = api.flush();
            self.dirty = false;
        }
    }

    fn on_start(&mut self, api: &KrxApiTable) {
        self.view = View::Home;
        self.cat_sel = 0;
        self.item_sel = 0;
        self.sub_sel = 0;
        self.sh.info_sel = 0;
        self.editing = false;
        self.dirty = false;
        self.held_key = 0xFF;
        self.repeat_cd = 0;
        self.tick_ctr = 0;
        self.scroller.jump_to_row(0);
        self.call_leaf(api, LEAF_DEV_LOAD, 0, 0);
        self.sync_status_bar(api);
    }

    #[inline(never)]
    fn step_cursor(&mut self, api: &KrxApiTable, up: bool, is_repeat: bool) {
        if self.view == View::AppDetail {
            let max_s: i16 = 34;
            if up {
                if self.sh.detail_scroll > 0 {
                    self.sh.detail_scroll = (self.sh.detail_scroll - 10).max(0);
                } else {
                    self.sh.detail_btn ^= 1;
                }
            } else if self.sh.detail_scroll < max_s {
                self.sh.detail_scroll = (self.sh.detail_scroll + 10).min(max_s);
            } else {
                self.sh.detail_btn ^= 1;
            }
            return;
        }

        if (self.view == View::Category || self.view == View::Submenu) && self.editing {
            let (st, _, row) = self.current_slice();
            let item = item_at_slice(st, row);
            if let Some((min, max, step)) = prop::lookup_bounds(item.prop_id) {
                let cur = api.prop_get(item.prop_id);
                let next = if up {
                    if cur >= max { if is_repeat { max } else { min } } else { cur.saturating_add(step).min(max) }
                } else if cur <= min {
                    if is_repeat { min } else { max }
                } else {
                    cur.saturating_sub(step).max(min)
                };
                if next != cur && api.pref_set(item.prop_id, next) {
                    self.dirty = true;
                    if is_repeat && api.prop_get(prop::KEY_BEEP) != 0 {
                        api.play_beep(5);
                    }
                }
            }
            return;
        }

        let (sel, count): (&mut u8, u8) = match self.view {
            View::Home => (&mut self.cat_sel, CAT_COUNT as u8),
            View::Category => (&mut self.item_sel, category(self.cat_sel).count),
            View::Submenu => (&mut self.sub_sel, self.sub_count),
            View::InstalledApps => (&mut self.sh.app_sel, self.sh.app_count),
            View::Shortcuts => (&mut self.sh.sc_sel, 21),
            View::BattInfo | View::SysInfo => (&mut self.sh.info_sel, 5),
            View::Bandplans => (&mut self.sh.bp_sel, self.sh.bp_count),
            View::AppDetail => return,
        };
        if count == 0 {
            return;
        }
        let prev = *sel;
        let wrapped = if up {
            if *sel > 0 { *sel -= 1; false } else { *sel = count - 1; true }
        } else if *sel + 1 < count {
            *sel += 1; false
        } else {
            *sel = 0; true
        };
        if *sel != prev {
            let anim = !wrapped && api.prop_get(prop::UI_ANIMATIONS) != 0;
            self.scroller.set_target_row(ui::scroll_top(*sel), anim);
            if wrapped && is_repeat {
                self.repeat_cd = REPEAT_DELAY_TICKS;
            }
            if is_repeat && api.prop_get(prop::KEY_BEEP) != 0 {
                api.play_beep(5);
            }
        }
    }

    #[inline(never)]
    fn on_confirm(&mut self, api: &KrxApiTable) {
        self.held_key = 0xFF;
        match self.view {
            View::Home => {
                self.view = View::Category;
                self.item_sel = 0;
                self.editing = false;
                self.scroller.jump_to_row(0);
                self.sync_status_bar(api);
            }
            View::Category | View::Submenu => {
                let (st, _, row) = self.current_slice();
                let item = item_at_slice(st, row);
                match item.kind {
                    Kind::Toggle => {
                        let cur = api.prop_get(item.prop_id);
                        if api.pref_set(item.prop_id, u8::from(cur == 0)) {
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
                        let raw = api.prop_get(item.prop_id);
                        let sel = if item.meta == CH_BATT_K1 { batt_type_to_idx(raw) } else { raw };
                        api.show_sheet(
                            MODAL_CHOICE,
                            item_label(item.label_idx),
                            choice_options(item.meta),
                            sel,
                            true,
                        );
                    }
                    Kind::Action => match item.meta {
                        ACT_INSTALLED_APPS | ACT_SYS_MODULES => {
                            let mod_only = u8::from(item.meta == ACT_SYS_MODULES);
                            self.call_leaf(api, LEAF_APPS_OPS, CMD_APPS_REFRESH, mod_only);
                            self.view = View::InstalledApps;
                            self.scroller.jump_to_row(ui::scroll_top(self.sh.app_sel));
                            self.sync_status_bar(api);
                        }
                        ACT_SYS_INFO | ACT_SUB_BATT_INFO => {
                            self.sh.info_sel = 0;
                            self.view = if item.meta == ACT_SYS_INFO { View::SysInfo } else { View::BattInfo };
                            self.scroller.jump_to_row(0);
                            self.sync_status_bar(api);
                        }
                        ACT_SUB_SPLASH | ACT_SUB_BTN_TWEAKS | ACT_SUB_BATT_TWEAKS => {
                            const SUBS: [(u8, u8); 3] = [
                                (SUB_SPLASH_START, SUB_SPLASH_COUNT),
                                (SUB_BTN_TWEAKS_START, SUB_BTN_TWEAKS_COUNT),
                                (SUB_BATT_TWEAKS_START, SUB_BATT_TWEAKS_COUNT),
                            ];
                            let idx = match item.meta {
                                ACT_SUB_SPLASH => 0,
                                ACT_SUB_BTN_TWEAKS => 1,
                                _ => 2,
                            };
                            let (s, c) = unsafe { *SUBS.get_unchecked(idx) };
                            self.sub_kind = item.meta;
                            self.sub_start = s;
                            self.sub_count = c;
                            self.sub_sel = 0;
                            self.view = View::Submenu;
                            self.scroller.jump_to_row(0);
                            self.sync_status_bar(api);
                        }
                        ACT_SHORTCUTS => {
                            self.sh.sc_sel = 0;
                            self.call_leaf(api, LEAF_SC_APPLY, CMD_SC_LOAD, 0);
                            self.view = View::Shortcuts;
                            self.scroller.jump_to_row(0);
                            self.sync_status_bar(api);
                        }
                        ACT_BANDPLANS => {
                            self.call_leaf(api, LEAF_BANDPLANS, CMD_BP_LOAD, 0);
                            self.view = View::Bandplans;
                            self.scroller.jump_to_row(ui::scroll_top(self.sh.bp_sel));
                            self.sync_status_bar(api);
                        }
                        ACT_ASSET_SPLASH..=ACT_ASSET_LANG => {
                            self.call_leaf(api, LEAF_ASSET_OPEN, 0, item.meta - ACT_ASSET_SPLASH);
                        }
                        ACT_DEV_MODEL => self.call_leaf(api, LEAF_DEV_PROMPT, CMD_DEV_MODEL_SHEET, 0),
                        ACT_DEV_OWNER..=ACT_DEV_RADIO_ID => {
                            self.call_leaf(api, LEAF_DEV_PROMPT, CMD_DEV_TEXT_PROMPT, MODAL_TEXT_OWNER + (item.meta - ACT_DEV_OWNER));
                        }
                        ACT_RESET_SETTINGS => self.call_leaf(api, LEAF_SYS_OPS, CMD_SYS_REQ_RESET, 0),
                        ACT_REBOOT_RADIO => self.call_leaf(api, LEAF_SYS_OPS, CMD_SYS_REQ_REBOOT, 0),
                        _ => {}
                    },
                }
            }
            View::InstalledApps => {
                if self.sh.app_count == 0 {
                    return;
                }
                if self.sh.modules_only != 0 {
                    self.call_leaf(api, LEAF_APPS_OPS, CMD_APPS_LAUNCH, 0);
                } else {
                    self.sh.detail_scroll = 0;
                    self.sh.detail_btn = 0;
                    self.call_leaf(api, LEAF_APP_LOAD, 0, 0);
                    self.view = View::AppDetail;
                    self.sync_status_bar(api);
                }
            }
            View::AppDetail => {
                let cmd = if self.sh.detail_btn == 0 { CMD_APPS_LAUNCH } else { CMD_APPS_REQ_DEL };
                self.call_leaf(api, LEAF_APPS_OPS, cmd, 0);
            }
            View::Shortcuts => {
                if self.sh.sc_sel == 1 {
                    self.call_leaf(api, LEAF_SC_APPLY, CMD_SC_TOGGLE_DRAWER, 0);
                } else {
                    self.call_leaf(api, LEAF_SC_OPEN, 0, 0);
                }
            }
            View::Bandplans => self.call_leaf(api, LEAF_BANDPLANS, CMD_BP_TOGGLE_TX, 0),
            View::BattInfo | View::SysInfo => {}
        }
    }

    #[inline(never)]
    fn on_back(&mut self, api: &KrxApiTable) -> bool {
        self.held_key = 0xFF;
        if self.editing {
            self.editing = false;
            self.flush_if_dirty(api);
            return true;
        }
        match self.view {
            View::Home => {
                self.flush_if_dirty(api);
                false
            }
            View::Category => {
                self.flush_if_dirty(api);
                self.view = View::Home;
                self.scroller.jump_to_row(ui::scroll_top(self.cat_sel));
                self.sync_status_bar(api);
                true
            }
            View::AppDetail => {
                self.view = View::InstalledApps;
                self.scroller.jump_to_row(ui::scroll_top(self.sh.app_sel));
                self.sync_status_bar(api);
                true
            }
            _ => {
                self.flush_if_dirty(api);
                self.view = View::Category;
                self.scroller.jump_to_row(ui::scroll_top(self.item_sel));
                self.sync_status_bar(api);
                true
            }
        }
    }

    fn on_modal(&mut self, api: &KrxApiTable, ev: &KrxEvent) {
        self.held_key = 0xFF;
        match ev.flags {
            MODAL_RESULT_SHEET_SELECTED => {
                let idx = ev.payload as u8;
                match ev.key {
                    MODAL_CHOICE => {
                        let (st, _, row) = self.current_slice();
                        let item = item_at_slice(st, row);
                        let val = if item.meta == CH_BATT_K1 { batt_idx_to_type(idx) } else { idx };
                        if api.pref_set(item.prop_id, val) {
                            self.dirty = true;
                            self.flush_if_dirty(api);
                        }
                    }
                    MODAL_ASSET_PICK => self.call_leaf(api, LEAF_ASSET_APPLY, 0, idx),
                    MODAL_MODEL_PICK => self.call_leaf(api, LEAF_DEV_APPLY, CMD_DEV_MODEL_PICK, idx),
                    MODAL_SHORTCUT_SCOPE => self.sh.sc_scope = idx.min(4),
                    MODAL_SHORTCUT_PICK => self.call_leaf(api, LEAF_SC_APPLY, CMD_SC_APPLY_PICK, idx),
                    _ => {}
                }
            }
            MODAL_RESULT_TEXT_DONE => {
                if matches!(ev.key, MODAL_TEXT_OWNER | MODAL_TEXT_TAG | MODAL_TEXT_RADIO_ID | MODAL_TEXT_MODEL) {
                    self.call_leaf(api, LEAF_DEV_APPLY, CMD_DEV_TEXT_APPLY, ev.key);
                }
            }
            MODAL_RESULT_CONFIRMED => match ev.key {
                MODAL_DELETE_APP => {
                    self.call_leaf(api, LEAF_APPS_OPS, CMD_APPS_DO_DEL, 0);
                    if self.sh.result != 0 && self.view == View::AppDetail {
                        self.view = View::InstalledApps;
                        self.scroller.jump_to_row(ui::scroll_top(self.sh.app_sel));
                        self.sync_status_bar(api);
                    }
                }
                MODAL_RESET_SETTINGS => self.call_leaf(api, LEAF_SYS_OPS, CMD_SYS_DO_RESET, 0),
                MODAL_REBOOT_RADIO => {
                    let _ = api.flush();
                    api.reboot(0);
                }
                _ => {}
            },
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
                    self.step_cursor(api, delta < 0, false);
                    return 1;
                }
                0
            }
            KRX_EV_TICK => {
                self.tick_ctr = self.tick_ctr.wrapping_add(1);
                let mut dirty_ui = self.scroller.tick();
                if self.held_key != 0xFF {
                    if self.repeat_cd > 0 {
                        self.repeat_cd -= 1;
                    }
                    if self.repeat_cd == 0 {
                        let up = self.held_key == key::UP || self.held_key == key::KEY2;
                        self.repeat_cd = REPEAT_RATE_TICKS;
                        self.step_cursor(api, up, true);
                        dirty_ui = true;
                    }
                }
                let anim = api.prop_get(prop::UI_ANIMATIONS) != 0
                    && matches!(self.view, View::Home | View::Category | View::Submenu | View::InstalledApps);
                u32::from(dirty_ui || anim || self.view == View::BattInfo || self.view == View::SysInfo)
            }
            KRX_EV_KEY => {
                if ev.payload == key::STATE_RELEASED {
                    if ev.key == self.held_key {
                        self.held_key = 0xFF;
                    }
                    return 0;
                }
                if ev.payload != key::STATE_PRESSED {
                    return 0;
                }
                match ev.key {
                    key::UP | key::KEY2 => {
                        self.held_key = ev.key;
                        self.repeat_cd = REPEAT_DELAY_TICKS;
                        self.step_cursor(api, true, false);
                        1
                    }
                    key::DOWN | key::KEY8 => {
                        self.held_key = ev.key;
                        self.repeat_cd = REPEAT_DELAY_TICKS;
                        self.step_cursor(api, false, false);
                        1
                    }
                    key::MENU | key::KEY5 => {
                        self.on_confirm(api);
                        1
                    }
                    key::EXIT => u32::from(self.on_back(api)),
                    _ => 0,
                }
            }
            _ => 0,
        }
    }

    fn render_item_list(&self, api: &KrxApiTable, start: u8, count: u8, sel: u8, sy: i16, anim_tick: u16) {
        let mut raw = core::mem::MaybeUninit::<[u8; 16]>::uninit();
        let val_buf = unsafe { &mut *raw.as_mut_ptr() };
        let mut idx = 0u8;
        while idx < count {
            let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - sy;
            let item = item_at_slice(start, idx);
            let is_sel = idx == sel;
            let val = if item.kind == Kind::Action { 0 } else { api.prop_get(item.prop_id) };
            let vstr: &[u8] = if item.kind == Kind::Action {
                match item.meta {
                    ACT_DEV_MODEL => trimmed_slice(&self.sh.dev_model),
                    ACT_DEV_OWNER => trimmed_slice(&self.sh.dev_owner),
                    ACT_DEV_TAG => trimmed_slice(&self.sh.dev_tag),
                    ACT_DEV_RADIO_ID => format_u16_into(self.sh.dev_radio_id as u16, 0, b"", val_buf),
                    ACT_ASSET_SPLASH => cstr_slice(&self.sh.splash_name),
                    ACT_ASSET_ROGER => cstr_slice(&self.sh.roger_name),
                    ACT_ASSET_JINGLE => cstr_slice(&self.sh.jingle_name),
                    ACT_ASSET_LANG => cstr_slice(&self.sh.lang_name),
                    _ => b"",
                }
            } else {
                format_value(item, val, val_buf)
            };
            let mut flags = u8::from(is_sel);
            if self.editing && is_sel {
                flags |= ui::FLAG_OUTLINE;
            }
            match item.kind {
                Kind::Toggle => {
                    flags |= ui::FLAG_TOGGLE;
                    if val != 0 {
                        flags |= ui::FLAG_TOGGLE_ON;
                    }
                }
                Kind::Action if is_sel && item.meta <= ACT_BANDPLANS => {
                    flags |= ui::FLAG_CHEVRON;
                }
                _ => {}
            }
            ui::draw_row(api, y, item_label(item.label_idx), vstr, 0, flags, anim_tick);
            idx += 1;
        }
        ui::finish_list_frame(api, sy, count);
    }

    #[inline(never)]
    fn render(&mut self, api: &KrxApiTable) {
        api.clear();
        let sy = self.scroller.cur_y;
        let anim_tick = if api.prop_get(prop::UI_ANIMATIONS) != 0 {
            (self.tick_ctr as u16) | 1
        } else {
            0
        };
        self.sh.anim_tick = anim_tick;
        match self.view {
            View::Home => {
                let mut idx = 0u8;
                while idx < CAT_COUNT as u8 {
                    let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - sy;
                    let c = category(idx);
                    let flags = if idx == self.cat_sel { ui::FLAG_SEL | ui::FLAG_CHEVRON } else { 0 };
                    ui::draw_row(api, y, cat_name(idx), b"", c.icon_id, flags, anim_tick);
                    idx += 1;
                }
                ui::finish_list_frame(api, sy, CAT_COUNT as u8);
            }
            View::Category => {
                let c = category(self.cat_sel);
                self.render_item_list(api, c.start, c.count, self.item_sel, sy, anim_tick);
            }
            View::Submenu => {
                self.render_item_list(api, self.sub_start, self.sub_count, self.sub_sel, sy, anim_tick);
            }
            View::InstalledApps => self.call_leaf(api, LEAF_APPS_RENDER, 0, 0),
            View::AppDetail => self.call_leaf(api, LEAF_APP_HDR, 0, 0),
            View::Shortcuts => self.call_leaf(api, LEAF_SC_RENDER, CMD_SC_RENDER, 0),
            View::BattInfo => self.call_leaf(api, LEAF_INFO_RENDER, CMD_INFO_BATT, 0),
            View::SysInfo => self.call_leaf(api, LEAF_INFO_RENDER, CMD_INFO_SYS, 0),
            View::Bandplans => self.call_leaf(api, LEAF_BANDPLANS, CMD_BP_RENDER, 0),
        }
        api.commit();
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
            0
        }
        _ => 0,
    }
}

krx_entry!(app_entry);
