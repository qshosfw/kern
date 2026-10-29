use crate::SysContext;
use crate::apps::{APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use gfx::Canvas;
use gfx::Color;
use gfx::gui::modules::grid_menu::{GridItem, GridMenuModel, GridMenuView};
use gfx::gui::view::View;
use kernel::app::{App, AppMetadata, EventResult};
use kernel::event::{Event, KeyCode, KeyState};

pub mod assets;
pub mod catalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherTarget {
    App(usize),
    Shortcut(Shortcut),
    About,
    Package(&'static str),
}

#[derive(Clone, Copy)]
pub struct LauncherEntry {
    pub title: &'static str,
    pub icon_8: &'static [u8; 8],
    pub icon_16: &'static [u8; 32],
    pub target: LauncherTarget,
}

impl GridItem for LauncherEntry {
    #[inline(always)]
    fn title(&self) -> &'static str {
        self.title
    }
    #[inline(always)]
    fn icon(&self) -> &'static [u8; 32] {
        self.icon_16
    }
}

pub const MAX_LAUNCHER_ENTRIES: usize = 12;

#[unsafe(link_section = ".uninit")]
static mut ENTRIES: [LauncherEntry; MAX_LAUNCHER_ENTRIES] = [LauncherEntry {
    title: "",
    icon_8: &crate::apps::vfo::assets::ICON_8,
    icon_16: &crate::apps::vfo::assets::ICON_16,
    target: LauncherTarget::App(APP_ID_VFO),
}; MAX_LAUNCHER_ENTRIES];

pub fn load_entries(
    storage: Option<&mut crate::services::SystemStorage>,
) -> &'static [LauncherEntry] {
    let cat = catalog::get_catalog();
    #[cfg(feature = "filesystem")]
    if let Some(s) = storage {
        cat.scan(s);
    }

    unsafe {
        let mut count = 0usize;

        // 1. Core Built-In Apps
        ENTRIES[count] = LauncherEntry {
            title: "Radio",
            icon_8: &crate::apps::vfo::assets::ICON_8,
            icon_16: &crate::apps::vfo::assets::ICON_16,
            target: LauncherTarget::App(APP_ID_VFO),
        };
        count += 1;

        ENTRIES[count] = LauncherEntry {
            title: "Settings",
            icon_8: &crate::apps::settings::assets::ICON_8,
            icon_16: &crate::apps::settings::assets::ICON_16,
            target: LauncherTarget::App(APP_ID_SETTINGS),
        };
        count += 1;

        // 2. Discovered Dynamic Packages from LittleFS /apps/ (no hardcoded stubs!)
        let cat_ref: &'static catalog::PackageCatalog =
            &*core::ptr::addr_of!(catalog::PACKAGE_CATALOG);
        for app in cat_ref.apps() {
            if !app.is_valid || count >= (MAX_LAUNCHER_ENTRIES - 2) {
                continue;
            }

            ENTRIES[count] = LauncherEntry {
                title: app.title.as_str(),
                icon_8: &assets::ICON_DIAG_8,
                icon_16: &assets::ICON_DIAG_16,
                target: LauncherTarget::Package(app.path.as_str()),
            };
            count += 1;
        }

        // 3. Built-In Utilities & Information
        ENTRIES[count] = LauncherEntry {
            title: "Torch",
            icon_8: &assets::ICON_TORCH_8,
            icon_16: &assets::ICON_TORCH_16,
            target: LauncherTarget::Shortcut(Shortcut::Flashlight),
        };
        count += 1;

        ENTRIES[count] = LauncherEntry {
            title: "About",
            icon_8: &assets::ICON_ABOUT_8,
            icon_16: &assets::ICON_ABOUT_16,
            target: LauncherTarget::About,
        };
        count += 1;

        &ENTRIES[..count]
    }
}

/// Mark package catalog dirty and reload in-memory Launcher entries.
pub fn reload_launcher(storage: &mut crate::services::SystemStorage) {
    let cat = catalog::get_catalog();
    cat.mark_dirty();
    let entries = load_entries(Some(storage));
    if let Some(ptr) = crate::apps::AppRegistry::app_instance(APP_ID_LAUNCHER) {
        unsafe {
            let app = &mut *(ptr as *mut LauncherApp);
            app.model.items = entries;
            if app.model.selected_idx >= entries.len() {
                app.model.selected_idx = entries.len().saturating_sub(1);
            }
        }
    }
}

pub struct LauncherApp {
    model: GridMenuModel<LauncherEntry>,
    view: GridMenuView,
    scroll_top: usize,
    scroller: gfx::gui::animation::KineticScroller,
    is_pressed: bool,
    is_launching: bool,
}

impl LauncherApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            let entries = load_entries(None);
            (*ptr).model = GridMenuModel::new(entries, "Home");
            (*ptr).view = GridMenuView::new();
            (*ptr).scroll_top = 0;
            (*ptr).scroller = gfx::gui::animation::KineticScroller::new(2);
            (*ptr).is_pressed = false;
            (*ptr).is_launching = false;
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.scroller.is_animating()
    }

    fn adjust_scroll_for_selection(&mut self, animations_enabled: bool) {
        let prev_scroll = self.scroll_top;
        if self.model.selected_idx < self.scroll_top {
            self.scroll_top = self.model.selected_idx;
        } else if self.model.selected_idx >= self.scroll_top + 4 {
            self.scroll_top = self.model.selected_idx - 3;
        }
        if self.scroll_top != prev_scroll {
            if animations_enabled {
                self.scroller.set_target((self.scroll_top as i16) * 13);
            } else {
                self.scroller.jump((self.scroll_top as i16) * 13);
            }
        }
    }

    #[inline(never)]
    fn render_to_canvas(&self, canvas: &mut Canvas, is_list_mode: bool) {
        if is_list_mode {
            self.render_list(canvas);
        } else {
            self.view.render(canvas, &self.model);
        }
    }

    #[inline(never)]
    fn execute_target(&mut self, ctx: &mut SysContext, target: LauncherTarget) -> bool {
        self.is_launching = true;
        self.model.is_pressed = true;

        let is_list = ctx.config.apps.launcher_style == 1;

        // Immediately update physical LCD so the outline border visibly persists during launch
        let sb_cfg = self.status_bar_config();
        let mut disp = ctx.display.take();
        if let Some(ref mut d) = disp {
            self.render_to_canvas(d.canvas, is_list);
            ctx.draw_status_bar_cfg(d.canvas, &sb_cfg);
            d.lcd.blit(d.canvas);
        }
        ctx.display = disp;

        let success = match target {
            LauncherTarget::App(app_id) => {
                ctx.post_event(Shortcut::LaunchApp(app_id as u8).into());
                true
            }
            LauncherTarget::Shortcut(sc) => {
                ctx.post_event(sc.into());
                true
            }
            LauncherTarget::About => {
                ctx.show_popup_str(0, "About KERN", crate::build_info::ABOUT_TEXT);
                true
            }
            LauncherTarget::Package(path) => {
                #[cfg(feature = "filesystem")]
                {
                    match crate::apps::runtime::launch_krx_package(ctx, path) {
                        Ok(_) => true,
                        Err(err) => {
                            ctx.show_confirm(0, err.title(), err.message(), "OK", "EXIT");
                            ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
                            reload_launcher(ctx.storage);
                            false
                        }
                    }
                }
                #[cfg(not(feature = "filesystem"))]
                false
            }
        };

        if !success {
            self.is_launching = false;
            self.is_pressed = false;
            self.model.is_pressed = false;
            let is_list = ctx.config.apps.launcher_style == 1;
            let sb_cfg = self.status_bar_config();
            let mut disp = ctx.display.take();
            if let Some(ref mut d) = disp {
                self.render_to_canvas(d.canvas, is_list);
                ctx.draw_status_bar_cfg(d.canvas, &sb_cfg);
                unsafe {
                    let modal_ref =
                        &*core::ptr::addr_of!(crate::orchestrator::compositor::ACTIVE_MODAL);
                    if modal_ref.is_active() {
                        modal_ref.render_overlay(d.canvas);
                    }
                }
                d.lcd.blit(d.canvas);
            }
            ctx.display = disp;
        }
        success
    }
}

impl LauncherApp {
    #[inline(never)]
    fn render_list(&self, canvas: &mut Canvas) {
        let total = self.model.items.len();
        if total == 0 {
            return;
        }

        let visible_count = 4;
        let row_height = 13i16;
        let scroll_y = self.scroller.current();
        let start_row = (scroll_y / row_height).max(0) as usize;
        let end_row = (((scroll_y + 52 + 12) / row_height) as usize).min(total);

        let is_active_selection = self.is_pressed || self.is_launching;

        for idx in start_row..end_row {
            let row_top = 9 + (idx as i16 * row_height) - scroll_y;
            if row_top + row_height <= 8 || row_top >= 64 {
                continue;
            }

            let y = row_top as u8;
            let is_sel = idx == self.model.selected_idx;
            let item = &self.model.items[idx];

            let fg_color = if is_sel {
                if is_active_selection {
                    canvas.draw_rect(2, y, 120, 13, Color::Fill);
                    canvas.clear_corners(2, y, 120, 13);
                    Color::Fill
                } else {
                    canvas.fill_rect(2, y, 120, 13, Color::Fill);
                    canvas.clear_corners(2, y, 120, 13);
                    Color::Clear
                }
            } else {
                Color::Fill
            };

            let mut cur_x: i16 = if self.model.show_numbers { 6 } else { 7 };

            // Numeric 1-touch index badge
            if self.model.show_numbers && idx < 9 {
                let badge_char = (b'1' + idx as u8) as char;
                gfx::text::draw_char(
                    canvas,
                    cur_x,
                    row_top + 9,
                    badge_char,
                    &gfx::fonts::mumatrix_regular::FONT,
                    fg_color,
                );
                cur_x = 16;
            }

            // 8x8 icon
            gfx::icons::draw_icon_8(canvas, cur_x, row_top + 2, item.icon_8, fg_color);
            cur_x += 14;

            // Title in MuMatrix
            if self.model.show_labels {
                gfx::text::draw_str(
                    canvas,
                    cur_x,
                    row_top + 9,
                    item.title,
                    &gfx::fonts::mumatrix_regular::FONT,
                    fg_color,
                );
            }

            // Right arrow on selected row (vertically centered at y+3, oscillating horizontal animation)
            if is_sel {
                let offset = match (self.model.tick_counter >> 1) % 6 {
                    0 => 0,
                    1 => 1,
                    2 => 2,
                    3 => 3,
                    4 => 2,
                    _ => 1,
                };
                let arrow_x = 110 + offset;
                gfx::icons::draw_icon_8(
                    canvas,
                    arrow_x,
                    row_top + 3,
                    &gfx::icons::ARROW_RIGHT,
                    fg_color,
                );
            }
        }

        // Clean any row bleed into status bar area (0..8)
        canvas.fill_rect(0, 0, 128, 8, Color::Clear);

        // Minimalist vertical scrollbar with smooth kinetic tracking
        if total > visible_count {
            gfx::widgets::draw_scrollbar(
                canvas,
                125,
                9,
                52,
                (self.scroller.current().max(0) as usize) / (row_height as usize),
                total,
                visible_count,
            );
        }
    }
}

impl<'a> App<crate::services::FirmwareContext<'a, '_>> for LauncherApp {
    fn meta() -> AppMetadata {
        AppMetadata {
            name: "Home",
            icon: &[0; 32],
            icon_8: &[0; 8],
        }
    }

    fn on_start(&mut self, ctx: &mut SysContext) {
        self.is_pressed = false;
        self.is_launching = false;
        let entries = load_entries(Some(ctx.storage));
        self.model.items = entries;
        if self.model.selected_idx >= entries.len() {
            self.model.selected_idx = entries.len().saturating_sub(1);
        }
        self.model.nav_layout = ctx.config.keys.nav_layout;
        self.model.show_numbers = ctx.config.apps.show_numbers != 0;
        self.model.show_labels = ctx.config.apps.show_labels != 0;
        self.model.is_pressed = false;
        self.adjust_scroll_for_selection(false);
        self.scroller.jump((self.scroll_top as i16) * 13);
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        self.model.nav_layout = ctx.config.keys.nav_layout;
        self.model.show_numbers = ctx.config.apps.show_numbers != 0;
        self.model.show_labels = ctx.config.apps.show_labels != 0;

        let is_list_mode = ctx.config.apps.launcher_style == 1;

        let mut animating = false;
        if let Event::Tick = event {
            self.model.tick_counter = self.model.tick_counter.wrapping_add(1);
            animating = self.scroller.tick();
        }

        if let Event::Key { code, state } = *event {
            match state {
                KeyState::Pressed => {
                    if code == KeyCode::Exit {
                        if self.is_pressed {
                            self.is_pressed = false;
                            self.model.is_pressed = false;
                            return EventResult::Consumed;
                        }
                        ctx.post_event(Shortcut::LaunchApp(APP_ID_VFO as u8).into());
                        return EventResult::Consumed;
                    }
                    if code == KeyCode::Menu {
                        self.is_pressed = true;
                        self.model.is_pressed = true;
                        return EventResult::Consumed;
                    }

                    // Direct numeric keypad 1-touch app / tool launch matching 3x2 grid or list
                    let direct_idx = match code {
                        KeyCode::Key1 => Some(0),
                        KeyCode::Key2 => Some(1),
                        KeyCode::Key3 => Some(2),
                        KeyCode::Key4 => Some(3),
                        KeyCode::Key5 => Some(4),
                        KeyCode::Key6 => Some(5),
                        KeyCode::Key0 => {
                            self.execute_target(
                                ctx,
                                LauncherTarget::Shortcut(Shortcut::Flashlight),
                            );
                            return EventResult::Consumed;
                        }
                        _ => None,
                    };
                    if let Some(idx) = direct_idx {
                        if idx < self.model.items.len() {
                            self.model.selected_idx = idx;
                            if is_list_mode {
                                self.adjust_scroll_for_selection(
                                    ctx.config.display.animations_enabled(),
                                );
                            }
                            self.is_pressed = true;
                            self.model.is_pressed = true;
                            return EventResult::Consumed;
                        }
                    }
                }
                KeyState::Held => {
                    if matches!(
                        code,
                        KeyCode::Menu
                            | KeyCode::Key1
                            | KeyCode::Key2
                            | KeyCode::Key3
                            | KeyCode::Key4
                            | KeyCode::Key5
                            | KeyCode::Key6
                    ) {
                        self.is_pressed = true;
                        self.model.is_pressed = true;
                        return EventResult::Consumed;
                    }
                }
                KeyState::Released => {
                    if self.is_pressed
                        && matches!(
                            code,
                            KeyCode::Menu
                                | KeyCode::Key1
                                | KeyCode::Key2
                                | KeyCode::Key3
                                | KeyCode::Key4
                                | KeyCode::Key5
                                | KeyCode::Key6
                        )
                    {
                        self.is_pressed = false;
                        if let Some(entry) = self.model.selected_item() {
                            let target = entry.target;
                            self.execute_target(ctx, target);
                            return EventResult::Consumed;
                        }
                    }
                }
            }
        }

        if is_list_mode {
            let total = self.model.items.len();
            if total > 0 {
                let nav_dir = match event {
                    Event::Tick => self.view.repeater.tick().and_then(|code| {
                        gfx::gui::modules::menu::get_list_nav_dir(self.model.nav_layout, code)
                    }),
                    Event::Key { code, state } => {
                        self.view.repeater.handle_key(*code, *state);
                        if *state == KeyState::Pressed {
                            gfx::gui::modules::menu::get_list_nav_dir(self.model.nav_layout, *code)
                        } else if *state == KeyState::Released
                            && gfx::gui::modules::grid_menu::GridMenuView::is_nav_key(*code)
                        {
                            return EventResult::Consumed;
                        } else {
                            None
                        }
                    }
                    _ => None,
                };

                if let Some(dir) = nav_dir {
                    self.is_pressed = false;
                    self.model.is_pressed = false;
                    let prev_sel = self.model.selected_idx;
                    let is_tick = matches!(event, Event::Tick);
                    match dir {
                        gfx::gui::modules::menu::ListNavDir::Down => {
                            if self.model.selected_idx + 1 < total {
                                self.model.selected_idx += 1;
                            } else if !is_tick || total > 4 {
                                self.model.selected_idx = 0;
                                self.scroll_top = 0;
                                self.scroller.jump(0);
                                if is_tick {
                                    self.view.repeater.pause(6);
                                }
                            }
                        }
                        gfx::gui::modules::menu::ListNavDir::Up => {
                            if self.model.selected_idx > 0 {
                                self.model.selected_idx -= 1;
                            } else if !is_tick || total > 4 {
                                self.model.selected_idx = total - 1;
                                self.scroll_top = self.model.selected_idx.saturating_sub(3);
                                self.scroller.jump((self.scroll_top as i16) * 13);
                                if is_tick {
                                    self.view.repeater.pause(6);
                                }
                            }
                        }
                    }

                    if self.model.selected_idx != prev_sel {
                        if ctx.config.audio.key_beep != 0 && matches!(event, Event::Tick) {
                            ctx.play_beep(crate::services::melody::BeepType::Scroll);
                        }
                        self.adjust_scroll_for_selection(ctx.config.display.animations_enabled());
                        return EventResult::Consumed;
                    }
                }
            }
        } else {
            let prev_sel = self.model.selected_idx;
            if self.view.on_event(&mut self.model, &event) {
                self.is_pressed = false;
                self.model.is_pressed = false;
                if self.model.selected_idx != prev_sel
                    && ctx.config.audio.key_beep != 0
                    && matches!(event, Event::Tick)
                {
                    ctx.play_beep(crate::services::melody::BeepType::Scroll);
                }
                return EventResult::Consumed;
            }
        }

        if animating {
            return EventResult::Consumed;
        }

        // Keep animated arrow oscillating smoothly at 20 Hz when animations are enabled
        if matches!(event, Event::Tick) && ctx.config.display.animations_enabled() {
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    fn status_bar_config(&self) -> kernel::app::StatusBarConfig<'_> {
        kernel::app::StatusBarConfig::new("Home")
    }

    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, ctx: &SysContext) {
        self.render_to_canvas(canvas, ctx.config.apps.launcher_style == 1);
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}
