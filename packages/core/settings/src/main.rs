//! Kern OS Settings — External Native `.krx` Application.
//!
//! Modular, schema-driven system preferences with canonical category icons,
//! long-press repeat navigation, kinetic scroll animations, oscillating chevrons,
//! installed package manager, and live hardware telemetry.
#![no_std]
#![no_main]

mod apps;
mod schema;
mod system;
mod ui;

use kern_sdk::{
    KRX_EV_ENCODER, KRX_EV_KEY, KRX_EV_MODAL, KRX_EV_TICK, KRX_OP_EVENT, KRX_OP_EXIT,
    KRX_OP_RENDER, KRX_OP_START, KrxApiTable, KrxEvent, MODAL_RESULT_CONFIRMED, icon, key,
    krx_entry, prop,
};

use apps::{AppsView, MODAL_DELETE_APP};
use schema::{
    ACT_DEVICE_INFO, ACT_INSTALLED_APPS, ACT_REBOOT_RADIO, ACT_RESET_SETTINGS, CAT_COUNT, Kind,
    cat_name, category, format_value, item_at, item_label,
};
use system::{INFO_ROWS, MODAL_REBOOT_RADIO, MODAL_RESET_SETTINGS};
use ui::KineticScroll;

const REPEAT_DELAY_TICKS: u8 = 7;
const REPEAT_RATE_TICKS: u8 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum View {
    Home = 0,
    Category = 1,
    InstalledApps = 2,
    DeviceInfo = 3,
}

struct SettingsState {
    view: View,
    cat_sel: u8,
    item_sel: u8,
    info_sel: u8,
    editing: bool,
    dirty: bool,
    held_key: u8,
    repeat_cd: u8,
    tick_ctr: u8,
    scroller: KineticScroll,
    apps: AppsView,
}

impl SettingsState {
    const fn new() -> Self {
        Self {
            view: View::Home,
            cat_sel: 0,
            item_sel: 0,
            info_sel: 0,
            editing: false,
            dirty: false,
            held_key: 0xFF,
            repeat_cd: 0,
            tick_ctr: 0,
            scroller: KineticScroll::new(),
            apps: AppsView::new(),
        }
    }

    #[inline(always)]
    fn active_sel(&self) -> u8 {
        match self.view {
            View::Home => self.cat_sel,
            View::Category => self.item_sel,
            View::InstalledApps => self.apps.sel,
            View::DeviceInfo => self.info_sel,
        }
    }

    #[inline(never)]
    fn sync_status_bar(&self, api: &KrxApiTable) {
        let title: &[u8] = match self.view {
            View::Home => b"Settings",
            View::Category => cat_name(self.cat_sel),
            View::InstalledApps => b"Installed",
            View::DeviceInfo => b"Hardware Info",
        };
        ui::set_title(api, title);
    }

    #[inline(never)]
    fn flush_if_dirty(&mut self, api: &KrxApiTable) {
        if self.dirty {
            let _ = api.flush();
            self.dirty = false;
        }
    }

    #[inline(never)]
    fn on_start(&mut self, api: &KrxApiTable) {
        self.view = View::Home;
        self.cat_sel = 0;
        self.item_sel = 0;
        self.info_sel = 0;
        self.editing = false;
        self.dirty = false;
        self.held_key = 0xFF;
        self.repeat_cd = 0;
        self.tick_ctr = 0;
        self.scroller.jump_to_row(0);
        self.sync_status_bar(api);
    }

    #[inline(never)]
    fn step_cursor(&mut self, api: &KrxApiTable, up: bool, is_repeat: bool) {
        if self.view == View::Category && self.editing {
            let item = item_at(self.cat_sel, self.item_sel);
            if let Some((min, max, step)) = prop::lookup_bounds(item.prop_id) {
                let cur = api.prop_get(item.prop_id);
                let next = if up {
                    if cur >= max {
                        if is_repeat { max } else { min }
                    } else {
                        cur.saturating_add(step).min(max)
                    }
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
            View::InstalledApps => (&mut self.apps.sel, self.apps.count),
            View::DeviceInfo => (&mut self.info_sel, INFO_ROWS),
        };
        if count == 0 {
            return;
        }
        let prev = *sel;
        let wrapped = if up {
            if *sel > 0 {
                *sel -= 1;
                false
            } else {
                *sel = count - 1;
                true
            }
        } else if *sel + 1 < count {
            *sel += 1;
            false
        } else {
            *sel = 0;
            true
        };
        if *sel != prev {
            let anim = !wrapped && api.prop_get(prop::UI_ANIMATIONS) != 0;
            let top = ui::scroll_top(*sel);
            self.scroller.set_target_row(top, anim);
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
            View::Category => {
                let item = item_at(self.cat_sel, self.item_sel);
                match item.kind {
                    Kind::Toggle => {
                        let cur = api.prop_get(item.prop_id);
                        if api.pref_set(item.prop_id, u8::from(cur == 0)) {
                            self.dirty = true;
                        }
                    }
                    Kind::Num | Kind::Choice => {
                        self.editing = !self.editing;
                        if !self.editing {
                            self.flush_if_dirty(api);
                        }
                    }
                    Kind::Action => match item.meta {
                        ACT_INSTALLED_APPS => {
                            self.apps.refresh(api);
                            self.view = View::InstalledApps;
                            self.scroller.jump_to_row(ui::scroll_top(self.apps.sel));
                            self.sync_status_bar(api);
                        }
                        ACT_DEVICE_INFO => {
                            self.info_sel = 0;
                            self.view = View::DeviceInfo;
                            self.scroller.jump_to_row(0);
                            self.sync_status_bar(api);
                        }
                        ACT_RESET_SETTINGS => system::request_reset(api),
                        ACT_REBOOT_RADIO => system::request_reboot(api),
                        _ => {}
                    },
                }
            }
            View::InstalledApps => self.apps.request_delete(api),
            View::DeviceInfo => {}
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
            View::InstalledApps | View::DeviceInfo => {
                self.view = View::Category;
                self.scroller.jump_to_row(ui::scroll_top(self.item_sel));
                self.sync_status_bar(api);
                true
            }
        }
    }

    #[inline(never)]
    fn on_event(&mut self, api: &KrxApiTable, ev: &KrxEvent) -> u32 {
        match ev.kind {
            KRX_EV_MODAL => {
                self.held_key = 0xFF;
                if ev.flags == MODAL_RESULT_CONFIRMED {
                    match ev.key {
                        MODAL_DELETE_APP => self.apps.confirm_delete(api),
                        MODAL_RESET_SETTINGS => system::apply_reset_defaults(api),
                        MODAL_REBOOT_RADIO => {
                            let _ = api.flush();
                            api.reboot(0);
                        }
                        _ => {}
                    }
                }
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
                // Keep oscillating chevron & live Device Info telemetry smoothly animated
                let anim_chevron = api.prop_get(prop::UI_ANIMATIONS) != 0
                    && (self.view == View::Home || self.view == View::Category);
                u32::from(dirty_ui || anim_chevron || self.view == View::DeviceInfo)
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

    #[inline(never)]
    fn render(&self, api: &KrxApiTable) {
        api.clear();
        let sy = self.scroller.cur_y;
        let tick = self.tick_ctr;
        match self.view {
            View::Home => {
                let mut idx = 0u8;
                while idx < CAT_COUNT as u8 {
                    let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - sy;
                    let c = category(idx);
                    let is_sel = idx == self.cat_sel;
                    ui::draw_scrolled_row(
                        api,
                        y,
                        cat_name(idx),
                        b"",
                        c.icon_id,
                        is_sel,
                        false,
                        if is_sel { icon::ARROW_RIGHT } else { 0 },
                        tick,
                    );
                    idx += 1;
                }
                ui::finish_list_frame(api, sy, CAT_COUNT as u8);
            }
            View::Category => {
                let c = category(self.cat_sel);
                let mut val_buf = [0u8; 16];
                let mut idx = 0u8;
                while idx < c.count {
                    let y = ui::ROW_START_Y + (idx as i16) * ui::ROW_STEP_Y - sy;
                    let item = item_at(self.cat_sel, idx);
                    let is_sel = idx == self.item_sel;
                    let val = if item.kind == Kind::Action {
                        0
                    } else {
                        api.prop_get(item.prop_id)
                    };
                    let vstr = format_value(item, val, &mut val_buf);
                    let right_ic = match item.kind {
                        Kind::Toggle => {
                            if val != 0 {
                                icon::CHECKBOX_ON
                            } else {
                                icon::CHECKBOX_OFF
                            }
                        }
                        Kind::Action
                            if item.meta == ACT_INSTALLED_APPS || item.meta == ACT_DEVICE_INFO =>
                        {
                            if is_sel {
                                icon::ARROW_RIGHT
                            } else {
                                0
                            }
                        }
                        _ => 0,
                    };
                    ui::draw_scrolled_row(
                        api,
                        y,
                        item_label(item.label_idx),
                        vstr,
                        0,
                        is_sel,
                        self.editing && is_sel,
                        right_ic,
                        tick,
                    );
                    idx += 1;
                }
                ui::finish_list_frame(api, sy, c.count);
            }
            View::InstalledApps => self.apps.render(api, sy, tick),
            View::DeviceInfo => system::render_device_info(api, self.info_sel, sy, tick),
        }
        let _ = self.active_sel();
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
