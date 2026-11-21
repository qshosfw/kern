//! Kern OS Settings — External Native `.krx` Application.
//!
//! Modular, schema-driven system preferences, installed package manager, and live
//! hardware telemetry running cleanly inside the relocatable KRX execution arena.
#![no_std]
#![no_main]

mod apps;
mod schema;
mod system;
mod ui;

use kern_sdk::{
    KRX_EV_ENCODER, KRX_EV_KEY, KRX_EV_MODAL, KRX_EV_TICK, KRX_OP_EVENT, KRX_OP_EXIT,
    KRX_OP_RENDER, KRX_OP_START, KrxApiTable, KrxEvent, MODAL_RESULT_CONFIRMED, key, krx_entry,
    prop,
};

use apps::{AppsView, MODAL_DELETE_APP};
use schema::{
    ACT_DEVICE_INFO, ACT_INSTALLED_APPS, ACT_REBOOT_RADIO, ACT_RESET_SETTINGS, CAT_COUNT, Kind,
    cat_name, category, format_value, item_at, item_label,
};
use system::{INFO_ROWS, MODAL_REBOOT_RADIO, MODAL_RESET_SETTINGS};

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
            apps: AppsView::new(),
        }
    }

    #[inline(never)]
    fn sync_status_bar(&self, api: &KrxApiTable) {
        let title: &[u8] = match self.view {
            View::Home => b"Settings",
            View::Category => cat_name(self.cat_sel),
            View::InstalledApps => b"Installed",
            View::DeviceInfo => b"Device Info",
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
        self.sync_status_bar(api);
    }

    #[inline(never)]
    fn step_cursor(&mut self, api: &KrxApiTable, up: bool) {
        if self.view == View::Category && self.editing {
            let item = item_at(self.cat_sel, self.item_sel);
            if let Some((min, max, step)) = prop::lookup_bounds(item.prop_id) {
                let cur = api.prop_get(item.prop_id);
                let next = if up {
                    if cur >= max { min } else { cur.saturating_add(step).min(max) }
                } else if cur <= min {
                    max
                } else {
                    cur.saturating_sub(step).max(min)
                };
                if next != cur && api.pref_set(item.prop_id, next) {
                    self.dirty = true;
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
        *sel = if up {
            if *sel > 0 { *sel - 1 } else { count - 1 }
        } else if *sel + 1 < count {
            *sel + 1
        } else {
            0
        };
    }

    #[inline(never)]
    fn on_confirm(&mut self, api: &KrxApiTable) {
        match self.view {
            View::Home => {
                self.view = View::Category;
                self.item_sel = 0;
                self.editing = false;
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
                            self.sync_status_bar(api);
                        }
                        ACT_DEVICE_INFO => {
                            self.info_sel = 0;
                            self.view = View::DeviceInfo;
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
                self.sync_status_bar(api);
                true
            }
            View::InstalledApps | View::DeviceInfo => {
                self.view = View::Category;
                self.sync_status_bar(api);
                true
            }
        }
    }

    #[inline(never)]
    fn on_event(&mut self, api: &KrxApiTable, ev: &KrxEvent) -> u32 {
        match ev.kind {
            KRX_EV_MODAL => {
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
                    self.step_cursor(api, delta < 0);
                    return 1;
                }
                0
            }
            KRX_EV_TICK => u32::from(self.view == View::DeviceInfo),
            KRX_EV_KEY if ev.payload == key::STATE_PRESSED => match ev.key {
                key::UP | key::KEY2 => {
                    self.step_cursor(api, true);
                    1
                }
                key::DOWN | key::KEY8 => {
                    self.step_cursor(api, false);
                    1
                }
                key::MENU | key::KEY5 => {
                    self.on_confirm(api);
                    1
                }
                key::EXIT => u32::from(self.on_back(api)),
                _ => 0,
            },
            _ => 0,
        }
    }

    #[inline(never)]
    fn render(&self, api: &KrxApiTable) {
        api.clear();
        match self.view {
            View::Home => {
                let top = ui::scroll_top(self.cat_sel);
                let mut vis = 0u8;
                while vis < ui::VISIBLE_ROWS {
                    let idx = top + vis;
                    if idx >= CAT_COUNT as u8 {
                        break;
                    }
                    let c = category(idx);
                    ui::draw_row(
                        api,
                        vis,
                        cat_name(idx),
                        b">",
                        c.icon_id,
                        idx == self.cat_sel,
                        false,
                    );
                    vis += 1;
                }
                ui::draw_list_scrollbar(api, top, CAT_COUNT as u8);
            }
            View::Category => {
                let c = category(self.cat_sel);
                let top = ui::scroll_top(self.item_sel);
                let mut val_buf = [0u8; 16];
                let mut vis = 0u8;
                while vis < ui::VISIBLE_ROWS {
                    let idx = top + vis;
                    if idx >= c.count {
                        break;
                    }
                    let item = item_at(self.cat_sel, idx);
                    let val = if item.kind == Kind::Action {
                        0
                    } else {
                        api.prop_get(item.prop_id)
                    };
                    let vstr = format_value(item, val, &mut val_buf);
                    ui::draw_row(
                        api,
                        vis,
                        item_label(item.label_idx),
                        vstr,
                        0,
                        idx == self.item_sel,
                        self.editing && idx == self.item_sel,
                    );
                    vis += 1;
                }
                ui::draw_list_scrollbar(api, top, c.count);
            }
            View::InstalledApps => self.apps.render(api),
            View::DeviceInfo => system::render_device_info(api, self.info_sel),
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
