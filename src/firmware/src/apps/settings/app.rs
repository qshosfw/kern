//! Industrial System Configuration & Settings Application
//! Provides modular submodules for Display, Audio, Buttons, Battery, and System.

use crate::SysContext;
use gfx::Canvas;
use gfx::gui::modules::menu::{
    MenuItem, MenuItemType, MenuModel, MenuModelMut, MenuView, OptionSheet,
};
use gfx::gui::view::View;
use kernel::app::{App, AppMetadata, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};

use super::dialog;
use super::options::get_page;
use super::page::{MenuFrame, PageId, SettingKind};

pub struct SettingsApp {
    pub(crate) items: [MenuItem; 16],
    pub(crate) item_icons: [&'static [u8; 8]; 16],
    pub(crate) items_len: usize,

    pub(crate) current_page: PageId,
    pub(crate) stack: [MenuFrame; 4],
    pub(crate) stack_top: usize,

    pub(crate) title: &'static str,
    pub(crate) selected_idx: usize,
    pub(crate) is_editing: bool,
    pub(crate) is_pressed: bool,
    pub(crate) dirty_timer_ms: u16,
    pub(crate) last_tick_ms: u32,

    pub(crate) view: MenuView,
    #[cfg(feature = "passcode")]
    pub(crate) passcode_candidate: heapless::String<16>,

    pub selected_pkg_idx: usize,
    pub slide_offset_x: i16,
    #[cfg(feature = "boot_assets")]
    pub(crate) active_asset: Option<&'static super::asset_chooser::AssetDescriptor>,
}

impl SettingsApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).title = "Settings";
            (*ptr).view = MenuView::new();
            (*ptr).selected_pkg_idx = 0;
            (*ptr).slide_offset_x = 0;
            (*ptr).dirty_timer_ms = 0;
            (*ptr).last_tick_ms = 0;
            #[cfg(feature = "boot_assets")]
            {
                (*ptr).active_asset = None;
            }
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.slide_offset_x != 0 || self.view.scroller.is_animating()
    }

    pub(crate) fn reset_input_state(&mut self) {
        self.is_pressed = false;
        self.is_editing = false;
    }

    fn active_items(&self) -> &[MenuItem] {
        &self.items[..self.items_len]
    }

    pub const ROOT_ICONS: &'static [&'static [u8; 8]] = &[
        &gfx::icons::ICON_DISPLAY,
        &gfx::icons::ICON_AUDIO,
        &gfx::icons::ICON_BUTTONS,
        &gfx::icons::ICON_BATTERY,
        &gfx::icons::ICON_DEVICE,
        &gfx::icons::ICON_APPS,
        #[cfg(feature = "passcode")]
        &gfx::icons::ICON_SECURITY,
        &gfx::icons::ICON_SYSTEM,
    ];

    #[inline(never)]
    fn build_menu_model<'a>(&'a self, is_editing: bool) -> MenuModel<'a> {
        let icons = if self.current_page == PageId::Root {
            Some(Self::ROOT_ICONS)
        } else if self.current_page == PageId::ManageApps || self.current_page == PageId::ManageServices {
            Some(&self.item_icons[..self.items_len])
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

    #[inline(never)]
    fn handle_menu_press(&mut self, ctx: &mut SysContext) {
        let sel_idx = self.selected_idx;
        if sel_idx >= self.items_len {
            return;
        }
        self.is_pressed = false;

        #[cfg(feature = "passcode")]
        if self.current_page == PageId::Security {
            if sel_idx == 0 {
                dialog::handle_security_press(ctx);
                return;
            }
        }

        if super::apps::handle_package_press(self, ctx, sel_idx) {
            return;
        }

        let (_, defs) = get_page(self.current_page);
        if let Some(def) = defs.get(sel_idx) {
            match def.kind() {
                SettingKind::Submenu(target_page) => {
                    self.push_submenu(target_page, ctx);
                    return;
                }
                SettingKind::Action(action_id) => {
                    dialog::handle_action_press(ctx, action_id);
                    return;
                }
                #[cfg(feature = "boot_assets")]
                SettingKind::Asset(desc) => {
                    self.active_asset = Some(desc);
                    let sheet = desc.build_sheet(&ctx.config, ctx.storage);
                    ctx.show_sheet(dialog::TAG_ASSET_CHOOSER, sheet);
                    return;
                }
                _ => {}
            }
        }

        let item = &self.items[sel_idx];
        if let MenuItemType::Choice { selected, options } = item.item_type {
            ctx.show_sheet(
                dialog::TAG_CHOICE_SHEET,
                OptionSheet::new(item.title, options, selected as usize).with_radio(true),
            );
        }
    }

    fn dispatch_view_event(&mut self, ctx: &mut SysContext, event: &Event) -> bool {
        let prev_idx = self.selected_idx;
        let was_editing = self.is_editing;
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

        if handled {
            if self.selected_idx != prev_idx
                && ctx.config.audio.key_beep != 0
                && matches!(event, Event::Tick | Event::Encoder(_))
            {
                ctx.play_beep(crate::services::melody::BeepType::Scroll);
            }
            if self.is_editing || was_editing {
                self.update_config_from_menu(ctx);
            }
        }
        handled
    }

    #[inline(never)]
    fn handle_menu_screen_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        self.view.animated = ctx.config.display.animations_enabled();

        // 1. If any key event arrives, immediately complete in-flight slide animation
        // so navigation feels instantaneous and no key presses are delayed or blocked.
        if matches!(event, Event::Key { .. }) && self.slide_offset_x != 0 {
            self.slide_offset_x = 0;
        }

        // 2. Handle Exit key (back navigation / cancel edit)
        if let Event::Key {
            code: KeyCode::Exit,
            state,
        } = *event
        {
            if state == KeyState::Pressed {
                if self.is_editing {
                    self.view.repeater.reset();
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
                // At root page: return Ignored so orchestrator navigates back to Launcher
                return EventResult::Ignored;
            } else if state == KeyState::Released {
                return EventResult::Consumed;
            }
        }

        // 3. Handle Menu / Key5 (Enter / Select)
        if let Event::Key {
            code: KeyCode::Menu | KeyCode::Key5,
            state,
        } = *event
        {
            if state == KeyState::Pressed {
                if self.is_editing {
                    self.view.repeater.reset();
                    self.reset_input_state();
                    let _ = ctx.flush_config();
                    return EventResult::Consumed;
                }

                let sel_idx = self.selected_idx;
                if sel_idx < self.items_len {
                    match self.items[sel_idx].item_type {
                        MenuItemType::Range { .. } => {
                            self.view.repeater.reset();
                            self.is_editing = true;
                            return EventResult::Consumed;
                        }
                        MenuItemType::Toggle(ref mut on) => {
                            *on = !*on;
                            self.update_config_from_menu(ctx);
                            return EventResult::Consumed;
                        }
                        _ => {
                            self.handle_menu_press(ctx);
                            return EventResult::Consumed;
                        }
                    }
                }
                return EventResult::Consumed;
            } else if state == KeyState::Held || state == KeyState::Released {
                // Prevent hold/release from leaking to global orchestrator shortcuts
                return EventResult::Consumed;
            }
        }

        // 4. Handle Tick event (animation and smooth scrolling)
        if let Event::Tick = event {
            let mut animating = false;
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
                    animating = true;
                }
            }

            let handled = self.dispatch_view_event(ctx, event);

            if animating || handled || self.slide_offset_x != 0 || self.view.scroller.is_animating()
            {
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }

        // 5. Handle directional navigation & list scrolling (Up, Down, Encoder, etc.)
        if self.dispatch_view_event(ctx, event) {
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }
}

impl App<SysContext<'_, '_>> for SettingsApp {
    fn meta() -> AppMetadata {
        AppMetadata {
            name: "Settings",
            icon: &gfx::icons::ICON_16_SETTINGS,
            icon_8: &gfx::icons::ICON_APP_SETTINGS,
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
        self.current_page = PageId::Root;
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
            if self.current_page == PageId::BatteryInfo || self.current_page == PageId::System {
                if (ctx.power.uptime_secs & 1) == 0 {
                    self.populate_current_menu(ctx);
                }
            }
            if self.dirty_timer_ms > 0 {
                self.dirty_timer_ms = self.dirty_timer_ms.saturating_sub(dt);
                if self.dirty_timer_ms == 0 {
                    if ctx.flush_config().is_err() {
                        ctx.show_popup(
                            dialog::TAG_POPUP_INFO,
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
                dialog::handle_sheet_selected(self, ctx, *tag, *index)
            }
            #[cfg(feature = "text_input")]
            ModalResult::TextInputDone { tag, text } => {
                dialog::handle_text_input_done(self, ctx, *tag, text)
            }
            ModalResult::Confirmed(tag) => dialog::handle_confirm_result(self, ctx, *tag),
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
