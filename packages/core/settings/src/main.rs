//! Kern OS Settings — Single-Crate `P1 v2` Native `.krx` Application.
//!
//! Consolidated from 33 legacy `P0` leaf crates into one clean `#![no_std]` Rust crate
//! compiled under `profile = "p1"` (`512 B` streamed pages via `ExtentStreamer`).
#![no_std]
#![no_main]

mod schema;
mod ui;

use kern_sdk::{
    KRX_EV_ENCODER, KRX_EV_KEY, KRX_EV_MODAL, KRX_EV_TICK, KRX_OP_EVENT, KRX_OP_EXIT,
    KRX_OP_RENDER, KRX_OP_START, KrxApiTable, KrxEvent, MODAL_RESULT_CONFIRMED,
    MODAL_RESULT_SHEET_SELECTED, Op, key, krx_entry, nth_packed_slice, prop,
};
use schema::{
    ACT_REBOOT_RADIO, ACT_RESET_SETTINGS, ACT_SUB_BATT_TWEAKS, ACT_SUB_BTN_TWEAKS,
    ACT_SUB_SPLASH, ACT_SYS_CALIB, ACT_SYS_DFU, ACT_SYS_INFO, ACT_SYS_STORAGE, CAT_COUNT,
    CHOICE_SRCS, ITEM_LABELS, Kind, SUB_BATT_TWEAKS_COUNT, SUB_BATT_TWEAKS_START,
    SUB_BTN_TWEAKS_COUNT, SUB_BTN_TWEAKS_START, SUB_SPLASH_COUNT, SUB_SPLASH_START, cat_name,
    category, item_at_slice, prop_bounds,
};
use ui::{
    FLAG_ARROW_LEFT, FLAG_ARROW_RIGHT, FLAG_CHEVRON, FLAG_TOGGLE, FLAG_TOGGLE_ON, KineticScroll,
    ROW_START_Y, ROW_STEP_Y, draw_row, finish_list_frame, set_title,
};

const MODAL_CHOICE: u8 = 1;
const MODAL_RESET: u8 = 2;
const MODAL_REBOOT: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum View {
    Home = 0,
    Category = 1,
    Submenu = 2,
}

struct SettingsApp {
    view: View,
    cat_idx: u8,
    sub_start: u8,
    sub_count: u8,
    sel: u8,
    saved_home_sel: u8,
    saved_home_top: u8,
    saved_cat_sel: u8,
    saved_cat_top: u8,
    editing: bool,
    choice_prop: u8,
    anim_tick: u16,
    scroll: KineticScroll,
    scratch: [u8; 128],
}

static mut APP: SettingsApp = SettingsApp {
    view: View::Home,
    cat_idx: 0,
    sub_start: 0,
    sub_count: 0,
    sel: 0,
    saved_home_sel: 0,
    saved_home_top: 0,
    saved_cat_sel: 0,
    saved_cat_top: 0,
    editing: false,
    choice_prop: 0,
    anim_tick: 0,
    scroll: KineticScroll::new(),
    scratch: [0; 128],
};

fn count_packed(packed: &[u8]) -> u8 {
    let mut n = 0u8;
    let mut i = 0usize;
    while i < packed.len() {
        if packed[i] == 0 {
            n = n.saturating_add(1);
        }
        i += 1;
    }
    n
}

fn fmt_u8(mut v: u8, dst: &mut [u8; 16]) -> usize {
    if v == 0 {
        dst[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 4];
    let mut n = 0usize;
    while v > 0 {
        tmp[n] = b'0' + (v % 10);
        v /= 10;
        n += 1;
    }
    let mut i = 0usize;
    while i < n {
        dst[i] = tmp[n - 1 - i];
        i += 1;
    }
    n
}

impl SettingsApp {
    fn cur_count(&self) -> u8 {
        match self.view {
            View::Home => CAT_COUNT as u8,
            View::Category => category(self.cat_idx).count,
            View::Submenu => self.sub_count,
        }
    }

    fn cur_item_slice(&self) -> (u8, u8) {
        match self.view {
            View::Category => {
                let c = category(self.cat_idx);
                (c.start, c.count)
            }
            View::Submenu => (self.sub_start, self.sub_count),
            View::Home => (0, 0),
        }
    }

    fn update_title(&mut self, api: &KrxApiTable) {
        let title: &[u8] = match self.view {
            View::Home => b"Settings",
            View::Category => cat_name(self.cat_idx),
            View::Submenu => b"Options",
        };
        set_title(api, &mut self.scratch, title);
    }

    fn move_cursor(&mut self, api: &KrxApiTable, dir: i8) {
        let count = self.cur_count();
        if count == 0 {
            return;
        }
        let anim = api.prop_get(prop::UI_ANIMATIONS) != 0;
        if dir < 0 {
            self.sel = if self.sel == 0 { count - 1 } else { self.sel - 1 };
        } else if dir > 0 {
            self.sel = if self.sel + 1 >= count { 0 } else { self.sel + 1 };
        }
        self.scroll.adjust(self.sel, count, anim);
    }

    fn adjust_num(&mut self, api: &KrxApiTable, dir: i8) {
        let (start, _) = self.cur_item_slice();
        let item = item_at_slice(start, self.sel);
        let (lo, hi, step) = prop_bounds(item.prop_id).unwrap_or((0, 100, 1));
        let cur = api.prop_get(item.prop_id) as i16;
        let next = (cur + (dir as i16) * (step.max(1) as i16)).clamp(lo as i16, hi as i16) as u8;
        if next != cur as u8 {
            api.pref_set(item.prop_id, next);
        }
    }

    fn activate(&mut self, api: &KrxApiTable) {
        match self.view {
            View::Home => {
                self.saved_home_sel = self.sel;
                self.saved_home_top = self.scroll.top_row;
                self.cat_idx = self.sel;
                self.view = View::Category;
                self.sel = 0;
                self.editing = false;
                self.scroll.jump(0, self.cur_count(), 0);
                self.update_title(api);
            }
            View::Category | View::Submenu => {
                let (start, _) = self.cur_item_slice();
                let item = item_at_slice(start, self.sel);
                match item.kind {
                    Kind::Toggle => {
                        let cur = api.prop_get(item.prop_id);
                        api.pref_set(item.prop_id, if cur == 0 { 1 } else { 0 });
                    }
                    Kind::Num => {
                        self.editing = !self.editing;
                    }
                    Kind::Choice => {
                        let tbl = CHOICE_SRCS[(item.meta as usize).min(CHOICE_SRCS.len() - 1)];
                        let cnt = count_packed(tbl);
                        let cur = api.prop_get(item.prop_id).min(cnt.saturating_sub(1));
                        self.choice_prop = item.prop_id;
                        let title = ITEM_LABELS[(item.label_idx as usize).min(ITEM_LABELS.len() - 1)];
                        api.show_sheet(MODAL_CHOICE, title, tbl, cur, true);
                    }
                    Kind::Action => match item.meta {
                        ACT_SUB_SPLASH => self.enter_sub(api, SUB_SPLASH_START, SUB_SPLASH_COUNT),
                        ACT_SUB_BTN_TWEAKS => {
                            self.enter_sub(api, SUB_BTN_TWEAKS_START, SUB_BTN_TWEAKS_COUNT)
                        }
                        ACT_SUB_BATT_TWEAKS => {
                            self.enter_sub(api, SUB_BATT_TWEAKS_START, SUB_BATT_TWEAKS_COUNT)
                        }
                        ACT_RESET_SETTINGS => {
                            api.show_confirm(
                                MODAL_RESET,
                                b"Reset Settings",
                                b"Restore all defaults?",
                                b"Reset",
                                b"Cancel",
                                1000,
                            );
                        }
                        ACT_REBOOT_RADIO => {
                            api.show_confirm(
                                MODAL_REBOOT,
                                b"Reboot Radio",
                                b"Restart device now?",
                                b"Reboot",
                                b"Cancel",
                                0,
                            );
                        }
                        ACT_SYS_INFO => {
                            let _ = api.launch_pkg(b"/apps/sysinfo.krx");
                        }
                        ACT_SYS_STORAGE => {
                            let _ = api.launch_pkg(b"/apps/storage.mod");
                        }
                        ACT_SYS_CALIB => {
                            let _ = api.launch_pkg(b"/apps/calib.mod");
                        }
                        ACT_SYS_DFU => {
                            let _ = api.launch_pkg(b"/apps/dfu.mod");
                        }
                        _ => {}
                    },
                }
            }
        }
    }

    fn enter_sub(&mut self, api: &KrxApiTable, start: u8, count: u8) {
        self.saved_cat_sel = self.sel;
        self.saved_cat_top = self.scroll.top_row;
        self.sub_start = start;
        self.sub_count = count;
        self.view = View::Submenu;
        self.sel = 0;
        self.editing = false;
        self.scroll.jump(0, count, 0);
        self.update_title(api);
    }

    fn go_back(&mut self, api: &KrxApiTable) -> bool {
        if self.editing {
            self.editing = false;
            return true;
        }
        match self.view {
            View::Submenu => {
                self.view = View::Category;
                self.sel = self.saved_cat_sel;
                self.scroll.jump(self.sel, self.cur_count(), self.saved_cat_top);
                self.update_title(api);
                true
            }
            View::Category => {
                self.view = View::Home;
                self.sel = self.saved_home_sel;
                self.scroll.jump(self.sel, self.cur_count(), self.saved_home_top);
                self.update_title(api);
                true
            }
            View::Home => false,
        }
    }

    fn on_event(&mut self, api: &KrxApiTable, ev: &KrxEvent) -> u32 {
        match ev.kind {
            KRX_EV_TICK => {
                self.anim_tick = self.anim_tick.wrapping_add(1);
                if self.scroll.tick() { 1 } else { 0 }
            }
            KRX_EV_ENCODER => {
                let dir = if (ev.payload as i32) < 0 { -1 } else { 1 };
                if self.editing {
                    self.adjust_num(api, dir);
                } else {
                    self.move_cursor(api, dir);
                }
                1
            }
            KRX_EV_KEY => {
                if ev.flags != 0 && ev.flags != 2 {
                    return 0;
                }
                let k = ev.key;
                let nav = api.prop_get(prop::NAV_LAYOUT);
                if k == key::EXIT {
                    return if self.go_back(api) { 1 } else { 0 };
                }
                if k == key::MENU || k == key::KEY5 {
                    self.activate(api);
                    return 1;
                }
                let dir = if k == key::UP || k == key::KEY2 {
                    if self.editing && nav != 0 { 1 } else { -1 }
                } else if k == key::DOWN || k == key::KEY8 {
                    if self.editing && nav != 0 { -1 } else { 1 }
                } else if k == key::KEY4 {
                    -1
                } else if k == key::KEY6 {
                    1
                } else {
                    0
                };
                if dir != 0 {
                    if self.editing {
                        self.adjust_num(api, dir);
                    } else {
                        self.move_cursor(api, dir);
                    }
                    return 1;
                }
                0
            }
            KRX_EV_MODAL => {
                let tag = ev.key;
                let res = ev.flags;
                if tag == MODAL_CHOICE && res == MODAL_RESULT_SHEET_SELECTED {
                    api.pref_set(self.choice_prop, ev.payload as u8);
                } else if tag == MODAL_RESET && res == MODAL_RESULT_CONFIRMED {
                    let _ = api.call_op(Op::SysQuery, ::abi::sub::sys_query::RESET as usize, 0, 0);
                } else if tag == MODAL_REBOOT && res == MODAL_RESULT_CONFIRMED {
                    let _ = api.call_op(Op::SysCtl, ::abi::sub::sys_ctl::REBOOT as usize, 0, 0);
                }
                1
            }
            _ => 0,
        }
    }

    fn render(&mut self, api: &KrxApiTable) {
        api.clear();
        let count = self.cur_count();
        let top_y = ROW_START_Y - self.scroll.cur_y;
        let mut i = 0u8;
        while i < count {
            let y = top_y + (i as i16) * ROW_STEP_Y;
            let selected = i == self.sel;
            match self.view {
                View::Home => {
                    let c = category(i);
                    let flags = if selected { 1u8 | FLAG_CHEVRON } else { 0u8 };
                    let icon_flags = (c.icon_id as u16) | ((flags as u16) << 8);
                    draw_row(api, y, cat_name(i), b"", icon_flags, self.anim_tick);
                }
                View::Category | View::Submenu => {
                    let (start, _) = self.cur_item_slice();
                    let item = item_at_slice(start, i);
                    let lbl = ITEM_LABELS[(item.label_idx as usize).min(ITEM_LABELS.len() - 1)];
                    let mut flags = if selected { 1u8 } else { 0u8 };
                    let mut val_buf = [0u8; 16];
                    let val: &[u8] = match item.kind {
                        Kind::Toggle => {
                            flags |= FLAG_TOGGLE;
                            if api.prop_get(item.prop_id) != 0 {
                                flags |= FLAG_TOGGLE_ON;
                            }
                            b""
                        }
                        Kind::Num => {
                            if selected && self.editing {
                                flags |= FLAG_ARROW_LEFT | FLAG_ARROW_RIGHT;
                            }
                            let n = fmt_u8(api.prop_get(item.prop_id), &mut val_buf);
                            &val_buf[..n]
                        }
                        Kind::Choice => {
                            let tbl = CHOICE_SRCS[(item.meta as usize).min(CHOICE_SRCS.len() - 1)];
                            let v = api.prop_get(item.prop_id);
                            nth_packed_slice(tbl, v)
                        }
                        Kind::Action => {
                            if selected {
                                flags |= FLAG_CHEVRON;
                            }
                            b""
                        }
                    };
                    let icon_flags = (flags as u16) << 8;
                    draw_row(api, y, lbl, val, icon_flags, self.anim_tick);
                }
            }
            i += 1;
        }
        finish_list_frame(api, self.scroll.cur_y, count);
    }
}

krx_entry!(entry);

fn entry(op: u32, api: &KrxApiTable, param: *const ()) -> u32 {
    let app = unsafe { &mut *core::ptr::addr_of_mut!(APP) };
    match op {
        KRX_OP_START => {
            app.view = View::Home;
            app.sel = 0;
            app.editing = false;
            app.scroll.jump(0, CAT_COUNT as u8, 0);
            app.update_title(api);
            0
        }
        KRX_OP_EVENT => {
            if param.is_null() {
                return 0;
            }
            let ev = unsafe { &*(param as *const KrxEvent) };
            app.on_event(api, ev)
        }
        KRX_OP_RENDER => {
            app.render(api);
            0
        }
        KRX_OP_EXIT => {
            let _ = api.call_op(Op::SysQuery, ::abi::sub::sys_query::FLUSH as usize, 0, 0);
            0
        }
        _ => 0,
    }
}
