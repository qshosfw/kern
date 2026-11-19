use crate::SysContext;
use crate::apps::{APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use gfx::Canvas;
use gfx::Color;
use gfx::gui::modules::grid_menu::{GridItem, GridMenuModel, GridMenuView};
use gfx::gui::view::View;
use kernel::app::{App, EventResult};
use kernel::event::{Event, KeyCode, KeyState};

pub mod catalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherTarget {
    App(u8),
    Shortcut(Shortcut),
    About,
    Package(u8),
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

#[inline(always)]
fn launcher_entries_mut() -> &'static mut [LauncherEntry; MAX_LAUNCHER_ENTRIES] {
    const _: () = {
        assert!(
            core::mem::size_of::<[LauncherEntry; MAX_LAUNCHER_ENTRIES]>()
                <= crate::data::arena::TRANSIENT_SCRATCH_OFFSET
        );
        assert!(
            core::mem::align_of::<LauncherEntry>()
                <= core::mem::align_of::<crate::data::arena::AlignedArena>()
        );
    };
    unsafe {
        &mut *(crate::data::arena::arena_base_ptr() as *mut [LauncherEntry; MAX_LAUNCHER_ENTRIES])
    }
}

static BUILTIN_HEAD: [LauncherEntry; 2] = [
    LauncherEntry {
        title: "Radio",
        icon_8: gfx::icons::ICON_APP_RADIO,
        icon_16: gfx::icons::ICON_16_RADIO,
        target: LauncherTarget::App(APP_ID_VFO as u8),
    },
    LauncherEntry {
        title: "Settings",
        icon_8: gfx::icons::ICON_APP_SETTINGS,
        icon_16: gfx::icons::ICON_16_SETTINGS,
        target: LauncherTarget::App(APP_ID_SETTINGS as u8),
    },
];

static BUILTIN_TAIL: [LauncherEntry; 2] = [
    LauncherEntry {
        title: "Torch",
        icon_8: gfx::icons::ICON_APP_TORCH,
        icon_16: gfx::icons::ICON_16_FLASHLIGHT,
        target: LauncherTarget::Shortcut(Shortcut::Flashlight),
    },
    LauncherEntry {
        title: "About",
        icon_8: gfx::icons::ICON_APP_ABOUT,
        icon_16: gfx::icons::ICON_16_INFO,
        target: LauncherTarget::About,
    },
];

pub fn load_entries(
    _storage: Option<&mut crate::services::SystemStorage>,
) -> &'static [LauncherEntry] {
    #[cfg(feature = "filesystem")]
    {
        let cat = catalog::get_catalog();
        if let Some(s) = _storage {
            cat.scan(s);
        }
    }

    unsafe {
        let entries = launcher_entries_mut();
        entries[..2].copy_from_slice(&BUILTIN_HEAD);
        let mut count = 2usize;

        // 2. Discovered Dynamic Packages from LittleFS /apps/ (no hardcoded stubs!)
        let cat_ref: &'static catalog::PackageCatalog =
            &*core::ptr::addr_of!(catalog::PACKAGE_CATALOG);
        for (pkg_idx, app) in cat_ref.entries.iter().enumerate() {
            if !app.is_valid
                || app.exec_type != kernel::proc::krx::KrxExecType::App
                || count >= (MAX_LAUNCHER_ENTRIES - 2)
            {
                continue;
            }

            let (icon_8, icon_16) =
                crate::apps::settings::apps::package_icons(app.title.as_str(), app.icon_id);
            entries[count] = LauncherEntry {
                title: app.title.as_str(),
                icon_8,
                icon_16,
                target: LauncherTarget::Package(pkg_idx as u8),
            };
            count += 1;
        }

        // 3. Built-In Utilities & Information
        entries[count..count + 2].copy_from_slice(&BUILTIN_TAIL);
        count += 2;

        &entries[..count]
    }
}

/// Mark package catalog dirty and reload in-memory Launcher entries.
pub fn reload_launcher(storage: &mut crate::services::SystemStorage) {
    let cat = catalog::get_catalog();
    cat.mark_dirty();
    let entries = load_entries(Some(storage));
    if let Some(ptr) = crate::apps::AppRegistry::app_instance(APP_ID_LAUNCHER) {
        unsafe {
            (*(ptr as *mut LauncherApp)).set_entries(entries);
        }
    }
}

#[repr(C)]
pub struct LauncherApp {
    is_launching: bool,
    scroll_top: usize,
    scroller: gfx::gui::animation::KineticScroller,
    model: GridMenuModel<LauncherEntry>,
    view: GridMenuView,
}

impl LauncherApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            let entries = load_entries(None);
            (*ptr).model = GridMenuModel::new(entries, "Home");
            (*ptr).view = GridMenuView::new();
            (*ptr).scroller = gfx::gui::animation::KineticScroller::new(2);
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.scroller.is_animating()
    }

    fn set_entries(&mut self, entries: &'static [LauncherEntry]) {
        self.model.items = entries;
        if self.model.selected_idx >= entries.len() {
            self.model.selected_idx = entries.len().saturating_sub(1);
        }
    }

    #[inline(always)]
    fn sync_config(&mut self, ctx: &SysContext) {
        self.model.nav_layout = ctx.config.keys.nav_layout;
        self.model.show_numbers = ctx.config.apps.show_numbers != 0;
        self.model.show_labels = ctx.config.apps.show_labels != 0;
    }

    fn adjust_scroll_for_selection(&mut self, animations_enabled: bool) {
        let prev_scroll = self.scroll_top;
        if self.model.selected_idx < self.scroll_top {
            self.scroll_top = self.model.selected_idx;
        } else if self.model.selected_idx >= self.scroll_top + 4 {
            self.scroll_top = self.model.selected_idx - 3;
        }
        let target_y = (self.scroll_top as i16) * 13;
        if !animations_enabled {
            self.scroller.jump(target_y);
        } else if self.scroll_top != prev_scroll {
            self.scroller.set_target(target_y);
        }
    }

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

        let success = match target {
            LauncherTarget::App(app_id) => {
                ctx.post_event(Shortcut::LaunchApp(app_id).into());
                true
            }
            LauncherTarget::Shortcut(sc) => {
                ctx.post_event(sc.into());
                true
            }
            LauncherTarget::About => {
                ctx.show_popup(0, "About KERN", crate::build_info::ABOUT_TEXT);
                true
            }
            LauncherTarget::Package(_pkg_idx) => {
                #[cfg(feature = "filesystem")]
                {
                    if let Some(pkg) = catalog::get_catalog().entries.get(_pkg_idx as usize) {
                        let path = pkg.path.as_str();
                        let ok = crate::orchestrator::launch_krx_shortcut(ctx, path);
                        if !ok {
                            reload_launcher(ctx.storage);
                        }
                        ok
                    } else {
                        false
                    }
                }
                #[cfg(not(feature = "filesystem"))]
                false
            }
        };

        if !success {
            self.is_launching = false;
            self.model.is_pressed = false;
        }
        success
    }

    #[inline(never)]
    fn handle_list_nav(&mut self, event: &Event, animations_enabled: bool) -> bool {
        let total = self.model.items.len();
        if total == 0 {
            return false;
        }
        let nav_dir = match event {
            Event::Tick => self.view.repeater.tick().and_then(|code| {
                gfx::gui::modules::menu::get_list_nav_dir(self.model.nav_layout, code)
            }),
            Event::Key { code, state } => {
                self.view.repeater.handle_key(*code, *state);
                if *state == KeyState::Pressed {
                    gfx::gui::modules::menu::get_list_nav_dir(self.model.nav_layout, *code)
                } else {
                    return *state == KeyState::Released
                        && gfx::gui::modules::grid_menu::GridMenuView::is_nav_key(*code);
                }
            }
            _ => None,
        };

        if let Some(dir) = nav_dir {
            let prev_sel = self.model.selected_idx;
            let is_tick = matches!(event, Event::Tick);
            let mut animate = animations_enabled;
            match dir {
                gfx::gui::modules::menu::ListNavDir::Down => {
                    if self.model.selected_idx + 1 < total {
                        self.model.selected_idx += 1;
                    } else if !is_tick || total > 4 {
                        self.model.selected_idx = 0;
                        animate = false;
                    }
                }
                gfx::gui::modules::menu::ListNavDir::Up => {
                    if self.model.selected_idx > 0 {
                        self.model.selected_idx -= 1;
                    } else if !is_tick || total > 4 {
                        self.model.selected_idx = total - 1;
                        animate = false;
                    }
                }
            }

            if self.model.selected_idx != prev_sel {
                if !animate && is_tick {
                    self.view.repeater.pause(6);
                }
                self.adjust_scroll_for_selection(animate);
                return true;
            }
        }
        false
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
        let start_row = self.scroll_top;
        let end_row = (self.scroll_top + 5).min(total);

        let is_active_selection = self.model.is_pressed || self.is_launching;

        for idx in start_row..end_row {
            let row_top = 9 + (idx as i16 * row_height) - scroll_y;
            if row_top + row_height <= 8 || row_top >= 64 {
                continue;
            }

            let y = row_top as u8;
            let is_sel = idx == self.model.selected_idx;
            let item = &self.model.items[idx];

            let fg_color = if is_sel {
                gfx::widgets::draw_selection_capsule(
                    canvas,
                    [2, y, 120, 13],
                    if is_active_selection {
                        gfx::widgets::CAPSULE_ROUND
                    } else {
                        gfx::widgets::CAPSULE_FILL_ROUND
                    },
                )
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
                let phase = ((self.model.tick_counter >> 1) & 3) as i16;
                let offset = if phase == 3 { 1 } else { phase };
                let arrow_x = 110 + offset;
                gfx::icons::draw_icon_8(
                    canvas,
                    arrow_x,
                    row_top + 3,
                    gfx::icons::ARROW_RIGHT,
                    fg_color,
                );
            }
        }

        // Clean any row bleed into status bar area (0..8)
        canvas.fill_rect(0, 0, 128, 8, Color::Clear);

        // Minimalist vertical scrollbar with smooth kinetic tracking
        if total > visible_count {
            gfx::widgets::draw_scrollbar(canvas, 125, 9, 52, self.scroll_top, total, visible_count);
        }
    }
}

impl<'a> App<crate::services::SysContext<'a, '_>> for LauncherApp {
    fn on_start(&mut self, ctx: &mut SysContext) {
        self.is_launching = false;
        let entries = load_entries(Some(ctx.storage));
        self.set_entries(entries);
        self.sync_config(ctx);
        self.model.is_pressed = false;
        self.adjust_scroll_for_selection(false);
        crate::orchestrator::compositor::present_next_system_alert(ctx);
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        self.sync_config(ctx);

        let is_list_mode = ctx.config.apps.launcher_style == 1;
        let animations_enabled = ctx.config.display.animations_enabled();

        let mut animating = false;
        if let Event::Tick = event {
            self.model.tick_counter = self.model.tick_counter.wrapping_add(1);
            animating = self.scroller.tick();
        }

        if let Event::Key { code, state } = *event {
            let digit_opt = code.to_digit();
            let is_direct_key = code == KeyCode::Menu || matches!(digit_opt, Some(1..=6));
            match state {
                KeyState::Pressed => {
                    if code == KeyCode::Exit {
                        if self.model.is_pressed {
                            self.model.is_pressed = false;
                            return EventResult::Consumed;
                        }
                        return EventResult::Ignored;
                    }
                    if code == KeyCode::Menu {
                        self.model.is_pressed = true;
                        return EventResult::Consumed;
                    }

                    // Direct numeric keypad 1-touch app / tool launch matching 3x2 grid or list
                    if let Some(digit) = digit_opt {
                        if digit == 0 {
                            self.execute_target(
                                ctx,
                                LauncherTarget::Shortcut(Shortcut::Flashlight),
                            );
                            return EventResult::Consumed;
                        }
                        let idx = (digit - 1) as usize;
                        if idx < self.model.items.len() {
                            self.model.selected_idx = idx;
                            if is_list_mode {
                                self.adjust_scroll_for_selection(animations_enabled);
                            }
                            self.model.is_pressed = true;
                            return EventResult::Consumed;
                        }
                    }
                }
                KeyState::Held => {
                    if is_direct_key {
                        self.model.is_pressed = true;
                        return EventResult::Consumed;
                    }
                }
                KeyState::Released => {
                    if self.model.is_pressed && is_direct_key {
                        self.model.is_pressed = false;
                        if let Some(entry) = self.model.selected_item() {
                            let target = entry.target;
                            self.execute_target(ctx, target);
                            return EventResult::Consumed;
                        }
                    }
                }
            }
        }

        let prev_sel = self.model.selected_idx;
        let nav_handled = if is_list_mode {
            self.handle_list_nav(event, animations_enabled)
        } else {
            self.view.on_event(&mut self.model, event)
        };
        if nav_handled {
            self.model.is_pressed = false;
            if self.model.selected_idx != prev_sel
                && ctx.config.audio.key_beep != 0
                && matches!(event, Event::Tick)
            {
                ctx.play_beep(crate::services::melody::BeepType::Scroll);
            }
            return EventResult::Consumed;
        }

        if animating || (matches!(event, Event::Tick) && animations_enabled) {
            return EventResult::Consumed;
        }

        EventResult::Ignored
    }

    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, ctx: &SysContext) {
        self.render_to_canvas(canvas, ctx.config.apps.launcher_style == 1);
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_entries_and_targets_are_compact() {
        assert!(core::mem::size_of::<LauncherTarget>() <= 8);
        let entries = load_entries(None);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].title(), "Radio");
        assert_eq!(entries[1].title(), "Settings");
    }
}
