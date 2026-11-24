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
    catalog::get_catalog().entries.get(idx as usize)
}

impl LauncherEntry {
    pub const fn builtin(target: LauncherTarget) -> Self {
        Self { target }
    }

    #[inline(never)]
    pub fn title_and_icon(&self) -> (&'static str, &'static [u8; 8]) {
        match self.target {
            LauncherTarget::App(id) if id == APP_ID_VFO as u8 => ("Radio", gfx::icons::ICON_APP_RADIO),
            LauncherTarget::App(_) => ("Settings", gfx::icons::ICON_APP_SETTINGS),
            LauncherTarget::Shortcut(_) => ("Torch", gfx::icons::ICON_APP_TORCH),
            LauncherTarget::About => ("About", gfx::icons::ICON_APP_ABOUT),
            LauncherTarget::Package(idx) => catalog_entry(idx)
                .map_or(("App", gfx::icons::ICON_FOLDER), |e| {
                    (e.title.as_str(), e.resolved_icon_8())
                }),
        }
    }

    #[cfg(test)]
    pub fn title(&self) -> &'static str {
        self.title_and_icon().0
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

#[inline(never)]
pub fn load_entries(
    mut _storage: Option<&mut crate::services::SystemStorage>,
) -> &'static [LauncherEntry] {
    #[cfg(feature = "filesystem")]
    if let Some(s) = &mut _storage {
        catalog::get_catalog().scan(*s);
    }

    unsafe {
        let entries = &mut *core::ptr::addr_of_mut!(LAUNCHER_ENTRIES);
        entries[..2].copy_from_slice(&BUILTIN_HEAD);
        let mut count = 2usize;

        let cat_ref: &'static catalog::PackageCatalog = catalog::get_catalog();
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
        &entries[..count + 2]
    }
}

/// Mark package catalog dirty and reload in-memory Launcher entries.
pub fn reload_launcher(storage: &mut crate::services::SystemStorage) {
    catalog::get_catalog().mark_dirty();
    let entries = load_entries(Some(storage));
    if let Some(ptr) = crate::apps::AppRegistry::app_instance(APP_ID_LAUNCHER) {
        unsafe { (*(ptr as *mut LauncherApp)).set_entries(entries) };
    }
}

/// Lean resident failsafe list launcher (`APP_ID_LAUNCHER`).
///
/// Always available in ROM without external assets; optional custom Home front-ends
/// may be installed as `/apps/home.krx`.
#[repr(C)]
pub struct LauncherApp {
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
        if (self.selected_idx as usize) >= entries.len() {
            self.selected_idx = entries.len().saturating_sub(1) as u8;
        }
    }

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

    #[inline(never)]
    fn adjust_scroll(&mut self, animated: bool) {
        let prev = self.scroll_top;
        if self.selected_idx < self.scroll_top {
            self.scroll_top = self.selected_idx;
        } else if self.selected_idx >= self.scroll_top + 4 {
            self.scroll_top = self.selected_idx - 3;
        }
        let target_y = (self.scroll_top as i16) * 13;
        if !animated {
            self.scroller.jump(target_y);
        } else if self.scroll_top != prev {
            self.scroller.set_target(target_y);
        }
    }

    #[inline(never)]
    fn step_selection(&mut self, ctx: &mut SysContext, down: bool, is_tick: bool, beep: bool) -> bool {
        let total = self.items.len();
        if total == 0 {
            return false;
        }
        let prev = self.selected_idx as usize;
        let mut animate = ctx.config.display.animations_enabled();
        let next = if down {
            if prev + 1 < total {
                prev + 1
            } else if !is_tick || total > 4 {
                animate = false;
                0
            } else {
                prev
            }
        } else if prev > 0 {
            prev - 1
        } else if !is_tick || total > 4 {
            animate = false;
            total - 1
        } else {
            prev
        };
        if next == prev {
            return false;
        }
        self.selected_idx = next as u8;
        self.is_pressed = false;
        if !animate && is_tick {
            self.repeater.pause(6);
        }
        self.adjust_scroll(animate);
        if beep && ctx.config.audio.key_beep != 0 {
            ctx.play_beep(crate::services::melody::BeepType::Scroll);
        }
        true
    }

    #[inline(never)]
    fn execute_target(&mut self, ctx: &mut SysContext, target: LauncherTarget) {
        self.is_pressed = false;
        match target {
            LauncherTarget::App(app_id) => {
                let _ = ctx.post_event(Shortcut::LaunchApp(app_id).into());
            }
            LauncherTarget::Shortcut(sc) => {
                let _ = ctx.post_event(sc.into());
            }
            LauncherTarget::About => {
                ctx.show_popup(0, "About KERN", crate::build_info::ABOUT_TEXT);
            }
            LauncherTarget::Package(_pkg_idx) => {
                #[cfg(feature = "filesystem")]
                if let Some(pkg) = catalog::get_catalog().entries.get(_pkg_idx as usize) {
                    if !crate::orchestrator::launch_krx_shortcut(ctx, pkg.path.as_str()) {
                        let entries = load_entries(Some(ctx.storage));
                        self.set_entries(entries);
                    }
                }
            }
        }
    }
}

impl<'a> App<crate::services::SysContext<'a, '_>, Canvas> for LauncherApp {
    fn on_start(&mut self, ctx: &mut SysContext) {
        let entries = load_entries(Some(ctx.storage));
        self.set_entries(entries);
        self.sync_config(ctx);
        self.is_pressed = false;
        self.adjust_scroll(false);
        crate::orchestrator::compositor::present_next_system_alert(ctx);
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        use gfx::gui::modules::menu::{ListNavDir, get_list_nav_dir};
        let animated = ctx.config.display.animations_enabled();

        match *event {
            Event::Tick => {
                self.tick_counter = self.tick_counter.wrapping_add(1);
                let animating = self.scroller.tick();
                if let Some(code) = self.repeater.tick()
                    && let Some(dir) = get_list_nav_dir(self.nav_layout, code)
                    && self.step_selection(ctx, dir == ListNavDir::Down, true, true)
                {
                    return EventResult::Consumed;
                }
                return if animating || animated {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                };
            }
            Event::Encoder(delta) if delta != 0 => {
                self.step_selection(ctx, delta > 0, false, true);
                return EventResult::Consumed;
            }
            Event::Key { code, state } => {
                self.repeater.handle_key(code, state);
                let digit_opt = code.to_digit();
                let is_direct = code == KeyCode::Menu || matches!(digit_opt, Some(1..=6));
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
                        if let Some(d) = digit_opt {
                            if d == 0 {
                                self.execute_target(
                                    ctx,
                                    LauncherTarget::Shortcut(Shortcut::Flashlight),
                                );
                                return EventResult::Consumed;
                            }
                            let idx = (d - 1) as usize;
                            if idx < self.items.len() {
                                self.selected_idx = idx as u8;
                                self.adjust_scroll(animated);
                                self.is_pressed = true;
                                return EventResult::Consumed;
                            }
                        }
                        if let Some(dir) = get_list_nav_dir(self.nav_layout, code) {
                            self.step_selection(ctx, dir == ListNavDir::Down, false, false);
                            return EventResult::Consumed;
                        }
                    }
                    KeyState::Held if is_direct => {
                        self.is_pressed = true;
                        return EventResult::Consumed;
                    }
                    KeyState::Released => {
                        if self.is_pressed && is_direct {
                            self.is_pressed = false;
                            if let Some(entry) = self.items.get(self.selected_idx as usize) {
                                self.execute_target(ctx, entry.target);
                                return EventResult::Consumed;
                            }
                        }
                        if get_list_nav_dir(self.nav_layout, code).is_some() {
                            return EventResult::Consumed;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        EventResult::Ignored
    }

    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, _ctx: &SysContext) {
        let total = self.items.len();
        if total == 0 {
            return;
        }
        let scroll_y = self.scroller.current();
        let start = self.scroll_top as usize;
        let end = (start + 5).min(total);

        for i in start..end {
            let item = &self.items[i];
            let row_top = 9 + (i as i16 * 13) - scroll_y;
            if row_top <= -5 || row_top >= 64 {
                continue;
            }
            let is_sel = i == self.selected_idx as usize;
            let has_num = self.show_numbers && i < 9;
            let (title, icon) = item.title_and_icon();
            let mut spec = gfx::widgets::ListRowSpec::new(
                2,
                row_top,
                120,
                13,
                if self.show_labels { title } else { "" },
            );
            spec.clip_top = 8;
            spec.prefix_icon = has_num.then_some(&gfx::icons::ICON_NONE);
            spec.icon = Some(icon);
            if is_sel {
                let phase = ((self.tick_counter >> 1) & 3) as i8;
                spec.flags = gfx::widgets::ROW_SELECTED
                    | gfx::widgets::ROW_CHEVRON
                    | if self.is_pressed {
                        gfx::widgets::ROW_OUTLINE
                    } else {
                        0
                    };
                spec.chevron_dx = (if phase == 3 { 1 } else { phase }) - 2;
            }
            gfx::widgets::draw_list_row(canvas, &spec);

            if has_num {
                let d = [b'1' + i as u8];
                let fg = if is_sel && !self.is_pressed {
                    Color::Clear
                } else {
                    Color::Fill
                };
                gfx::text::draw_str(
                    canvas,
                    6,
                    row_top + 9,
                    unsafe { core::str::from_utf8_unchecked(&d) },
                    &gfx::fonts::mumatrix_regular::FONT,
                    fg,
                );
            }
        }

        canvas.fill_rect(0, 0, 128, 8, Color::Clear);
        if total > 4 {
            gfx::widgets::draw_scrollbar(canvas, 125, 9, 52, scroll_y.max(0) as usize, total * 13, 52);
        }
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
            title: utils::format::InlineStr::from_str("Settings"),
            path: utils::format::InlineStr::from_str("/apps/settings.krx"),
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
            title: utils::format::InlineStr::from_str("Radio"),
            path: utils::format::InlineStr::from_str("/apps/vfo.krx"),
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
        app.adjust_scroll(false);
        assert_eq!(app.selected_idx, 3);
        let saved_scroll = app.scroll_top;

        let entries = load_entries(None);
        app.set_entries(entries);
        app.adjust_scroll(false);
        assert_eq!(app.selected_idx, 3);
        assert_eq!(app.scroll_top, saved_scroll);
    }
}
