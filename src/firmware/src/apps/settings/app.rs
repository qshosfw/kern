use crate::SysContext;
use gfx::Canvas;
#[cfg(feature = "passcode")]
use gfx::gui::modules::menu::ShortString;
use gfx::gui::modules::menu::{
    MenuItem, MenuItemType, MenuModel, MenuModelMut, MenuView, OptionSheet,
};
use gfx::gui::view::View;
use kernel::app::{App, AppMetadata, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};

use super::options::{
    ACTION_CHOOSE_MODEL, ACTION_CLEAR_LOGS, PAGE_APP_DETAIL, PAGE_AUDIO, PAGE_BATTERY_INFO,
    PAGE_MANAGE_APPS, PAGE_MANAGE_SERVICES, PAGE_ROOT, PAGE_SERVICE_DETAIL, PAGE_SYSTEM,
    SettingKind, apply_page_changes, find_page, populate_defs,
};
#[cfg(feature = "passcode")]
use super::options::{ACTION_EDIT_OWNER, ACTION_EDIT_RADIO_ID, ACTION_EDIT_TAG, PAGE_SECURITY};
#[cfg(feature = "boot_assets")]
use super::options::{ACTION_JINGLE_CHOOSER, ACTION_ROGER_CHOOSER, ACTION_SPLASH_CHOOSER};

#[cfg(feature = "passcode")]
const TAG_PASSCODE_VERIFY: u8 = 2;
#[cfg(feature = "passcode")]
pub const TAG_PASSCODE_NEW: u8 = 3;
#[cfg(feature = "passcode")]
const TAG_PASSCODE_CONFIRM: u8 = 4;
const TAG_CHOICE_SHEET: u8 = 6;
pub const TAG_POPUP_INFO: u8 = 7;
#[cfg(feature = "passcode")]
const TAG_OWNER_INPUT: u8 = 8;
#[cfg(feature = "passcode")]
const TAG_TAG_INPUT: u8 = 9;
#[cfg(feature = "passcode")]
const TAG_RADIO_ID_INPUT: u8 = 10;
#[cfg(feature = "boot_assets")]
const TAG_SPLASH_CHOOSER: u8 = 11;
const TAG_MODEL_CHOOSER: u8 = 12;
#[cfg(feature = "passcode")]
const TAG_MODEL_INPUT: u8 = 13;
const TAG_APP_DELETE_CONFIRM: u8 = 14;
const TAG_APP_RESET_CONFIRM: u8 = 15;
#[cfg(feature = "boot_assets")]
const TAG_ROGER_CHOOSER: u8 = 16;
#[cfg(feature = "boot_assets")]
const TAG_JINGLE_CHOOSER: u8 = 17;

#[derive(Clone, Copy)]
struct MenuFrame {
    page_id: u8,
    selected_idx: usize,
    title: &'static str,
}

pub struct SettingsApp {
    items: [MenuItem; 16],
    items_len: usize,

    current_page_id: u8,
    stack: [MenuFrame; 4],
    stack_top: usize,

    title: &'static str,
    selected_idx: usize,
    is_editing: bool,
    is_pressed: bool,
    dirty_timer_ms: u16,
    last_tick_ms: u32,

    view: MenuView,
    #[cfg(feature = "passcode")]
    passcode_candidate: heapless::String<16>,

    pub selected_app_idx: usize,
    pub selected_service_idx: usize,
    pub slide_offset_x: i16,
}

#[cfg(feature = "passcode")]
fn parse_u32_digits(s: &str) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut val = 0u32;
    for &b in s.as_bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        val = val.checked_mul(10)?.checked_add((b - b'0') as u32)?;
    }
    Some(val)
}

#[cfg(feature = "passcode")]
fn prompt_config_str(ctx: &mut SysContext, tag: u8) {
    let mut buf = ShortString::empty();
    let (title, max_len, digits) = match tag {
        TAG_OWNER_INPUT => {
            buf.set_str(ctx.config.system.owner_str());
            ("Owner", 12, false)
        }
        TAG_TAG_INPUT => {
            buf.set_str(ctx.config.system.tag_str());
            ("Tag", 8, false)
        }
        TAG_MODEL_INPUT => {
            buf.set_str(ctx.config.system.custom_model_str());
            ("Model", 12, false)
        }
        _ => {
            buf.push_int_fixed(ctx.config.system.radio_id as i32, 0, "", "");
            ("Radio ID", 8, true)
        }
    };
    ctx.prompt_text(tag, title, buf.as_str(), max_len, digits);
}

impl SettingsApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).title = "Settings";
            (*ptr).view = MenuView::new();
            (*ptr).selected_app_idx = 0;
            (*ptr).selected_service_idx = 0;
            (*ptr).slide_offset_x = 0;
            (*ptr).dirty_timer_ms = 0;
            (*ptr).last_tick_ms = 0;
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.slide_offset_x != 0 || self.view.scroller.is_animating()
    }

    fn reset_input_state(&mut self) {
        self.is_pressed = false;
        self.is_editing = false;
    }

    #[inline(never)]
    fn populate_current_menu(&mut self, ctx: &mut SysContext) {
        #[cfg(feature = "passcode")]
        if self.current_page_id == PAGE_SECURITY {
            self.items_len = super::security::populate_security(&mut self.items, ctx);
            return;
        }
        if self.current_page_id == PAGE_MANAGE_APPS {
            self.items_len = super::apps::populate_manage_apps(&mut self.items, ctx);
            return;
        }
        if self.current_page_id == PAGE_APP_DETAIL {
            self.items_len =
                super::apps::populate_app_detail(&mut self.items, self.selected_app_idx, ctx);
            return;
        }
        if self.current_page_id == PAGE_MANAGE_SERVICES {
            self.items_len = super::apps::populate_manage_services(&mut self.items, ctx);
            return;
        }
        if self.current_page_id == PAGE_SERVICE_DETAIL {
            self.items_len = super::apps::populate_service_detail(
                &mut self.items,
                self.selected_service_idx,
                ctx,
            );
            return;
        }

        if let Some(page) = find_page(self.current_page_id) {
            populate_defs(&mut self.items, page.defs, ctx);
            self.items_len = page.defs.len().min(self.items.len());
        } else {
            self.items_len = 0;
        }
    }

    #[inline(never)]
    fn push_submenu(&mut self, page_id: u8, ctx: &mut SysContext) {
        if self.stack_top < self.stack.len() {
            self.stack[self.stack_top] = MenuFrame {
                page_id: self.current_page_id,
                selected_idx: self.selected_idx,
                title: self.title,
            };
            self.stack_top += 1;
        }
        self.current_page_id = page_id;
        self.selected_idx = 0;
        self.title = find_page(page_id).map(|p| p.title).unwrap_or("Settings");
        self.populate_current_menu(ctx);
        self.view.reset_scroll();
        if ctx.config.display.animations_enabled() {
            self.slide_offset_x = 28;
        } else {
            self.slide_offset_x = 0;
        }
        self.reset_input_state();
        ctx.sink_enter_keys();
    }

    #[inline(never)]
    fn pop_submenu(&mut self, ctx: &mut SysContext) -> bool {
        if self.stack_top > 0 {
            self.stack_top -= 1;
            let frame = self.stack[self.stack_top];
            self.current_page_id = frame.page_id;
            self.selected_idx = frame.selected_idx;
            self.title = frame.title;
            self.populate_current_menu(ctx);
            if self.selected_idx >= self.items_len {
                self.selected_idx = self.items_len.saturating_sub(1);
            }
            self.view.adjust_scroll(self.selected_idx, self.items_len);
            self.view.scroller.jump((self.view.scroll_top as i16) * 13);
            if ctx.config.display.animations_enabled() {
                self.slide_offset_x = -28;
            } else {
                self.slide_offset_x = 0;
            }
            self.reset_input_state();
            ctx.sink_exit_key();

            if ctx.config.is_dirty() {
                let _ = ctx.flush_config();
            }
            true
        } else {
            false
        }
    }

    #[inline(never)]
    fn update_config_from_menu(&mut self, ctx: &mut SysContext) {
        #[cfg(feature = "passcode")]
        if self.current_page_id == PAGE_SECURITY {
            let idx = self.selected_idx;
            if idx < self.items_len {
                if let MenuItemType::Toggle(b) = self.items[idx].item_type {
                    super::security::update_security_toggle(ctx, idx, b);
                }
            }
            return;
        }

        if let Some(page) = find_page(self.current_page_id) {
            let idx = self.selected_idx;
            if idx < self.items_len && idx < page.defs.len() {
                let def = &page.defs[idx];
                let val = match self.items[idx].item_type {
                    MenuItemType::Range { val, .. } => Some(val as i32),
                    MenuItemType::Toggle(b) => Some(b as i32),
                    _ => None,
                };
                if let Some(v) = val {
                    if def.field.set_val(v, &mut ctx.config) {
                        apply_page_changes(self.current_page_id, ctx);
                        self.dirty_timer_ms = 250;
                    }
                }
            }
        }
    }

    fn active_items(&self) -> &[MenuItem] {
        &self.items[..self.items_len]
    }

    #[allow(dead_code)]
    fn selected_item(&self) -> Option<&MenuItem> {
        if self.selected_idx < self.items_len {
            Some(&self.items[self.selected_idx])
        } else {
            None
        }
    }

    pub const ROOT_ICONS: &'static [&'static [u8; 8]] = &[
        &super::assets::ICON_DISPLAY,
        &super::assets::ICON_AUDIO,
        &super::assets::ICON_BUTTONS,
        &super::assets::ICON_BATTERY,
        &super::assets::ICON_DEVICE,
        &super::assets::ICON_APPS,
        #[cfg(feature = "passcode")]
        &super::assets::ICON_SECURITY,
        &super::assets::ICON_SYSTEM,
    ];

    #[inline(never)]
    fn build_menu_model<'a>(&'a self, is_editing: bool) -> MenuModel<'a> {
        let icons = if self.current_page_id == PAGE_ROOT {
            Some(Self::ROOT_ICONS)
        } else {
            None
        };
        MenuModel {
            title: self.title,
            items: self.active_items(),
            selected_idx: self.selected_idx,
            icons,
            is_editing,
            is_pressed: if is_editing { self.is_pressed } else { false },
        }
    }
}

#[cfg(feature = "passcode")]
#[inline(never)]
fn handle_security_press(ctx: &mut SysContext) {
    let is_set = crate::security::passcode::is_passcode_set(ctx.storage);
    let pcfg = crate::security::passcode::get_passcode_config(ctx.storage);
    if is_set {
        let max_len = if pcfg.passcode_len > 0 {
            pcfg.passcode_len as usize
        } else {
            16
        };
        let expose_len = pcfg.passcode_len > 0;
        ctx.prompt_passcode(
            TAG_PASSCODE_VERIFY,
            "Verify Passcode",
            max_len,
            pcfg.digits_only,
            expose_len,
        );
    } else {
        ctx.prompt_passcode(
            TAG_PASSCODE_NEW,
            "Set Passcode",
            16,
            pcfg.digits_only,
            false,
        );
    }
}

#[inline(never)]
fn handle_action_press(ctx: &mut SysContext, action_id: u8) {
    match action_id {
        ACTION_CHOOSE_MODEL => {
            let sel =
                (ctx.config.system.model as usize).min(super::options::MODEL_OPTIONS.len() - 1);
            ctx.show_sheet(
                TAG_MODEL_CHOOSER,
                OptionSheet::new("Model", &super::options::MODEL_OPTIONS, sel).with_radio(true),
            );
        }
        #[cfg(feature = "passcode")]
        ACTION_EDIT_OWNER => prompt_config_str(ctx, TAG_OWNER_INPUT),
        #[cfg(feature = "passcode")]
        ACTION_EDIT_TAG => prompt_config_str(ctx, TAG_TAG_INPUT),
        #[cfg(feature = "passcode")]
        ACTION_EDIT_RADIO_ID => prompt_config_str(ctx, TAG_RADIO_ID_INPUT),
        #[cfg(feature = "boot_assets")]
        ACTION_SPLASH_CHOOSER => {
            let sheet = super::device::DeviceDomain::create_splash_chooser(ctx);
            ctx.show_sheet(TAG_SPLASH_CHOOSER, sheet);
        }
        #[cfg(feature = "boot_assets")]
        ACTION_ROGER_CHOOSER => {
            let sheet = super::audio::AudioDomain::create_roger_chooser(ctx);
            ctx.show_sheet(TAG_ROGER_CHOOSER, sheet);
        }
        #[cfg(feature = "boot_assets")]
        ACTION_JINGLE_CHOOSER => {
            let sheet = super::audio::AudioDomain::create_jingle_chooser(ctx);
            ctx.show_sheet(TAG_JINGLE_CHOOSER, sheet);
        }
        ACTION_CLEAR_LOGS => {
            #[cfg(feature = "flight_recorder")]
            crate::panic::BOOT_RECORD.get_mut().reset_all();
            ctx.show_popup(TAG_POPUP_INFO, "Debug", "Crash logs cleared");
        }
        _ => {}
    }
}

impl SettingsApp {
    #[inline(never)]
    fn handle_menu_press(&mut self, ctx: &mut SysContext) {
        let sel_idx = self.selected_idx;
        if sel_idx >= self.items_len {
            return;
        }
        self.is_pressed = false;

        #[cfg(feature = "passcode")]
        if self.current_page_id == PAGE_SECURITY {
            if sel_idx == 0 {
                handle_security_press(ctx);
                return;
            }
        }

        if self.current_page_id == PAGE_MANAGE_APPS {
            let cat = crate::apps::launcher::catalog::get_catalog();
            let count = cat.app_count();
            if count > 0 && sel_idx < count {
                self.selected_app_idx = sel_idx;
                let is_valid = cat.apps().nth(sel_idx).map_or(false, |a| a.is_valid);
                if !is_valid {
                    ctx.show_confirm(
                        TAG_APP_DELETE_CONFIRM,
                        "Invalid App",
                        "Invalid or corrupted package.\nDelete from /apps/?",
                        "Delete",
                        "Cancel",
                    );
                } else {
                    self.push_submenu(PAGE_APP_DETAIL, ctx);
                }
            } else {
                ctx.show_popup(
                    TAG_POPUP_INFO,
                    "Apps Storage",
                    "Stage .krx files into /apps/",
                );
            }
            return;
        }

        if self.current_page_id == PAGE_MANAGE_SERVICES {
            let count = crate::apps::launcher::catalog::get_catalog().service_count();
            if count > 0 && sel_idx < count {
                self.selected_service_idx = sel_idx;
                self.push_submenu(PAGE_SERVICE_DETAIL, ctx);
            } else {
                ctx.show_popup(
                    TAG_POPUP_INFO,
                    "KRX Services",
                    "No background services in /apps/",
                );
            }
            return;
        }

        if self.current_page_id == PAGE_APP_DETAIL {
            let cat = crate::apps::launcher::catalog::get_catalog();
            if let Some(app) = cat.apps().nth(self.selected_app_idx) {
                if !app.is_valid {
                    if sel_idx == 0 {
                        ctx.show_confirm(
                            TAG_APP_DELETE_CONFIRM,
                            "Delete Invalid",
                            "Delete invalid package from /apps/?",
                            "Delete",
                            "Cancel",
                        );
                    }
                    return;
                }
                if sel_idx == 0 {
                    let path = app.path.as_str();
                    match crate::apps::runtime::launch_krx_package(ctx, path) {
                        Ok(_) => {}
                        Err(err) => {
                            ctx.show_confirm(TAG_POPUP_INFO, err.title(), err.message(), "OK", "EXIT");
                            ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
                        }
                    }
                    return;
                } else if sel_idx == 5 {
                    ctx.show_confirm(
                        TAG_APP_DELETE_CONFIRM,
                        "Delete App",
                        "Uninstall and delete application binary?",
                        "Delete",
                        "Cancel",
                    );
                }
            }
            return;
        }

        if self.current_page_id == PAGE_SERVICE_DETAIL {
            ctx.show_popup(
                TAG_POPUP_INFO,
                "Service Info",
                "KRX background package in /apps/",
            );
            return;
        }

        if let Some(page) = find_page(self.current_page_id) {
            if let Some(def) = page.defs.get(sel_idx) {
                match def.kind {
                    SettingKind::Submenu(target_page_id) => {
                        self.push_submenu(target_page_id, ctx);
                        return;
                    }
                    SettingKind::Action(action_id) => {
                        handle_action_press(ctx, action_id);
                        return;
                    }
                    _ => {}
                }
            }
        }

        let item = &self.items[sel_idx];
        if let MenuItemType::Choice { selected, options } = item.item_type {
            ctx.show_sheet(
                TAG_CHOICE_SHEET,
                OptionSheet::new(item.title, options, selected as usize).with_radio(true),
            );
        }
    }

    fn flush_and_refresh(&mut self, ctx: &mut SysContext) {
        let _ = ctx.flush_config();
        self.populate_current_menu(ctx);
    }

    #[cfg(feature = "passcode")]
    fn copy_padded(dst: &mut [u8], src: &[u8]) {
        dst.fill(b' ');
        let len = src.len().min(dst.len());
        dst[..len].copy_from_slice(&src[..len]);
    }

    #[inline(never)]
    fn handle_radio_confirmed(&mut self, ctx: &mut SysContext, sel: usize) {
        let sel_idx = self.selected_idx;
        if sel_idx < self.items_len {
            if self.current_page_id == PAGE_AUDIO {
                super::audio::AudioDomain::on_choice_confirmed(sel_idx, sel, ctx);
            }
            #[cfg(feature = "passcode")]
            if self.current_page_id == PAGE_SECURITY {
                super::security::update_security_option(ctx, sel_idx, sel);
                return;
            }
            if let Some(page) = find_page(self.current_page_id) {
                if let Some(def) = page.defs.get(sel_idx) {
                    let nav_layout = ctx.config.keys.nav_layout;
                    let val: i32 = if matches!(def.kind, SettingKind::BatteryType) {
                        let (_, types) = super::options::battery_options_and_types(nav_layout);
                        types.get(sel).copied().unwrap_or(types[0]) as i32
                    } else {
                        sel as i32
                    };
                    if def.field.set_val(val, &mut ctx.config) {
                        apply_page_changes(self.current_page_id, ctx);
                        self.dirty_timer_ms = 250;
                    }
                }
            }
        }
    }

    #[inline(never)]
    fn handle_menu_screen_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        self.view.animated = ctx.config.display.animations_enabled();
        let was_editing = self.is_editing;
        let handled = {
            let mut model = MenuModelMut {
                items: &mut self.items[..self.items_len],
                selected_idx: self.selected_idx,
                nav_layout: ctx.config.keys.nav_layout,
                is_editing: self.is_editing,
                is_pressed: self.is_pressed,
            };

            let prev_idx = self.selected_idx;
            let handled = self.view.on_event(&mut model, event);

            self.selected_idx = model.selected_idx;
            self.is_editing = model.is_editing;
            self.is_pressed = model.is_pressed;

            if handled
                && self.selected_idx != prev_idx
                && ctx.config.audio.key_beep != 0
                && matches!(event, Event::Tick)
            {
                ctx.play_beep(crate::services::melody::BeepType::Scroll);
            }

            handled
        };

        if let Event::Tick = event {
            if self.slide_offset_x != 0 {
                if !self.view.animated {
                    self.slide_offset_x = 0;
                } else {
                    const STEP: i16 = 2;
                    if self.slide_offset_x > 0 {
                        self.slide_offset_x = (self.slide_offset_x - STEP).max(0);
                    } else {
                        self.slide_offset_x = (self.slide_offset_x + STEP).min(0);
                    }
                }
            }
        }

        if handled || self.slide_offset_x != 0 {
            if was_editing
                || self.is_editing
                || matches!(
                    event,
                    Event::Key {
                        code: KeyCode::Menu | KeyCode::Key5,
                        state: KeyState::Pressed
                    }
                )
            {
                self.update_config_from_menu(ctx);
            }

            if !was_editing && !self.is_editing {
                if let Event::Key {
                    code,
                    state: KeyState::Pressed,
                } = *event
                {
                    if code == KeyCode::Menu || code == KeyCode::Key5 {
                        self.handle_menu_press(ctx);
                    }
                }
            } else if was_editing && !self.is_editing {
                let _ = ctx.flush_config();
            }
            return EventResult::Consumed;
        }

        if let Event::Key { code, state } = *event {
            if state == KeyState::Pressed && code == KeyCode::Exit {
                if self.is_editing {
                    self.reset_input_state();
                    let _ = ctx.flush_config();
                    return EventResult::Consumed;
                }

                if self.pop_submenu(ctx) {
                    return EventResult::Consumed;
                }

                if ctx.config.is_dirty() {
                    let _ = ctx.flush_config();
                }
            } else if state == KeyState::Held && (code == KeyCode::Menu || code == KeyCode::Key5) {
                // Prevent Menu hold from leaking to global orchestrator shortcut while inside settings
                return EventResult::Consumed;
            }
        }

        EventResult::Ignored
    }

    #[inline(never)]
    fn handle_sheet_selected(&mut self, ctx: &mut SysContext, tag: u8, idx: usize) -> EventResult {
        match tag {
            #[cfg(feature = "boot_assets")]
            TAG_SPLASH_CHOOSER => {
                if super::device::DeviceDomain::apply_splash_choice(idx, ctx) {
                    ctx.config.mark_display_dirty();
                }
            }
            #[cfg(feature = "boot_assets")]
            TAG_ROGER_CHOOSER => {
                if super::audio::AudioDomain::apply_roger_choice(idx, ctx) {
                    ctx.config.mark_audio_dirty();
                }
            }
            #[cfg(feature = "boot_assets")]
            TAG_JINGLE_CHOOSER => {
                if super::audio::AudioDomain::apply_jingle_choice(idx, ctx) {
                    ctx.config.mark_audio_dirty();
                }
            }
            TAG_MODEL_CHOOSER => {
                if idx <= 1 {
                    let name = if idx == 0 { "UV-K1" } else { "UV-K5 V3" };
                    ctx.config.system.set_custom_model(name);
                    ctx.config.system.model = idx as u8;
                    ctx.config.sanitize();
                    ctx.config.mark_system_dirty();
                } else {
                    #[cfg(feature = "passcode")]
                    prompt_config_str(ctx, TAG_MODEL_INPUT);
                    return EventResult::Consumed;
                }
            }
            TAG_CHOICE_SHEET => {
                self.handle_radio_confirmed(ctx, idx);
            }
            _ => return EventResult::Ignored,
        }
        self.flush_and_refresh(ctx);
        EventResult::Consumed
    }

    #[cfg(feature = "passcode")]
    #[inline(never)]
    fn handle_text_input_done(&mut self, ctx: &mut SysContext, tag: u8, text: &str) -> EventResult {
        match tag {
            TAG_MODEL_INPUT | TAG_OWNER_INPUT | TAG_TAG_INPUT | TAG_RADIO_ID_INPUT => {
                match tag {
                    TAG_MODEL_INPUT => {
                        ctx.config.system.set_custom_model(text);
                        ctx.config.sanitize();
                    }
                    TAG_RADIO_ID_INPUT => {
                        if let Some(id) = parse_u32_digits(text) {
                            ctx.config.system.radio_id = id;
                        }
                    }
                    _ => {
                        let dst: &mut [u8] = if tag == TAG_OWNER_INPUT {
                            &mut ctx.config.system.owner
                        } else {
                            &mut ctx.config.system.tag
                        };
                        Self::copy_padded(dst, text.as_bytes());
                    }
                }
                ctx.config.mark_system_dirty();
            }
            TAG_PASSCODE_VERIFY => {
                super::security::handle_passcode_verify(ctx, text);
                return EventResult::Consumed;
            }
            TAG_PASSCODE_NEW => {
                if text.is_empty() {
                    let _ = crate::security::passcode::disable_passcode(ctx.storage, ctx.passcode);
                    ctx.show_popup(TAG_POPUP_INFO, "", "Passcode Removed");
                } else {
                    self.passcode_candidate.clear();
                    let _ = self.passcode_candidate.push_str(text);
                    let candidate_len = self.passcode_candidate.len();
                    let digits_only =
                        crate::security::passcode::get_passcode_config(ctx.storage).digits_only;
                    ctx.prompt_passcode(
                        TAG_PASSCODE_CONFIRM,
                        "Confirm Passcode",
                        candidate_len,
                        digits_only,
                        true,
                    );
                    return EventResult::Consumed;
                }
            }
            TAG_PASSCODE_CONFIRM => {
                super::security::handle_passcode_confirm(
                    self.passcode_candidate.as_str(),
                    text,
                    ctx,
                );
            }
            _ => return EventResult::Ignored,
        }
        self.flush_and_refresh(ctx);
        EventResult::Consumed
    }
}

impl App<SysContext<'_, '_>> for SettingsApp {
    fn meta() -> AppMetadata {
        AppMetadata {
            name: "Settings",
            icon: &super::assets::ICON_16,
            icon_8: &super::assets::ICON_8,
        }
    }

    fn status_bar_config(&self) -> StatusBarConfig<'_> {
        StatusBarConfig {
            visible: true,
            title: Some(self.title),
            ..Default::default()
        }
    }

    fn on_start(&mut self, ctx: &mut SysContext) {
        self.current_page_id = PAGE_ROOT;
        self.stack_top = 0;
        self.title = "Settings";
        self.selected_idx = 0;
        self.dirty_timer_ms = 0;

        self.view.reset_scroll();
        self.populate_current_menu(ctx);
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        if let Event::Tick = *event {
            let now_ms = py32_hal::delay::millis();
            let dt = if self.last_tick_ms == 0 {
                50
            } else {
                now_ms.wrapping_sub(self.last_tick_ms).min(250) as u16
            };
            self.last_tick_ms = now_ms;

            // Live metric polling (throttle to once per second to save CPU and battery)
            if self.current_page_id == PAGE_BATTERY_INFO || self.current_page_id == PAGE_SYSTEM {
                if (ctx.power.uptime_secs & 1) == 0 {
                    self.populate_current_menu(ctx);
                }
            }
            if self.dirty_timer_ms > 0 {
                self.dirty_timer_ms = self.dirty_timer_ms.saturating_sub(dt);
                if self.dirty_timer_ms == 0 {
                    if ctx.flush_config().is_err() {
                        ctx.show_popup(
                            TAG_POPUP_INFO,
                            "Save Error",
                            "Failed to write configuration",
                        );
                    }
                }
            }
        }

        self.handle_menu_screen_event(ctx, event)
    }

    fn on_modal_result(&mut self, ctx: &mut SysContext, result: &ModalResult<'_>) -> EventResult {
        match result {
            ModalResult::SheetSelected { tag, index } => {
                self.handle_sheet_selected(ctx, *tag, *index)
            }
            #[cfg(feature = "passcode")]
            ModalResult::TextInputDone { tag, text } => {
                self.handle_text_input_done(ctx, *tag, text)
            }
            ModalResult::Confirmed(tag) => match *tag {
                TAG_APP_RESET_CONFIRM => {
                    ctx.show_popup(
                        TAG_POPUP_INFO,
                        "Config Reset",
                        "Defaults restored on next launch",
                    );
                    EventResult::Consumed
                }
                TAG_APP_DELETE_CONFIRM => {
                    let cat = crate::apps::launcher::catalog::get_catalog();
                    let path = cat
                        .apps()
                        .nth(self.selected_app_idx)
                        .map(|app| app.path.clone());
                    if let Some(path) = path {
                        let _ = ctx.storage.remove(path.as_str());

                        // If dynamic app was loaded, unregister it and mark process dead
                        let dyn_id = crate::apps::AppRegistry::count();
                        crate::apps::AppRegistry::unregister_dynamic();
                        if let Some(pcb) =
                            crate::orchestrator::proc_table_mut().find_mut_by_app(dyn_id)
                        {
                            pcb.set_state(kernel::proc::ProcState::Dead);
                        }

                        ctx.show_popup(TAG_POPUP_INFO, "App Deleted", "Application uninstalled");
                        cat.mark_dirty();
                        cat.scan(ctx.storage);
                        crate::apps::launcher::reload_launcher(ctx.storage);
                        if self.current_page_id == PAGE_APP_DETAIL {
                            self.pop_submenu(ctx);
                        } else {
                            self.populate_current_menu(ctx);
                            if self.selected_idx >= self.items_len && self.items_len > 0 {
                                self.selected_idx = self.items_len - 1;
                            }
                        }
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            },
            _ => EventResult::Ignored,
        }
    }

    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, _ctx: &SysContext) {
        let model = self.build_menu_model(self.is_editing);
        self.view.render(canvas, &model);
        if self.slide_offset_x != 0 {
            canvas.scroll_horizontal(self.slide_offset_x as i8, 0);
        }
    }

    fn on_exit(&mut self, ctx: &mut SysContext) {
        if ctx.config.is_dirty() {
            let _ = ctx.flush_config();
        }
    }
}
