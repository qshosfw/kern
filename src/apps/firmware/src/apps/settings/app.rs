use crate::SysContext;
use gfx::Canvas;
use gfx::gui::modules::menu::{
    MenuItem, MenuItemType, MenuModel, MenuModelMut, MenuView, OptionSheet, ShortString,
};
use gfx::gui::view::View;
use gfx::icons;
use kernel::app::{App, AppMetadata, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};

use super::audio::AudioDomain;
use super::options::{
    ACTION_CHOOSE_MODEL, ACTION_EDIT_OWNER, ACTION_EDIT_RADIO_ID, ACTION_EDIT_TAG,
    ACTION_ROGER_CHOOSER, ACTION_SPLASH_CHOOSER, PAGE_AUDIO, PAGE_BATTERY_INFO, PAGE_ROOT,
    PAGE_SECURITY, PAGE_SYSTEM, PAGE_SYSTEM_STORAGE, SettingKind, apply_page_changes, find_page,
    populate_defs,
};

const TAG_PASSCODE_VERIFY: u8 = 2;
pub const TAG_PASSCODE_NEW: u8 = 3;
const TAG_PASSCODE_CONFIRM: u8 = 4;
const TAG_ROGER_CHOOSER: u8 = 5;
const TAG_CHOICE_SHEET: u8 = 6;
pub const TAG_POPUP_INFO: u8 = 7;
const TAG_OWNER_INPUT: u8 = 8;
const TAG_TAG_INPUT: u8 = 9;
const TAG_RADIO_ID_INPUT: u8 = 10;
const TAG_SPLASH_CHOOSER: u8 = 11;
const TAG_MODEL_CHOOSER: u8 = 12;
const TAG_MODEL_INPUT: u8 = 13;

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

    view: MenuView,
    passcode_candidate: heapless::String<16>,
}

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

fn prompt_config_str(ctx: &mut SysContext, tag: u8, title: &'static str, which: u8) {
    let mut buf = ShortString::empty();
    let (max_len, digits) = match which {
        0 => {
            let _ = buf.push_str(ctx.config.system.owner_str());
            (12, false)
        }
        1 => {
            let _ = buf.push_str(ctx.config.system.tag_str());
            (8, false)
        }
        2 => {
            let _ = buf.push_str(ctx.config.system.custom_model_str());
            (12, false)
        }
        _ => {
            buf = ShortString::from_u32(ctx.config.system.radio_id);
            (8, true)
        }
    };
    ctx.prompt_text(tag, title, buf.as_str(), max_len, digits);
}

impl SettingsApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).title = "Settings";
            (*ptr).view = MenuView::new();
        }
    }

    fn reset_input_state(&mut self) {
        self.is_pressed = false;
        self.is_editing = false;
    }

    #[inline(never)]
    fn populate_current_menu(&mut self, ctx: &mut SysContext) {
        if self.current_page_id == PAGE_SECURITY {
            self.items_len = super::security::populate_security(&mut self.items, ctx);
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
            self.reset_input_state();
            ctx.sink_exit_key();

            let _ = ctx.flush_config();
            true
        } else {
            false
        }
    }

    #[inline(never)]
    fn update_config_from_menu(&mut self, ctx: &mut SysContext) {
        if let Some(page) = find_page(self.current_page_id) {
            let idx = self.selected_idx;
            if idx < self.items_len && idx < page.defs.len() {
                let changed =
                    super::options::update_single_def(&self.items[idx], &page.defs[idx], ctx);
                if changed {
                    apply_page_changes(self.current_page_id, ctx);
                    self.dirty_timer_ms = 250;
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

    pub const ROOT_ICONS: [&'static [u8; 8]; 8] = [
        &gfx::icons::ICON_DISPLAY,
        &gfx::icons::ICON_AUDIO,
        &gfx::icons::ICON_BUTTONS,
        &gfx::icons::ICON_BATTERY,
        &gfx::icons::ICON_DEVICE,
        &gfx::icons::ICON_APPS,
        &gfx::icons::ICON_SECURITY,
        &gfx::icons::ICON_SYSTEM,
    ];

    fn build_menu_model<'a>(&'a self, is_editing: bool) -> MenuModel<'a> {
        let icons = if self.current_page_id == PAGE_ROOT {
            Some(&Self::ROOT_ICONS[..])
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
            let sel = (ctx.config.system.model as usize).min(2);
            ctx.show_sheet(
                TAG_MODEL_CHOOSER,
                OptionSheet::new("Model", &super::options::MODEL_OPTIONS, sel).with_radio(true),
            );
        }
        ACTION_EDIT_OWNER => prompt_config_str(ctx, TAG_OWNER_INPUT, "Owner", 0),
        ACTION_EDIT_TAG => prompt_config_str(ctx, TAG_TAG_INPUT, "Tag", 1),
        ACTION_EDIT_RADIO_ID => prompt_config_str(ctx, TAG_RADIO_ID_INPUT, "Radio ID", 3),
        ACTION_ROGER_CHOOSER => {
            let sheet = AudioDomain::create_roger_chooser(ctx);
            ctx.show_sheet(TAG_ROGER_CHOOSER, sheet);
        }
        ACTION_SPLASH_CHOOSER => {
            let sheet = super::device::DeviceDomain::create_splash_chooser(ctx);
            ctx.show_sheet(TAG_SPLASH_CHOOSER, sheet);
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

        if self.current_page_id == PAGE_SECURITY && sel_idx == 0 {
            handle_security_press(ctx);
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

    fn copy_padded(dst: &mut [u8], src: &[u8]) {
        dst.fill(b' ');
        let len = src.len().min(dst.len());
        dst[..len].copy_from_slice(&src[..len]);
    }

    #[inline(never)]
    fn handle_radio_confirmed(&mut self, ctx: &mut SysContext, sel: usize) {
        let sel_idx = self.selected_idx;
        if sel_idx < self.items_len {
            if let MenuItemType::Choice {
                ref mut selected, ..
            } = self.items[sel_idx].item_type
            {
                *selected = sel as u16;
            }
            if self.current_page_id == PAGE_AUDIO
                && super::audio::AudioDomain::on_choice_confirmed(sel_idx, sel, ctx)
            {
                // Handled by AudioDomain
            } else if self.current_page_id == PAGE_SECURITY {
                super::security::update_security_option(ctx, sel_idx, sel);
            } else {
                self.update_config_from_menu(ctx);
            }
        }
    }

    #[inline(never)]
    fn handle_menu_screen_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        let was_editing = self.is_editing;
        let handled = {
            let mut model = MenuModelMut {
                items: &mut self.items[..self.items_len],
                selected_idx: self.selected_idx,
                nav_layout: ctx.config.keys.nav_layout,
                is_editing: self.is_editing,
                is_pressed: self.is_pressed,
            };

            let handled = self.view.on_event(&mut model, event);

            self.selected_idx = model.selected_idx;
            self.is_editing = model.is_editing;
            self.is_pressed = model.is_pressed;

            handled
        };

        if handled {
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
            }
        }

        EventResult::Ignored
    }

    #[inline(never)]
    fn handle_sheet_selected(&mut self, ctx: &mut SysContext, tag: u8, idx: usize) -> EventResult {
        match tag {
            TAG_ROGER_CHOOSER => {
                if AudioDomain::apply_roger_choice(idx, ctx) {
                    ctx.config.mark_audio_dirty();
                }
            }
            TAG_SPLASH_CHOOSER => {
                if super::device::DeviceDomain::apply_splash_choice(idx, ctx) {
                    ctx.config.mark_display_dirty();
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
                    prompt_config_str(ctx, TAG_MODEL_INPUT, "Model", 2);
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
            icon: &icons::ICON_16_SETTINGS,
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

        self.populate_current_menu(ctx);
    }

    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        if let Event::Tick = *event {
            if self.current_page_id == PAGE_BATTERY_INFO
                || self.current_page_id == PAGE_SYSTEM
                || self.current_page_id == PAGE_SYSTEM_STORAGE
            {
                self.populate_current_menu(ctx);
            }
            if self.dirty_timer_ms > 0 {
                self.dirty_timer_ms = self.dirty_timer_ms.saturating_sub(50);
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

    fn on_modal_result(&mut self, ctx: &mut SysContext, result: &ModalResult) -> EventResult {
        match result {
            ModalResult::SheetSelected { tag, index } => {
                self.handle_sheet_selected(ctx, *tag, *index)
            }
            ModalResult::TextInputDone { tag, text } => {
                self.handle_text_input_done(ctx, *tag, text.as_str())
            }
            _ => EventResult::Ignored,
        }
    }

    fn render(&self, canvas: &mut Canvas, _ctx: &SysContext) {
        let model = self.build_menu_model(self.is_editing);
        self.view.render(canvas, &model);
    }

    fn on_exit(&mut self, ctx: &mut SysContext) {
        let _ = ctx.flush_config();
    }
}
