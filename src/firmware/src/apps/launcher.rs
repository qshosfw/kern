use crate::SysContext;
use crate::apps::{APP_ID_LAUNCHER, APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use gfx::Canvas;
use gfx::Color;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LauncherEntry {
    pub target: LauncherTarget,
}

pub const MAX_LAUNCHER_ENTRIES: usize = 12;

static mut LAUNCHER_ENTRIES: [LauncherEntry; MAX_LAUNCHER_ENTRIES] = [LauncherEntry {
    target: LauncherTarget::About,
}; MAX_LAUNCHER_ENTRIES];

#[inline(always)]
fn catalog_entry(idx: u8) -> Option<&'static catalog::PackageCatalogEntry> {
    let cat: &'static catalog::PackageCatalog =
        unsafe { &*core::ptr::addr_of!(catalog::PACKAGE_CATALOG) };
    cat.entries.get(idx as usize)
}

impl LauncherEntry {
    pub const fn builtin(target: LauncherTarget) -> Self {
        Self { target }
    }

    pub fn title(&self) -> &'static str {
        match self.target {
            LauncherTarget::App(id) if id == APP_ID_VFO as u8 => "Radio",
            LauncherTarget::App(_) => "Settings",
            LauncherTarget::Shortcut(_) => "Torch",
            LauncherTarget::About => "About",
            LauncherTarget::Package(idx) => catalog_entry(idx).map_or("App", |e| e.title.as_str()),
        }
    }

    pub fn icon_8(&self) -> &'static [u8; 8] {
        match self.target {
            LauncherTarget::App(id) if id == APP_ID_VFO as u8 => gfx::icons::ICON_APP_RADIO,
            LauncherTarget::App(_) => gfx::icons::ICON_APP_SETTINGS,
            LauncherTarget::Shortcut(_) => gfx::icons::ICON_APP_TORCH,
            LauncherTarget::About => gfx::icons::ICON_APP_ABOUT,
            LauncherTarget::Package(idx) => {
                catalog_entry(idx).map_or(gfx::icons::ICON_FOLDER, |e| e.resolved_icon_8())
            }
        }
    }
}

static BUILTIN_HEAD: [LauncherEntry; 2] = [
    LauncherEntry::builtin(LauncherTarget::App(APP_ID_VFO as u8)),
    LauncherEntry::builtin(LauncherTarget::App(APP_ID_SETTINGS as u8)),
];

static BUILTIN_TAIL: [LauncherEntry; 2] = [
    LauncherEntry::builtin(LauncherTarget::Shortcut(Shortcut::Flashlight)),
    LauncherEntry::builtin(LauncherTarget::About),
];

pub fn load_entries(
    mut _storage: Option<&mut crate::services::SystemStorage>,
) -> &'static [LauncherEntry] {
    #[cfg(feature = "filesystem")]
    {
        let cat = catalog::get_catalog();
        if let Some(s) = &mut _storage {
            cat.scan(*s);
        }
    }

    unsafe {
        let entries = &mut *core::ptr::addr_of_mut!(LAUNCHER_ENTRIES);
        entries[..2].copy_from_slice(&BUILTIN_HEAD);
        let mut count = 2usize;

        let cat_ref: &'static catalog::PackageCatalog =
            &*core::ptr::addr_of!(catalog::PACKAGE_CATALOG);
        for (pkg_idx, app) in cat_ref.entries.iter().enumerate() {
            if !app.is_valid
                || app.exec_type != kernel::runtime::KrxExecType::App
                || count >= (MAX_LAUNCHER_ENTRIES - 2)
                || utils::format::ascii_eq_ignore_case(app.path.as_str(), "/apps/home.krx")
            {
                continue;
            }

            let entry = LauncherEntry {
                target: LauncherTarget::Package(pkg_idx as u8),
            };
            if utils::format::ascii_eq_ignore_case(app.path.as_str(), "/apps/vfo.krx") {
                entries[0] = entry;
            } else if utils::format::ascii_eq_ignore_case(app.path.as_str(), "/apps/settings.krx") {
                entries[1] = entry;
            } else {
                entries[count] = entry;
                count += 1;
            }
        }

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

/// Lean resident failsafe list launcher (`APP_ID_LAUNCHER`).
///
/// Always available in ROM without external assets; optional custom Home front-ends
/// may be installed as `/apps/home.krx` without bloating core Flash with 16x16 grids.
#[repr(C)]
pub struct LauncherApp {
    is_launching: bool,
    is_pressed: bool,
    show_numbers: bool,
    show_labels: bool,
    nav_layout: u8,
    tick_counter: u8,
    selected_idx: u8,
    scroll_top: u8,
    items: &'static [LauncherEntry],
    scroller: gfx::gui::animation::KineticScroller,
    repeater: gfx::gui::KeyRepeater,
}

impl LauncherApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).items = load_entries(None);
            (*ptr).show_numbers = true;
            (*ptr).show_labels = true;
            (*ptr).scroller = gfx::gui::animation::KineticScroller::new(2);
            (*ptr).repeater = gfx::gui::KeyRepeater::new(10, 2);
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.scroller.is_animating()
    }

    fn set_entries(&mut self, entries: &'static [LauncherEntry]) {
        self.items = entries;
        let len = entries.len();
        if (self.selected_idx as usize) >= len {
            self.selected_idx = len.saturating_sub(1) as u8;
        }
    }

    #[inline(never)]
    fn sync_config(&mut self, ctx: &SysContext) {
        self.nav_layout = ctx.config.keys.nav_layout;
        self.show_numbers = ctx.config.apps.show_numbers != 0;
        self.show_labels = ctx.config.apps.show_labels != 0;
        self.repeater.configure(
            ctx.config.keys.hold_delay_ms(),
            ctx.config.keys.repeat_rate_ms(),
            ctx.config.keys.fast_scroll != 0,
        );
    }

    fn adjust_scroll_for_selection(&mut self, animations_enabled: bool) {
        let prev_scroll = self.scroll_top;
        if self.selected_idx < self.scroll_top {
            self.scroll_top = self.selected_idx;
        } else if self.selected_idx >= self.scroll_top + 4 {
            self.scroll_top = self.selected_idx - 3;
        }
        let target_y = (self.scroll_top as i16) * 13;
        if !animations_enabled {
            self.scroller.jump(target_y);
        } else if self.scroll_top != prev_scroll {
            self.scroller.set_target(target_y);
        }
    }

    #[inline(never)]
    fn execute_target(&mut self, ctx: &mut SysContext, target: LauncherTarget) -> bool {
        self.is_launching = false;
        self.is_pressed = false;
        match target {
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
                        let ok = crate::orchestrator::launch_krx_shortcut(ctx, pkg.path.as_str());
                        self.is_launching = ok;
                        self.is_pressed = ok;
                        if !ok {
                            let entries = load_entries(Some(ctx.storage));
                            self.set_entries(entries);
                        }
                        ok
                    } else {
                        false
                    }
                }
                #[cfg(not(feature = "filesystem"))]
                {
                    false
                }
            }
        }
    }

    #[inline(never)]
    fn handle_list_nav(&mut self, event: &Event, animations_enabled: bool) -> bool {
        let total = self.items.len();
        if total == 0 {
            return false;
        }
        let nav_dir = match event {
            Event::Tick => self.repeater.tick().and_then(|code| {
                gfx::gui::modules::menu::get_list_nav_dir(self.nav_layout, code)
            }),
            Event::Key { code, state } => {
                self.repeater.handle_key(*code, *state);
                if *state == KeyState::Pressed {
                    gfx::gui::modules::menu::get_list_nav_dir(self.nav_layout, *code)
                } else {
                    return *state == KeyState::Released
                        && gfx::gui::modules::menu::get_list_nav_dir(self.nav_layout, *code)
                            .is_some();
                }
            }
            _ => None,
        };

        if let Some(dir) = nav_dir {
            let prev_sel = self.selected_idx;
            let is_tick = matches!(event, Event::Tick);
            let mut animate = animations_enabled;
            let sel = self.selected_idx as usize;
            match dir {
                gfx::gui::modules::menu::ListNavDir::Down => {
                    if sel + 1 < total {
                        self.selected_idx = (sel + 1) as u8;
                    } else if !is_tick || total > 4 {
                        self.selected_idx = 0;
                        animate = false;
                    }
                }
                gfx::gui::modules::menu::ListNavDir::Up => {
                    if sel > 0 {
                        self.selected_idx = (sel - 1) as u8;
                    } else if !is_tick || total > 4 {
                        self.selected_idx = (total - 1) as u8;
                        animate = false;
                    }
                }
            }

            if self.selected_idx != prev_sel {
                if !animate && is_tick {
                    self.repeater.pause(6);
                }
                self.adjust_scroll_for_selection(animate);
                return true;
            }
        }
        false
    }

    #[inline(never)]
    fn render_list(&self, canvas: &mut Canvas) {
        let total = self.items.len();
        if total == 0 {
            return;
        }

        let visible_count = 4usize;
        let row_height = 13i16;
        let scroll_y = self.scroller.current();
        let start_row = self.scroll_top as usize;
        let end_row = (start_row + 5).min(total);
        let is_active_selection = self.is_pressed || self.is_launching;

        for idx in start_row..end_row {
            let row_top = 9 + (idx as i16 * row_height) - scroll_y;
            if row_top + row_height <= 8 || row_top >= 64 {
                continue;
            }

            let is_sel = idx == self.selected_idx as usize;
            let item = &self.items[idx];
            let has_num = self.show_numbers && idx < 9;

            let mut spec = gfx::widgets::ListRowSpec::new(
                2,
                row_top,
                120,
                13,
                if self.show_labels { item.title() } else { "" },
            );
            spec.clip_top = 8;
            spec.prefix_icon = if has_num {
                Some(&gfx::icons::ICON_NONE)
            } else {
                None
            };
            spec.icon = Some(item.icon_8());
            if is_sel {
                let phase = ((self.tick_counter >> 1) & 3) as i8;
                let offset = if phase == 3 { 1 } else { phase };
                spec.flags = gfx::widgets::ROW_SELECTED
                    | gfx::widgets::ROW_CHEVRON
                    | if is_active_selection {
                        gfx::widgets::ROW_OUTLINE
                    } else {
                        0
                    };
                spec.chevron_dx = offset - 2;
            }
            gfx::widgets::draw_list_row(canvas, &spec);

            if has_num {
                let fg_color = if is_sel && !is_active_selection {
                    Color::Clear
                } else {
                    Color::Fill
                };
                gfx::text::draw_char(
                    canvas,
                    6,
                    row_top + 9,
                    (b'1' + idx as u8) as char,
                    &gfx::fonts::mumatrix_regular::FONT,
                    fg_color,
                );
            }
        }

        // Clean any row bleed into status bar area (0..8)
        canvas.fill_rect(0, 0, 128, 8, Color::Clear);

        if total > visible_count {
            gfx::widgets::draw_scrollbar(
                canvas,
                125,
                9,
                52,
                scroll_y.max(0) as usize,
                total * 13,
                visible_count * 13,
            );
        }
    }
}

impl<'a> App<crate::services::SysContext<'a, '_>, Canvas> for LauncherApp {
    fn on_start(&mut self, ctx: &mut SysContext) {
        self.is_launching = false;
        let entries = load_entries(Some(ctx.storage));
        self.set_entries(entries);
        self.sync_config(ctx);
        self.is_pressed = false;
        self.adjust_scroll_for_selection(false);
        crate::orchestrator::compositor::present_next_system_alert(ctx);
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        self.sync_config(ctx);
        let animations_enabled = ctx.config.display.animations_enabled();

        let mut animating = false;
        if let Event::Tick = event {
            self.tick_counter = self.tick_counter.wrapping_add(1);
            animating = self.scroller.tick();
        }

        if let Event::Encoder(delta) = *event {
            if delta != 0 && !self.items.is_empty() {
                let len = self.items.len();
                let prev_sel = self.selected_idx as usize;
                let next_sel = if delta > 0 {
                    if prev_sel + 1 < len { prev_sel + 1 } else { 0 }
                } else if prev_sel > 0 {
                    prev_sel - 1
                } else {
                    len - 1
                };
                self.selected_idx = next_sel as u8;
                self.is_pressed = false;
                self.adjust_scroll_for_selection(animations_enabled);
                if next_sel != prev_sel && ctx.config.audio.key_beep != 0 {
                    ctx.play_beep(crate::services::melody::BeepType::Scroll);
                }
                return EventResult::Consumed;
            }
        }

        if let Event::Key { code, state } = *event {
            let digit_opt = code.to_digit();
            let is_direct_key = code == KeyCode::Menu || matches!(digit_opt, Some(1..=6));
            match state {
                KeyState::Pressed => {
                    if code == KeyCode::Exit {
                        if self.is_pressed {
                            self.is_pressed = false;
                            return EventResult::Consumed;
                        }
                        return EventResult::Ignored;
                    }
                    if code == KeyCode::Menu {
                        self.is_pressed = true;
                        return EventResult::Consumed;
                    }

                    if let Some(digit) = digit_opt {
                        if digit == 0 {
                            self.execute_target(
                                ctx,
                                LauncherTarget::Shortcut(Shortcut::Flashlight),
                            );
                            return EventResult::Consumed;
                        }
                        let idx = (digit - 1) as usize;
                        if idx < self.items.len() {
                            self.selected_idx = idx as u8;
                            self.adjust_scroll_for_selection(animations_enabled);
                            self.is_pressed = true;
                            return EventResult::Consumed;
                        }
                    }
                }
                KeyState::Held => {
                    if is_direct_key {
                        self.is_pressed = true;
                        return EventResult::Consumed;
                    }
                }
                KeyState::Released => {
                    if self.is_pressed && is_direct_key {
                        self.is_pressed = false;
                        if let Some(entry) = self.items.get(self.selected_idx as usize) {
                            let target = entry.target;
                            self.execute_target(ctx, target);
                            return EventResult::Consumed;
                        }
                    }
                }
            }
        }

        let prev_sel = self.selected_idx;
        if self.handle_list_nav(event, animations_enabled) {
            self.is_pressed = false;
            if self.selected_idx != prev_sel
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
    fn render(&self, canvas: &mut Canvas, _ctx: &SysContext) {
        self.render_list(canvas);
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_entries_and_targets_are_compact() {
        assert!(core::mem::size_of::<LauncherTarget>() <= 4);
        assert!(core::mem::size_of::<LauncherEntry>() <= 4);
        catalog::get_catalog().entries.clear();
        let entries = load_entries(None);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].title(), "Radio");
        assert_eq!(entries[1].title(), "Settings");
        assert_eq!(
            entries[1].target,
            LauncherTarget::App(APP_ID_SETTINGS as u8)
        );

        crate::data::arena::transient_arena_bytes_mut()[..1024].fill(0xAA);
        assert_eq!(entries[0].title(), "Radio");
        assert_eq!(entries[1].title(), "Settings");
        assert_eq!(entries[2].title(), "Torch");
        assert_eq!(entries[3].title(), "About");
    }

    #[test]
    fn settings_krx_replaces_builtin_settings_slot_without_duplication() {
        let cat = catalog::get_catalog();
        cat.entries.clear();
        let _ = cat.entries.push(catalog::PackageCatalogEntry {
            exec_type: kernel::runtime::KrxExecType::App,
            is_valid: true,
            icon_id: ::abi::icon::APP_SETTINGS as u16,
            icon_16_off: 0,
            icon_8: *gfx::icons::ICON_APP_SETTINGS,
            title: catalog::SmallStr::from_str("Settings"),
            path: catalog::SmallStr::from_str("/apps/settings.krx"),
        });

        let entries = load_entries(None);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[1].title(), "Settings");
        assert_eq!(entries[1].target, LauncherTarget::Package(0));
        assert_eq!(entries[2].title(), "Torch");
        assert_eq!(entries[3].title(), "About");

        let _ = cat.entries.push(catalog::PackageCatalogEntry {
            exec_type: kernel::runtime::KrxExecType::App,
            is_valid: true,
            icon_id: ::abi::icon::APP_RADIO as u16,
            icon_16_off: 0,
            icon_8: *gfx::icons::ICON_APP_RADIO,
            title: catalog::SmallStr::from_str("Radio"),
            path: catalog::SmallStr::from_str("/apps/vfo.krx"),
        });

        let entries = load_entries(None);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].title(), "Radio");
        assert_eq!(entries[0].target, LauncherTarget::Package(1));
        assert_eq!(entries[1].target, LauncherTarget::Package(0));

        cat.entries.clear();
    }

    #[test]
    fn launcher_preserves_selection_and_scroll_across_exit() {
        let mut app = core::mem::MaybeUninit::<LauncherApp>::zeroed();
        unsafe { LauncherApp::init_at(app.as_mut_ptr()) };
        let app = unsafe { app.assume_init_mut() };
        app.selected_idx = 3;
        app.adjust_scroll_for_selection(false);
        assert_eq!(app.selected_idx, 3);
        let saved_scroll = app.scroll_top;

        // Reloading the same catalog entries must preserve cursor focus and scroll window
        let entries = load_entries(None);
        app.set_entries(entries);
        app.adjust_scroll_for_selection(false);
        assert_eq!(app.selected_idx, 3);
        assert_eq!(app.scroll_top, saved_scroll);
    }
}
