use crate::apps::{APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use gfx::elements::{CAPSULE_FILL_ROUND, CAPSULE_ROUND, draw_selection_capsule};
use gfx::{Canvas, Color};
use kernel::app::{EventResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};

pub mod catalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherTarget {
    About,
    App(u8),
    Shortcut(Shortcut),
    Package(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct LauncherEntry {
    pub target: LauncherTarget,
    pub icon_slot: u8,
}

pub const MAX_LAUNCHER_ENTRIES: usize = 12;
const MAX_CUSTOM_ICONS_16: usize = 8;

static mut LAUNCHER_ENTRIES: [LauncherEntry; MAX_LAUNCHER_ENTRIES] = [LauncherEntry {
    target: LauncherTarget::About,
    icon_slot: 0xFF,
}; MAX_LAUNCHER_ENTRIES];

/// Dedicated `.uninit` cache for installed packages' `16x16` `SECTION_ASSETS` icons (`256 B`),
/// isolated from `TRANSIENT_ARENA` so dialogs, locales, and `.krx` execution never clobber Home artwork.
#[unsafe(link_section = ".uninit")]
static mut LAUNCHER_ICONS_16: [[u8; 32]; MAX_CUSTOM_ICONS_16] = [[0u8; 32]; MAX_CUSTOM_ICONS_16];

#[inline(always)]
fn catalog_entry(idx: u8) -> Option<&'static catalog::PackageCatalogEntry> {
    catalog::get_catalog().entries.get(idx as usize)
}

impl LauncherEntry {
    pub const fn builtin(target: LauncherTarget) -> Self {
        Self {
            target,
            icon_slot: 0xFF,
        }
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

    pub fn icon_16(&self) -> &'static [u8; 32] {
        if (self.icon_slot as usize) < MAX_CUSTOM_ICONS_16 {
            return unsafe { &(&*core::ptr::addr_of!(LAUNCHER_ICONS_16))[self.icon_slot as usize] };
        }
        match self.target {
            LauncherTarget::App(id) if id == APP_ID_VFO as u8 => gfx::icons::ICON_16_RADIO,
            LauncherTarget::App(_) => gfx::icons::ICON_16_SETTINGS,
            LauncherTarget::Shortcut(_) => gfx::icons::ICON_16_FLASHLIGHT,
            LauncherTarget::About => gfx::icons::ICON_16_INFO,
            LauncherTarget::Package(idx) => {
                catalog_entry(idx).map_or(gfx::icons::ICON_16_APPS, |e| e.resolved_icon_16())
            }
        }
    }

    fn is_radio(&self) -> bool {
        match self.target {
            LauncherTarget::App(id) => id == APP_ID_VFO as u8,
            LauncherTarget::Package(idx) => catalog_entry(idx)
                .is_some_and(|e| utils::format::ascii_eq_ignore_case(e.filename(), "vfo.krx")),
            _ => false,
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

pub fn load_entries<D: kernfs::BlockDevice>(
    mut _storage: Option<&mut kernfs::KernFS<'_, D>>,
) -> &'static [LauncherEntry] {
    #[cfg(feature = "filesystem")]
    if let Some(s) = &mut _storage {
        catalog::get_catalog().scan(*s);
    }

    unsafe {
        let entries = &mut *core::ptr::addr_of_mut!(LAUNCHER_ENTRIES);
        entries[..2].copy_from_slice(&BUILTIN_HEAD);
        let mut count = 2usize;
        #[cfg(feature = "filesystem")]
        let icons_16_buf = &mut *core::ptr::addr_of_mut!(LAUNCHER_ICONS_16);
        #[cfg(feature = "filesystem")]
        let mut icon_slot = 0usize;

        let cat_ref: &'static catalog::PackageCatalog = catalog::get_catalog();
        for (pkg_idx, app) in cat_ref.entries.iter().enumerate() {
            let fname = app.filename();
            if !app.is_valid
                || app.exec_type != kernel::runtime::KrxExecType::App
                || count >= (MAX_LAUNCHER_ENTRIES - 2)
                || utils::format::ascii_eq_ignore_case(fname, "home.krx")
            {
                continue;
            }

            let mut loaded_slot = 0xFFu8;
            #[cfg(feature = "filesystem")]
            if app.icon_16_off != 0 && icon_slot < MAX_CUSTOM_ICONS_16 {
                if let Some(s) = &mut _storage {
                    let dst = &mut icons_16_buf[icon_slot];
                    if s.read_file_at(app.path.as_str(), app.icon_16_off as usize, dst) == Ok(32) {
                        loaded_slot = icon_slot as u8;
                        icon_slot += 1;
                    }
                }
            }

            let entry = LauncherEntry {
                target: LauncherTarget::Package(pkg_idx as u8),
                icon_slot: loaded_slot,
            };
            if utils::format::ascii_eq_ignore_case(fname, "vfo.krx") {
                entries[0] = entry;
            } else if utils::format::ascii_eq_ignore_case(fname, "settings.krx") {
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
pub fn reload_launcher<D: kernfs::BlockDevice>(storage: &mut kernfs::KernFS<'_, D>) {
    catalog::get_catalog().mark_dirty();
    let entries = load_entries(Some(storage));
    crate::apps::AppRegistry::launcher_mut().set_entries(entries);
}

static PAGE_BADGES: [&str; 2] = ["1/2", "2/2"];

#[inline(always)]
fn is_torch_on() -> bool {
    crate::hardware::is_torch_on()
}

/// Unified dual-mode (`3x2` `16x16` `Grid` + kinetic `List`) resident Home Launcher (`APP_ID_LAUNCHER`).
///
/// Shares a single navigation, repeater, and selection state across both layouts:
/// - `launcher_style == 0` (default): Crisp `3x2` tile grid with `16x16` icons, page pills, and `1..6` quick-launch.
/// - `launcher_style == 1`: Smooth kinetic list with non-overlapping `[1..9]` key badges, `8x8` icons, and live VFO/Torch status.
/// - `*`: Instant toggle between `Grid` and `List` mode.
/// - `0`: Instant Torch toggle from Home.
#[repr(C)]
pub struct LauncherApp {
    is_pressed: bool,
    is_list_mode: bool,
    show_numbers: bool,
    show_labels: bool,
    nav_layout: u8,
    tick_counter: u8,
    selected_idx: u8,
    scroll_top: u8,
    items: &'static [LauncherEntry],
    scroller: gfx::views::KineticScroller,
    repeater: gfx::views::KeyRepeater,
}

impl LauncherApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).items = &BUILTIN_HEAD;
            (*ptr).show_numbers = true;
            (*ptr).show_labels = true;
            (*ptr).scroller = gfx::views::KineticScroller::new(2);
            (*ptr).repeater = gfx::views::KeyRepeater::new(10, 2);
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.is_list_mode && self.scroller.is_animating()
    }

    fn set_entries(&mut self, entries: &'static [LauncherEntry]) {
        self.items = entries;
        if (self.selected_idx as usize) >= entries.len() {
            self.selected_idx = entries.len().saturating_sub(1) as u8;
        }
    }

    fn sync_config(&mut self) {
        let cfg = crate::services::config();
        self.is_list_mode = cfg.apps.launcher_style != 0;
        self.nav_layout = cfg.keys.nav_layout;
        self.show_numbers = cfg.apps.show_numbers != 0;
        self.show_labels = cfg.apps.show_labels != 0;
        self.repeater.configure(
            cfg.keys.hold_delay_ms(),
            cfg.keys.repeat_rate_ms(),
            cfg.keys.fast_scroll != 0,
        );
    }

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

    fn nav_delta(&self, code: KeyCode) -> Option<i8> {
        let dir = if self.nav_layout == 0 { -1 } else { 1 };
        match code {
            KeyCode::Up => Some(dir),
            KeyCode::Down => Some(-dir),
            KeyCode::Key2 if !self.show_numbers => Some(if self.is_list_mode { -1 } else { -3 }),
            KeyCode::Key8 if !self.show_numbers => Some(if self.is_list_mode { 1 } else { 3 }),
            KeyCode::Key4 if !self.show_numbers && !self.is_list_mode => Some(-1),
            KeyCode::Key6 if !self.show_numbers && !self.is_list_mode => Some(1),
            _ => None,
        }
    }

    fn step_by(&mut self, delta: i8, is_tick: bool, beep: bool) -> bool {
        let len = self.items.len();
        if len == 0 || delta == 0 {
            return false;
        }
        let cur = self.selected_idx as usize;
        let cfg = crate::services::config();
        let mut animate = cfg.display.animations_enabled();
        let next = match delta {
            -1 => {
                if cur > 0 {
                    cur - 1
                } else if !self.is_list_mode || !is_tick || len > 4 {
                    animate = false;
                    len - 1
                } else {
                    cur
                }
            }
            1 => {
                if cur + 1 < len {
                    cur + 1
                } else if !self.is_list_mode || !is_tick || len > 4 {
                    animate = false;
                    0
                } else {
                    cur
                }
            }
            -3 => {
                if cur >= 3 {
                    cur - 3
                } else {
                    (cur + 3).min(len - 1)
                }
            }
            _ => {
                if cur + 3 < len {
                    cur + 3
                } else {
                    let mut v = cur;
                    while v >= 3 {
                        v -= 3;
                    }
                    v
                }
            }
        };
        if next == cur {
            return false;
        }
        self.selected_idx = next as u8;
        self.is_pressed = false;
        if !animate && is_tick {
            self.repeater.pause(6);
        }
        self.adjust_scroll(animate);
        if beep && cfg.audio.key_beep != 0 {
            crate::services::play_beep(crate::services::melody::BeepType::Scroll);
        }
        true
    }

    fn toggle_launcher_style(&mut self) {
        self.is_list_mode = !self.is_list_mode;
        self.is_pressed = false;
        let _ = crate::master_config()
            .prop_set(::abi::prop::LAUNCHER_STYLE, u8::from(self.is_list_mode));
        self.adjust_scroll(false);
        if crate::services::config().audio.key_beep != 0 {
            crate::services::play_beep(crate::services::melody::BeepType::Click);
        }
    }

    fn execute_target(&mut self, target: LauncherTarget) {
        self.is_pressed = false;
        match target {
            LauncherTarget::App(app_id) => {
                let _ = crate::services::post_event(Shortcut::LaunchApp(app_id).into());
            }
            LauncherTarget::Shortcut(sc) => {
                let _ = crate::services::post_event(sc.into());
                if crate::services::config().audio.key_beep != 0 {
                    crate::services::play_beep(crate::services::melody::BeepType::Click);
                }
            }
            LauncherTarget::About => {
                crate::services::show_popup(0, "About KERN", crate::build_info::ABOUT_TEXT);
            }
            LauncherTarget::Package(_pkg_idx) => {
                #[cfg(feature = "filesystem")]
                if let Some(pkg) = catalog::get_catalog().entries.get(_pkg_idx as usize) {
                    if !crate::orchestrator::launch_krx_shortcut(pkg.path.as_str()) {
                        let entries = load_entries(Some(crate::services::storage_mut()));
                        self.set_entries(entries);
                    }
                }
            }
        }
    }

    pub fn on_start(&mut self) {
        let entries = load_entries(Some(crate::services::storage_mut()));
        self.set_entries(entries);
        self.sync_config();
        self.is_pressed = false;
        self.adjust_scroll(false);
        crate::orchestrator::compositor::present_next_system_alert();
    }

    fn resolve_digit_target_idx(&self, d: u8) -> Option<u8> {
        if !self.show_numbers {
            return None;
        }
        let total = self.items.len();
        let idx = if self.is_list_mode {
            (d - 1) as usize
        } else {
            if d > 6 {
                return None;
            }
            let page = utils::math::div3_u16((self.selected_idx >> 1) as u16).0 as usize;
            page * 6 + (d - 1) as usize
        };
        (idx < total).then_some(idx as u8)
    }

    pub fn on_event(&mut self, event: &Event) -> EventResult {
        self.sync_config();
        let animated = crate::services::config().display.animations_enabled();

        match *event {
            Event::Tick => {
                self.tick_counter = self.tick_counter.wrapping_add(1);
                let animating = self.is_list_mode && self.scroller.tick();
                if let Some(code) = self.repeater.tick()
                    && let Some(delta) = self.nav_delta(code)
                    && self.step_by(delta, true, true)
                {
                    return EventResult::Consumed;
                }
                let cadence_mask = if self.is_list_mode { 1 } else { 7 };
                return if animating || (animated && (self.tick_counter & cadence_mask) == 0) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                };
            }
            Event::Encoder(delta) if delta != 0 => {
                self.step_by(if delta > 0 { 1 } else { -1 }, false, true);
                return EventResult::Consumed;
            }
            Event::Key { code, state } => {
                self.repeater.handle_key(code, state);
                let digit_opt = code.to_digit();
                let max_direct = if self.is_list_mode { 9 } else { 6 };
                let is_direct = code == KeyCode::Menu
                    || (self.show_numbers
                        && digit_opt.is_some_and(|d| (1..=max_direct).contains(&d)));

                match state {
                    KeyState::Pressed => {
                        if code == KeyCode::Exit {
                            if self.is_pressed {
                                self.is_pressed = false;
                                return EventResult::Consumed;
                            }
                            return EventResult::Ignored;
                        }
                        if code == KeyCode::Star {
                            self.toggle_launcher_style();
                            return EventResult::Consumed;
                        }
                        if code == KeyCode::Menu {
                            self.is_pressed = true;
                            return EventResult::Consumed;
                        }
                        if let Some(d) = digit_opt {
                            if d == 0 {
                                self.execute_target(LauncherTarget::Shortcut(Shortcut::Flashlight));
                                return EventResult::Consumed;
                            }
                            if let Some(idx) = self.resolve_digit_target_idx(d) {
                                self.selected_idx = idx;
                                self.adjust_scroll(animated);
                                self.is_pressed = true;
                                return EventResult::Consumed;
                            }
                        }
                        if let Some(delta) = self.nav_delta(code) {
                            self.step_by(delta, false, true);
                            return EventResult::Consumed;
                        }
                    }
                    KeyState::Held if self.is_pressed && is_direct => {
                        return EventResult::Consumed;
                    }
                    KeyState::Released => {
                        if self.is_pressed && is_direct {
                            self.is_pressed = false;
                            if let Some(entry) = self.items.get(self.selected_idx as usize) {
                                self.execute_target(entry.target);
                                return EventResult::Consumed;
                            }
                        }
                        if self.nav_delta(code).is_some() {
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
    fn render_grid(&self, canvas: &mut Canvas) {
        use gfx::fonts::tomthumb::FONT as TOMTHUMB;
        let total = self.items.len();
        if total == 0 {
            return;
        }
        let total_pages = (((total as u32 + 5) * 0xAAAB) >> 18) as usize;
        let page = utils::math::div3_u16((self.selected_idx >> 1) as u16).0 as usize;
        let start_idx = page * 6;
        let end_idx = (start_idx + 6).min(total);
        let anim_hop = i16::from(!self.is_pressed && ((self.tick_counter >> 3) & 1) == 1);
        let (base_ty, row_stride, tile_h, label_dy) = if total_pages > 1 {
            (8u8, 26u8, 25u8, 23u8)
        } else {
            (9u8, 27u8, 26u8, 24u8)
        };

        for idx in start_idx..end_idx {
            let page_rel = idx - start_idx;
            let (col_idx, row_idx) = if page_rel >= 3 {
                (page_rel - 3, 1u8)
            } else {
                (page_rel, 0u8)
            };
            let tx = 2u8 + (col_idx as u8) * 42;
            let ty = base_ty + row_idx * row_stride;
            let is_sel = idx == self.selected_idx as usize;
            let item = &self.items[idx];

            let fg = if is_sel {
                draw_selection_capsule(
                    canvas,
                    [tx, ty, 40, tile_h],
                    if self.is_pressed {
                        CAPSULE_ROUND
                    } else {
                        CAPSULE_FILL_ROUND
                    },
                )
            } else {
                Color::Fill
            };

            if self.show_numbers {
                gfx::text::draw_char(
                    canvas,
                    (tx + 3) as i16,
                    (ty + 6) as i16,
                    (b'1' + page_rel as u8) as char,
                    &TOMTHUMB,
                    fg,
                );
            }

            let base_icon_y = if self.show_labels { ty + 2 } else { ty + 5 };
            let icon_y = base_icon_y as i16 - if is_sel { anim_hop } else { 0 };
            gfx::icons::draw_icon_16(canvas, (tx + 12) as i16, icon_y, item.icon_16(), fg);

            if self.show_labels {
                let lbl = gfx::text::localize_scratch(item.title(), true);
                if gfx::text::str_width(lbl, &TOMTHUMB) > 36 {
                    gfx::text::draw_str_ellipsis(
                        canvas,
                        (tx + 2) as i16,
                        (ty + label_dy) as i16,
                        lbl,
                        &TOMTHUMB,
                        fg,
                        (tx + 38) as i16,
                    );
                } else {
                    gfx::text::draw_str_align(
                        canvas,
                        (tx + 20) as i16,
                        (ty + label_dy) as i16,
                        lbl,
                        &TOMTHUMB,
                        fg,
                        gfx::text::Align::Center,
                    );
                }
            }
        }

        if total_pages > 1 {
            let dot_spacing = 7u8;
            let dots_w = (total_pages as u8 - 1) * dot_spacing + 4;
            let start_x = 64u8.saturating_sub(dots_w >> 1);
            for p in 0..total_pages {
                let dx = start_x + (p as u8 * dot_spacing);
                if p == page {
                    canvas.fill_rect(dx, 61, 4, 2, Color::Fill);
                } else {
                    canvas.fill_rect(dx + 1, 61, 2, 2, Color::DarkGray);
                }
            }
        }
    }

    #[inline(never)]
    fn render_list(&self, canvas: &mut Canvas) {
        let total = self.items.len();
        if total == 0 {
            return;
        }
        let scroll_y = self.scroller.current();
        let start = self.scroll_top as usize;
        let end = (start + 5).min(total);
        let torch_on = is_torch_on();

        #[cfg(all(target_arch = "arm", feature = "radio"))]
        let active_freq_hz = {
            let coord = &crate::radio_state_ref().coord;
            coord
                .vfo_channel(coord.active_vfo)
                .effective_rx_freq()
                .raw()
        };
        #[cfg(not(all(target_arch = "arm", feature = "radio")))]
        let active_freq_hz = 0u32;

        let mut freq_buf = [0u8; 12];
        let freq_str: &str = if active_freq_hz >= 1_000_000 {
            gfx::text::format_int_fixed(
                utils::math::div1000_u32(active_freq_hz) as i32,
                3,
                "",
                "",
                &mut freq_buf,
            )
        } else {
            ""
        };

        for i in start..end {
            let item = &self.items[i];
            let row_top = 9 + (i as i16 * 13) - scroll_y;
            if row_top <= -5 || row_top >= 64 {
                continue;
            }
            let is_sel = i == self.selected_idx as usize;
            let has_num = self.show_numbers && i < 9;
            let title = if self.show_labels { item.title() } else { "" };
            let mut spec = gfx::elements::ListRowSpec::new(2, row_top, 120, 13, title);
            spec.clip_top = 8;
            spec.prefix_icon = has_num.then_some(&gfx::icons::ICON_NONE);
            spec.icon = Some(item.icon_8());
            if matches!(item.target, LauncherTarget::Shortcut(Shortcut::Flashlight)) && torch_on {
                spec.value = "ON";
            } else if item.is_radio() && !freq_str.is_empty() {
                spec.value = freq_str;
            }
            if is_sel {
                let phase = ((self.tick_counter >> 1) & 3) as i8;
                spec.flags = gfx::elements::ROW_SELECTED
                    | gfx::elements::ROW_CHEVRON
                    | if self.is_pressed {
                        gfx::elements::ROW_OUTLINE
                    } else {
                        0
                    };
                spec.chevron_dx = (if phase == 3 { 1 } else { phase }) - 2;
            }
            gfx::elements::draw_list_row(canvas, &spec);

            if has_num && row_top >= 6 && row_top + 10 <= 64 {
                let d = [b'1' + i as u8];
                let key_str = unsafe { core::str::from_utf8_unchecked(&d) };
                gfx::elements::draw_key_badge_box(
                    canvas,
                    5,
                    row_top + 3,
                    key_str,
                    is_sel && !self.is_pressed,
                );
            }
        }

        canvas.fill_rect(0, 0, 128, 8, Color::Clear);
        if total > 4 {
            gfx::elements::draw_scrollbar(
                canvas,
                125,
                9,
                52,
                scroll_y.max(0) as usize,
                total * 13,
                52,
            );
        }
    }

    pub fn render(&self, canvas: &mut Canvas) {
        if self.is_list_mode {
            self.render_list(canvas);
        } else {
            self.render_grid(canvas);
        }
    }

    pub fn render_with_status(&self, canvas: &mut Canvas, sb_cfg: &mut StatusBarConfig<'_>) {
        self.render(canvas);
        sb_cfg.visible = true;
        let tag = crate::services::config().system.tag_str();
        sb_cfg.title = Some(if !tag.is_empty() && tag != "KERN" {
            tag
        } else {
            "Home"
        });
        let total = self.items.len();
        if !self.is_list_mode && total > 6 {
            let page = (utils::math::div3_u16((self.selected_idx >> 1) as u16).0 as usize).min(1);
            sb_cfg.left_badge = Some(PAGE_BADGES[page]);
        } else if is_torch_on() {
            sb_cfg.left_badge = Some("TORCH");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoDev;
    impl kernfs::BlockDevice for NoDev {
        fn read(&mut self, _: usize, _: &mut [u8]) -> Result<(), kernfs::FsError> {
            Err(kernfs::FsError::NotFound)
        }
        fn program(&mut self, _: usize, _: &[u8]) -> Result<(), kernfs::FsError> {
            Err(kernfs::FsError::NotFound)
        }
        fn erase_sector(&mut self, _: usize) -> Result<(), kernfs::FsError> {
            Err(kernfs::FsError::NotFound)
        }
    }

    static CATALOG_TEST_LOCK: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);

    struct CatalogTestGuard;
    impl CatalogTestGuard {
        fn lock() -> Self {
            while CATALOG_TEST_LOCK
                .compare_exchange_weak(
                    false,
                    true,
                    core::sync::atomic::Ordering::Acquire,
                    core::sync::atomic::Ordering::Relaxed,
                )
                .is_err()
            {
                core::hint::spin_loop();
            }
            Self
        }
    }
    impl Drop for CatalogTestGuard {
        fn drop(&mut self) {
            CATALOG_TEST_LOCK.store(false, core::sync::atomic::Ordering::Release);
        }
    }

    #[test]
    fn launcher_entries_and_targets_are_compact() {
        let _guard = CatalogTestGuard::lock();
        assert!(core::mem::size_of::<LauncherTarget>() <= 4);
        assert!(core::mem::size_of::<LauncherEntry>() <= 4);
        catalog::get_catalog().entries.clear();
        let entries = load_entries::<NoDev>(None);
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
        assert_eq!(entries[0].icon_16(), gfx::icons::ICON_16_RADIO);
    }

    #[test]
    fn settings_krx_replaces_builtin_settings_slot_without_duplication() {
        let _guard = CatalogTestGuard::lock();
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

        let entries = load_entries::<NoDev>(None);
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

        let entries = load_entries::<NoDev>(None);
        assert_eq!(entries.len(), 4);
        assert_eq!(entries[0].title(), "Radio");
        assert_eq!(entries[0].target, LauncherTarget::Package(1));
        assert_eq!(entries[1].target, LauncherTarget::Package(0));

        cat.entries.clear();
    }

    #[test]
    fn launcher_preserves_selection_and_scroll_across_exit_and_style_toggle() {
        let _guard = CatalogTestGuard::lock();
        let mut app = core::mem::MaybeUninit::<LauncherApp>::zeroed();
        unsafe { LauncherApp::init_at(app.as_mut_ptr()) };
        let app = unsafe { app.assume_init_mut() };
        app.selected_idx = 3;
        app.adjust_scroll(false);
        assert_eq!(app.selected_idx, 3);
        let saved_scroll = app.scroll_top;

        let mut canvas = Canvas::new();
        app.render(&mut canvas);
        assert_eq!(
            app.on_event(&Event::Key {
                code: KeyCode::Star,
                state: KeyState::Pressed,
            }),
            EventResult::Consumed
        );
        assert!(app.is_list_mode);
        assert_eq!(app.selected_idx, 3);
        canvas.clear();
        app.render(&mut canvas);

        let entries = load_entries::<NoDev>(None);
        app.set_entries(entries);
        app.adjust_scroll(false);
        assert_eq!(app.selected_idx, 3);
        assert_eq!(app.scroll_top, saved_scroll);
        let _ = crate::master_config().prop_set(::abi::prop::LAUNCHER_STYLE, 0);
    }

    #[test]
    fn trailing_held_and_released_without_press_never_launches_radio() {
        let mut app = core::mem::MaybeUninit::<LauncherApp>::zeroed();
        unsafe { LauncherApp::init_at(app.as_mut_ptr()) };
        let app = unsafe { app.assume_init_mut() };
        assert_eq!(app.selected_idx, 0);
        assert!(!app.is_pressed);

        while crate::system::dequeue_event().is_some() {}

        let held_res = app.on_event(&Event::Key {
            code: KeyCode::Menu,
            state: KeyState::Held,
        });
        assert_eq!(held_res, EventResult::Ignored);
        assert!(!app.is_pressed);

        let rel_res = app.on_event(&Event::Key {
            code: KeyCode::Menu,
            state: KeyState::Released,
        });
        assert_eq!(rel_res, EventResult::Ignored);
        assert!(crate::system::dequeue_event().is_none());
    }
}
