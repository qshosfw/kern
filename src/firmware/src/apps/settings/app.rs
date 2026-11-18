//! Industrial System Configuration & Settings Application
//! Provides modular submodules for Display, Audio, Buttons, Battery, and System.

use crate::SysContext;
use gfx::Canvas;
use gfx::gui::modules::menu::{MenuItem, MenuItemType, MenuModel, MenuModelMut, MenuView};
use gfx::gui::view::View;
use kernel::app::{App, AppMetadata, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};

use super::dialog;
use super::options::page_defs;
#[cfg(feature = "filesystem")]
use super::page::TAG_ASSET;
use super::page::{ActionId, MenuFrame, PageId, TAG_ACTION, TAG_SUBMENU};

#[repr(C)]
pub struct SettingsApp {
    // ── Hot 1- & 2-byte scalars inside first 32B (Thumb-1 imm5 ldrb/ldrh) ──
    pub(crate) current_page: PageId,
    pub(crate) is_editing: bool,
    pub(crate) is_pressed: bool,
    pub(crate) resume_nav: bool,
    pub(crate) active_action_module: bool,
    pub(crate) shortcut_ctx: u8,
    pub(crate) dirty_timer_ms: u16,
    pub(crate) last_poll_sec: u16,
    pub slide_offset_x: i16,
    // ── Hot word fields inside first 124B ──
    pub(crate) items_len: usize,
    pub(crate) selected_idx: usize,
    pub(crate) stack_top: usize,
    pub(crate) last_tick_ms: u32,
    pub selected_pkg_idx: usize,
    pub(crate) title: &'static str,
    #[cfg(feature = "filesystem")]
    pub(crate) active_asset: Option<&'static super::asset_chooser::AssetDescriptor>,
    #[cfg(feature = "passcode")]
    pub(crate) passcode_candidate: heapless::String<16>,
    // ── Cold/large structures ──
    pub(crate) stack: [MenuFrame; 4],
    pub(crate) view: MenuView,
    pub(crate) item_icons: [&'static [u8; 8]; 20],
    pub(crate) items: [MenuItem; 20],
}

impl SettingsApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).title = "Settings";
            (*ptr).view = MenuView::new();
            (*ptr).selected_pkg_idx = 0;
            (*ptr).resume_nav = false;
            (*ptr).active_action_module = false;
            (*ptr).shortcut_ctx = 0;
            (*ptr).slide_offset_x = 0;
            (*ptr).dirty_timer_ms = 0;
            (*ptr).last_poll_sec = u16::MAX;
            (*ptr).last_tick_ms = 0;
            #[cfg(feature = "filesystem")]
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
        gfx::icons::ICON_DISPLAY,
        gfx::icons::ICON_AUDIO,
        gfx::icons::ICON_BUTTONS,
        gfx::icons::ICON_BATTERY,
        gfx::icons::ICON_DEVICE,
        gfx::icons::ICON_APPS,
        #[cfg(feature = "passcode")]
        gfx::icons::ICON_SECURITY,
        gfx::icons::ICON_SYSTEM,
    ];

    #[inline(never)]
    fn build_menu_model<'a>(&'a self, is_editing: bool) -> MenuModel<'a> {
        let icons = if self.current_page == PageId::Root {
            Some(Self::ROOT_ICONS)
        } else if matches!(
            self.current_page,
            PageId::ManageApps | PageId::ManageServices | PageId::ButtonsShortcuts
        ) {
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

        if self.current_page == PageId::ButtonsShortcuts && sel_idx > 0 {
            let slot = sel_idx - 1;
            if slot < crate::apps::shortcut::KEY_SHORTCUT_COUNT {
                let cat = crate::apps::launcher::catalog::get_catalog();
                cat.scan(ctx.storage);
                let cur_id = crate::apps::shortcut::resolve_slot(
                    &ctx.config.keys,
                    slot,
                    self.shortcut_ctx,
                );
                let sheet = crate::apps::shortcut::build_picker_sheet(
                    crate::apps::shortcut::slot_label(slot),
                    cur_id,
                    cat,
                );
                ctx.show_sheet(dialog::TAG_SHORTCUT_SHEET, sheet);
            }
            return;
        }

        let defs = page_defs(self.current_page);
        #[cfg(feature = "krx_native")]
        let def_idx = if self.current_page == PageId::System {
            let cat = crate::apps::launcher::catalog::get_catalog();
            cat.scan(ctx.storage);
            let sys_caps = ctx.caps();
            let menu_flag = kernel::proc::krx::SectionType::FLAG_SYSTEM_MENU;
            let act_flag = kernel::proc::krx::SectionType::FLAG_SYSTEM_ACTION;
            let menu_count = cat.system_entry_count(sys_caps, menu_flag);
            let act_count = cat.system_entry_count(sys_caps, act_flag);
            let ins_menu = super::system::system_modules_insert_idx().min(defs.len());
            let ins_act = (super::system::system_actions_insert_idx() + menu_count)
                .min(defs.len() + menu_count);

            let picked: Option<(&str, bool)> =
                if sel_idx >= ins_menu && sel_idx < ins_menu + menu_count {
                    cat.system_entry_at(sys_caps, menu_flag, sel_idx - ins_menu)
                        .map(|e| (e.path.as_str(), false))
                } else if sel_idx >= ins_act && sel_idx < ins_act + act_count {
                    cat.system_entry_at(sys_caps, act_flag, sel_idx - ins_act)
                        .map(|e| (e.path.as_str(), true))
                } else {
                    None
                };

            if let Some((path, is_action)) = picked {
                let res = if is_action {
                    #[cfg(feature = "filesystem")]
                    {
                        self.active_action_module = true;
                        let r = crate::runtime::loader::invoke_action_krx_package(ctx, path);
                        if r.is_err() {
                            self.active_action_module = false;
                        }
                        r
                    }
                    #[cfg(not(feature = "filesystem"))]
                    Ok(())
                } else {
                    self.resume_nav = true;
                    let r = crate::apps::runtime::launch_krx_package(ctx, path).map(|_| ());
                    if r.is_err() {
                        self.resume_nav = false;
                    }
                    r
                };
                if let Err(err) = res {
                    crate::runtime::loader::show_launch_error_dialog(ctx, path, err);
                    crate::apps::launcher::catalog::get_catalog().mark_dirty();
                    self.populate_current_menu(ctx);
                    if self.selected_idx >= self.items_len {
                        self.selected_idx = self.items_len.saturating_sub(1);
                    }
                }
                return;
            }

            let mut idx = sel_idx;
            if sel_idx >= ins_act + act_count {
                idx -= act_count;
            }
            if sel_idx >= ins_menu + menu_count {
                idx -= menu_count;
            }
            idx
        } else {
            sel_idx
        };
        #[cfg(not(feature = "krx_native"))]
        let def_idx = sel_idx;

        if let Some(def) = defs.get(def_idx) {
            match def.tag {
                TAG_SUBMENU => {
                    if let Some(target_page) = PageId::from_u8(def.aux) {
                        self.push_submenu(target_page, ctx);
                    }
                    return;
                }
                TAG_ACTION => {
                    if let Some(action_id) = ActionId::from_u8(def.aux) {
                        dialog::handle_action_press(ctx, action_id);
                    }
                    return;
                }
                #[cfg(feature = "filesystem")]
                TAG_ASSET => {
                    if let Some(&desc) = super::page::ASSETS.get(def.aux as usize) {
                        self.active_asset = Some(desc);
                        let sheet = desc.build_sheet(&ctx.config, ctx.storage);
                        ctx.show_sheet(dialog::TAG_ASSET_CHOOSER, sheet);
                    }
                    return;
                }
                _ => {}
            }
        }

        let item = &self.items[sel_idx];
        if let MenuItemType::Choice { selected, options } = item.item_type {
            dialog::show_radio_sheet(
                ctx,
                dialog::TAG_CHOICE_SHEET,
                item.title,
                options,
                selected as usize,
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

        if self.is_editing
            && matches!(
                *event,
                Event::Key {
                    code: KeyCode::Exit | KeyCode::Menu | KeyCode::Key5,
                    state: KeyState::Pressed,
                }
            )
        {
            self.view.repeater.reset();
            self.reset_input_state();
            let _ = ctx.flush_config();
            return EventResult::Consumed;
        }

        // 2. Handle Exit key (back navigation / cancel edit)
        if let Event::Key {
            code: KeyCode::Exit,
            state,
        } = *event
        {
            if state == KeyState::Pressed {
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
        #[cfg(feature = "settings_info")]
        if self.current_page == PageId::SystemInfo {
            let dir: isize = match *event {
                Event::Key {
                    code: KeyCode::Up | KeyCode::Key2,
                    state: KeyState::Pressed | KeyState::Held,
                } => -1,
                Event::Key {
                    code: KeyCode::Down | KeyCode::Key8,
                    state: KeyState::Pressed | KeyState::Held,
                } => 1,
                Event::Encoder(d) => d as isize,
                _ => 0,
            };
            if dir != 0 {
                let max = super::system::system_info_row_count()
                    - super::system::SYSTEM_INFO_VISIBLE_ROWS;
                let prev = self.selected_idx;
                self.selected_idx =
                    (self.selected_idx as isize + dir).clamp(0, max as isize) as usize;
                if self.selected_idx != prev && ctx.config.audio.key_beep != 0 {
                    ctx.play_beep(crate::services::melody::BeepType::Scroll);
                }
                return EventResult::Consumed;
            }
            if matches!(
                *event,
                Event::Key {
                    code: KeyCode::Up | KeyCode::Down | KeyCode::Key2 | KeyCode::Key8,
                    state: KeyState::Released,
                }
            ) {
                return EventResult::Consumed;
            }
        }

        if self.dispatch_view_event(ctx, event) {
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    #[inline(always)]
    fn begin_start(&mut self) {
        if self.resume_nav {
            self.resume_nav = false;
            return;
        }
        self.active_action_module = false;
        self.current_page = PageId::Root;
        self.stack_top = 0;
        self.title = "Settings";
        self.selected_idx = 0;
        self.dirty_timer_ms = 0;
        self.last_poll_sec = u16::MAX;
        self.view.reset_scroll();
    }
}

impl App<SysContext<'_, '_>> for SettingsApp {
    fn meta() -> AppMetadata {
        AppMetadata {
            name: "Settings",
            icon: &gfx::icons::ICON_16_SETTINGS,
            icon_8: gfx::icons::ICON_APP_SETTINGS,
        }
    }

    fn status_bar_config(&self) -> StatusBarConfig<'_> {
        StatusBarConfig {
            visible: true,
            title: Some(self.title),
            ..Default::default()
        }
    }

    #[inline(never)]
    fn on_start(&mut self, ctx: &mut SysContext) {
        self.begin_start();
        self.populate_current_menu(ctx);
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        let mut polled_info = false;
        if let Event::Tick = *event {
            let now_ms = kernel::proc::primitives::TimerPrimitive::uptime_ms(ctx);
            let dt = if self.last_tick_ms == 0 {
                50
            } else {
                now_ms.wrapping_sub(self.last_tick_ms).min(250) as u16
            };
            self.last_tick_ms = now_ms;

            // Live metric polling (refresh once per second transition to save CPU and battery)
            let on_uptime_page = {
                #[cfg(feature = "settings_info")]
                {
                    self.current_page == PageId::BatteryInfo
                        || self.current_page == PageId::SystemInfo
                }
                #[cfg(not(feature = "settings_info"))]
                {
                    self.current_page == PageId::BatteryInfo
                }
            };
            if on_uptime_page {
                let cur_sec = ctx.power.uptime_secs as u16;
                if cur_sec != self.last_poll_sec {
                    self.last_poll_sec = cur_sec;
                    polled_info = true;
                    if self.current_page == PageId::BatteryInfo {
                        self.populate_current_menu(ctx);
                    }
                }
            }
            if self.dirty_timer_ms > 0 {
                self.dirty_timer_ms = self.dirty_timer_ms.saturating_sub(dt);
                if self.dirty_timer_ms == 0 {
                    if ctx.flush_config().is_err() {
                        ctx.show_popup(dialog::TAG_POPUP_INFO, "Save Error", "Write failed");
                    }
                }
            }
        }

        match self.handle_menu_screen_event(ctx, event) {
            EventResult::Consumed => EventResult::Consumed,
            other => {
                if polled_info {
                    EventResult::Consumed
                } else {
                    other
                }
            }
        }
    }

    fn on_modal_result(&mut self, ctx: &mut SysContext, result: &ModalResult<'_>) -> EventResult {
        #[cfg(all(feature = "filesystem", feature = "krx_native"))]
        if self.active_action_module {
            self.active_action_module = false;
            return crate::runtime::loader::finish_action_krx_package(ctx, result);
        }
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
        #[cfg(feature = "settings_info")]
        let is_info = self.current_page == PageId::SystemInfo;
        #[cfg(not(feature = "settings_info"))]
        let is_info = false;
        if is_info {
            #[cfg(feature = "settings_info")]
            super::system::render_system_info_page(canvas, _ctx, self.selected_idx);
        } else {
            let model = self.build_menu_model(self.is_editing);
            self.view.render(canvas, &model);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_app_hot_scalars_fit_thumb1_imm5_window() {
        assert!(core::mem::offset_of!(SettingsApp, current_page) < 32);
        assert!(core::mem::offset_of!(SettingsApp, is_editing) < 32);
        assert!(core::mem::offset_of!(SettingsApp, is_pressed) < 32);
        assert!(core::mem::offset_of!(SettingsApp, dirty_timer_ms) < 32);
        assert!(core::mem::offset_of!(SettingsApp, last_poll_sec) < 32);
        assert!(core::mem::offset_of!(SettingsApp, items_len) < 124);
        assert!(core::mem::offset_of!(SettingsApp, selected_idx) < 124);
        assert!(core::mem::offset_of!(SettingsApp, stack_top) < 124);
    }

    #[test]
    fn system_menu_groups_info_and_bootloader_cleanly() {
        let sys_defs = page_defs(PageId::System);
        let mut idx = 0usize;
        assert_eq!(sys_defs[idx].title, "Clock");
        idx += 1;
        #[cfg(feature = "uart")]
        {
            assert_eq!(sys_defs[idx].title, "UART Baud");
            idx += 1;
        }
        #[cfg(feature = "settings_info")]
        {
            assert_eq!(sys_defs[idx].title, "Info");
            assert_eq!(PageId::from_u8(sys_defs[idx].aux), Some(PageId::SystemInfo));
            idx += 1;
        }
        #[cfg(feature = "filesystem")]
        {
            assert_eq!(sys_defs[idx].title, "Storage");
            assert_eq!(
                PageId::from_u8(sys_defs[idx].aux),
                Some(PageId::SystemStorage)
            );
            idx += 1;
        }
        #[cfg(feature = "settings_calib")]
        {
            assert_eq!(sys_defs[idx].title, "Calibration");
            idx += 1;
        }
        #[cfg(feature = "settings_debug")]
        {
            assert_eq!(sys_defs[idx].title, "Debug");
            idx += 1;
        }
        assert_eq!(sys_defs[idx].title, "Reset Settings");
        assert_eq!(
            ActionId::from_u8(sys_defs[idx].aux),
            Some(ActionId::ResetSettings)
        );
        idx += 1;
        assert_eq!(sys_defs[idx].title, "Restart");
        assert_eq!(
            ActionId::from_u8(sys_defs[idx].aux),
            Some(ActionId::RebootFirmware)
        );
        assert_eq!(sys_defs.len(), idx + 1);

        #[cfg(feature = "settings_info")]
        {
            assert_eq!(
                super::super::options::page_title(PageId::SystemInfo),
                "System Info"
            );
            assert!(page_defs(PageId::SystemInfo).is_empty());

            let qs1 = bsp::uid::factory_cert_status();
            let val_w = gfx::text::str_width(qs1.as_str(), &gfx::fonts::tomthumb::FONT);
            let key_w = gfx::text::str_width("QS-1", &gfx::fonts::tomthumb::FONT);
            assert!((122 - val_w) - (2 + key_w) >= 8);

            // Verify all wrapped FCC notice lines fit between X=4 and scrollbar gap X=123
            for line in super::super::system::FCC_NOTICE.split('\0') {
                assert!(
                    gfx::text::str_width(line, &gfx::fonts::tomthumb::FONT) <= 119,
                    "FCC line too wide: {line}"
                );
            }
        }

        // Verify clock option count and config sanitization match overclock_72mhz gate.
        let clk_opts =
            gfx::gui::modules::menu::SheetOptions::Packed(super::super::system::CPU_CLOCK_OPTIONS);
        let mut cfg = crate::config::MasterConfig::DEFAULT;
        cfg.system.cpu_clock = 4;
        cfg.sanitize();
        if cfg!(feature = "overclock_72mhz") {
            assert_eq!(clk_opts.len(), 5);
            assert_eq!(cfg.system.cpu_clock, 4);
        } else {
            assert_eq!(clk_opts.len(), 4);
            assert_eq!(cfg.system.cpu_clock, 3);
        }

        // Verify destructive levels and non-None recovery classifications.
        use super::super::page::DestructiveLevel;
        assert_eq!(
            ActionId::ResetSettings.destructive_level(),
            DestructiveLevel::UserDataLoss
        );
        #[cfg(feature = "filesystem")]
        {
            assert_eq!(ActionId::from_u8(12), Some(ActionId::DeletePackage));
            assert_eq!(
                ActionId::DeletePackage.destructive_level(),
                DestructiveLevel::UserDataLoss
            );
        }
        assert_eq!(
            ActionId::RebootFirmware.destructive_level(),
            DestructiveLevel::Reboot
        );
    }

    #[cfg(feature = "krx_native")]
    #[test]
    fn system_module_gate_requires_valid_module_flag_and_supported_caps() {
        use crate::apps::launcher::catalog::PackageCatalogEntry;
        use kernel::caps::{Cap, CapabilitySet};
        use kernel::proc::krx::{KrxClass, KrxExecType, SectionType};

        let mut path = heapless::String::new();
        let _ = path.push_str("/apps/sysinfo.mod");
        let e = PackageCatalogEntry {
            title: heapless::String::new(),
            path,
            size_bytes: 4238,
            app_class: KrxClass::Native,
            exec_type: KrxExecType::Module,
            lifecycle_flags: SectionType::FLAG_SYSTEM_MENU,
            icon_id: 1,
            caps: (Cap::DisplayDirect as u32) | (Cap::Keypad as u32) | (Cap::FactoryCalib as u32),
            is_valid: true,
        };

        let full_caps = CapabilitySet::empty()
            .with(Cap::DisplayDirect)
            .with(Cap::Keypad)
            .with(Cap::FactoryCalib);
        assert!(e.is_system_module(full_caps));

        // Missing FLAG_SYSTEM_MENU must hide the menu button.
        let mut no_flag = e.clone();
        no_flag.lifecycle_flags = 0;
        assert!(!no_flag.is_system_module(full_caps));

        // Corrupt / failed CRC header must hide the menu button.
        let mut invalid = e.clone();
        invalid.is_valid = false;
        assert!(!invalid.is_system_module(full_caps));

        // Standalone App instead of Module must hide the menu button.
        let mut as_app = e.clone();
        as_app.exec_type = KrxExecType::App;
        assert!(!as_app.is_system_module(full_caps));

        // Requesting ungranted capability must hide the menu button.
        let limited_caps = CapabilitySet::empty().with(Cap::DisplayDirect);
        assert!(!e.is_system_module(limited_caps));
    }

    #[test]
    fn settings_app_preserves_page_and_focus_when_resuming_from_child_module() {
        let mut raw = core::mem::MaybeUninit::<SettingsApp>::uninit();
        unsafe { SettingsApp::init_at(raw.as_mut_ptr()) };
        let app = unsafe { raw.assume_init_mut() };

        app.current_page = PageId::System;
        app.stack_top = 1;
        app.selected_idx = 4;
        app.title = "System";
        app.resume_nav = true;

        app.begin_start();
        assert_eq!(app.current_page, PageId::System);
        assert_eq!(app.stack_top, 1);
        assert_eq!(app.selected_idx, 4);
        assert_eq!(app.title, "System");
        assert!(!app.resume_nav);

        // Next cold entry (without resume_nav) resets to Root.
        app.begin_start();
        assert_eq!(app.current_page, PageId::Root);
        assert_eq!(app.stack_top, 0);
        assert_eq!(app.selected_idx, 0);
    }

    #[test]
    fn package_icons_honors_krx_icon_id_and_falls_back_cleanly() {
        assert_eq!(
            super::super::apps::package_icons("Info", 30),
            (gfx::icons::ICON_APP_ABOUT, &gfx::icons::ICON_16_INFO)
        );
        assert_eq!(
            super::super::apps::package_icons("Bench", 1),
            (gfx::icons::ICON_APP_DIAG, &gfx::icons::ICON_16_DIAGNOSTICS)
        );
        assert_eq!(
            super::super::apps::package_icons("DFU Update", 49).0,
            gfx::icons::ICON_UPLOAD
        );
    }
}
