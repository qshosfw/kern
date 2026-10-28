use crate::SysContext;
use crate::apps::{APP_ID_SETTINGS, APP_ID_TASKS, APP_ID_VFO, Shortcut};
use crate::services::power::PowerService;
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
    Diagnostics,
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

pub const LAUNCHER_ENTRIES: &[LauncherEntry] = &[
    LauncherEntry {
        title: "Radio",
        icon_8: &crate::apps::vfo::assets::ICON_8,
        icon_16: &crate::apps::vfo::assets::ICON_16,
        target: LauncherTarget::App(APP_ID_VFO),
    },
    LauncherEntry {
        title: "Settings",
        icon_8: &crate::apps::settings::assets::ICON_8,
        icon_16: &crate::apps::settings::assets::ICON_16,
        target: LauncherTarget::App(APP_ID_SETTINGS),
    },
    #[cfg(feature = "app_explorer")]
    LauncherEntry {
        title: "Explorer",
        icon_8: &crate::apps::explorer::assets::ICON_8,
        icon_16: &crate::apps::explorer::assets::ICON_16,
        target: LauncherTarget::Shortcut(Shortcut::Explorer),
    },
    LauncherEntry {
        title: "Tasks",
        icon_8: &assets::ICON_DIAG_8,
        icon_16: &assets::ICON_DIAG_16,
        target: LauncherTarget::App(APP_ID_TASKS),
    },
    LauncherEntry {
        title: "Counter",
        icon_8: &assets::ICON_DIAG_8,
        icon_16: &assets::ICON_DIAG_16,
        target: LauncherTarget::Package(0),
    },
    LauncherEntry {
        title: "Timer",
        icon_8: &assets::ICON_DIAG_8,
        icon_16: &assets::ICON_DIAG_16,
        target: LauncherTarget::Package(1),
    },
    LauncherEntry {
        title: "Torch",
        icon_8: &assets::ICON_TORCH_8,
        icon_16: &assets::ICON_TORCH_16,
        target: LauncherTarget::Shortcut(Shortcut::Flashlight),
    },
    LauncherEntry {
        title: "Diag",
        icon_8: &assets::ICON_DIAG_8,
        icon_16: &assets::ICON_DIAG_16,
        target: LauncherTarget::Diagnostics,
    },
    LauncherEntry {
        title: "About",
        icon_8: &assets::ICON_ABOUT_8,
        icon_16: &assets::ICON_ABOUT_16,
        target: LauncherTarget::About,
    },
];

pub struct LauncherApp {
    model: GridMenuModel<LauncherEntry>,
    view: GridMenuView,
    scroll_top: usize,
    scroller: gfx::gui::animation::KineticScroller,
}

impl LauncherApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).model = GridMenuModel::new(LAUNCHER_ENTRIES, "KERN OS");
            (*ptr).view = GridMenuView::new();
            (*ptr).scroll_top = 0;
            (*ptr).scroller = gfx::gui::animation::KineticScroller::new(2);
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.scroller.is_animating()
    }

    #[inline(never)]
    fn show_diagnostics(&self, ctx: &mut SysContext) {
        let mut msg = heapless::String::<128>::new();
        let v_mv = ctx.power.battery_voltage_mv();
        let pct = ctx.power.battery_percentage();
        let clk = if ctx.config.battery.cpu_clock == 1 {
            "72MHz"
        } else {
            "48MHz"
        };
        let up = ctx.power.uptime_secs();
        let stk = kernel::sys::SystemDiagnostics::stack_margin_bytes();
        let peak = kernel::sys::SystemDiagnostics::stack_peak_used_bytes();
        let mut buf = [0u8; 12];
        let _ = msg.push_str("Battery: ");
        let _ = msg.push_str(gfx::text::format_voltage(v_mv, 2, &mut buf));
        let _ = msg.push_str(" (");
        let _ = msg.push_str(gfx::text::format_u32(pct as u32, &mut buf));
        let _ = msg.push_str("%)\nCPU Clock: ");
        let _ = msg.push_str(clk);
        let _ = msg.push_str("\nStack Free: ");
        let _ = msg.push_str(gfx::text::format_u32(stk as u32, &mut buf));
        let _ = msg.push_str("B\nStack Peak: ");
        let _ = msg.push_str(gfx::text::format_u32(peak as u32, &mut buf));
        let _ = msg.push_str("\nUptime: ");
        let _ = msg.push_str(gfx::text::format_u32(up, &mut buf));
        let _ = msg.push('s');
        ctx.show_popup_str(0, "Diagnostics", msg.as_str());
    }

    fn show_about(&self, ctx: &mut SysContext) {
        ctx.show_popup_str(0, "About KERN", crate::build_info::ABOUT_TEXT);
    }

    #[inline(never)]
    fn execute_target(&self, ctx: &mut SysContext, target: LauncherTarget) {
        match target {
            LauncherTarget::App(app_id) => {
                ctx.post_event(Shortcut::LaunchApp(app_id as u8).into());
            }
            LauncherTarget::Shortcut(sc) => {
                ctx.post_event(sc.into());
            }
            LauncherTarget::Diagnostics => {
                self.show_diagnostics(ctx);
            }
            LauncherTarget::About => {
                self.show_about(ctx);
            }
            LauncherTarget::Package(pkg_id) => {
                launch_package(ctx, pkg_id);
            }
        }
    }
}

#[inline(never)]
fn launch_package(ctx: &mut SysContext, pkg_id: u8) {
    static COUNTER_LABELS: [&str; 5] = ["Tally", "Count +1", "Count -1", "Reset (0)", "Current"];
    static COUNTER_ITEMS: [kernel::proc::krx::DeclareItem; 4] = [
        kernel::proc::krx::DeclareItem {
            kind: kernel::proc::declare::ITEM_ACTION,
            flags: 0,
            label_offset: 1,
            target: 0,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        },
        kernel::proc::krx::DeclareItem {
            kind: kernel::proc::declare::ITEM_ACTION,
            flags: 0,
            label_offset: 2,
            target: 1,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        },
        kernel::proc::krx::DeclareItem {
            kind: kernel::proc::declare::ITEM_ACTION,
            flags: 0,
            label_offset: 3,
            target: 2,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        },
        kernel::proc::krx::DeclareItem {
            kind: kernel::proc::declare::ITEM_VALUE,
            flags: 0,
            label_offset: 4,
            target: 0,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        },
    ];
    static COUNTER_ACTIONS: [kernel::proc::krx::ActionRecord; 3] = [
        kernel::proc::krx::ActionRecord {
            op: kernel::proc::declare::ACTION_SET_STATE,
            flags: 0,
            a: 0,
            b: 1,
            c: 1,
        },
        kernel::proc::krx::ActionRecord {
            op: kernel::proc::declare::ACTION_SET_STATE,
            flags: 0,
            a: 0,
            b: !0,
            c: 1,
        },
        kernel::proc::krx::ActionRecord {
            op: kernel::proc::declare::ACTION_SET_STATE,
            flags: 0,
            a: 0,
            b: 0,
            c: 0,
        },
    ];

    static TIMER_LABELS: [&str; 4] = ["Elapsed", "Start (+10)", "Reset", "Seconds"];
    static TIMER_ITEMS: [kernel::proc::krx::DeclareItem; 3] = [
        kernel::proc::krx::DeclareItem {
            kind: kernel::proc::declare::ITEM_ACTION,
            flags: 0,
            label_offset: 1,
            target: 0,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        },
        kernel::proc::krx::DeclareItem {
            kind: kernel::proc::declare::ITEM_ACTION,
            flags: 0,
            label_offset: 2,
            target: 1,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        },
        kernel::proc::krx::DeclareItem {
            kind: kernel::proc::declare::ITEM_VALUE,
            flags: 0,
            label_offset: 3,
            target: 0,
            value_ref: 0,
            aux: 0,
            reserved: 0,
        },
    ];
    static TIMER_ACTIONS: [kernel::proc::krx::ActionRecord; 2] = [
        kernel::proc::krx::ActionRecord {
            op: kernel::proc::declare::ACTION_SET_STATE,
            flags: 0,
            a: 0,
            b: 10,
            c: 1,
        },
        kernel::proc::krx::ActionRecord {
            op: kernel::proc::declare::ACTION_SET_STATE,
            flags: 0,
            a: 0,
            b: 0,
            c: 0,
        },
    ];

    static NODES: [kernel::proc::krx::DeclareNode; 2] = [
        kernel::proc::krx::DeclareNode {
            node_id: 0,
            layout: kernel::proc::declare::LAYOUT_MENU,
            item_count: 4,
            title_offset: 0,
            first_item_offset: 0,
        },
        kernel::proc::krx::DeclareNode {
            node_id: 0,
            layout: kernel::proc::declare::LAYOUT_MENU,
            item_count: 3,
            title_offset: 0,
            first_item_offset: 0,
        },
    ];

    if pkg_id >= 2 {
        let cat = crate::apps::launcher::catalog::get_catalog();
        if let Some(app) = cat.apps().nth((pkg_id - 2) as usize) {
            let mut path_buf = [0u8; 32];
            path_buf[..6].copy_from_slice(b"/apps/");
            let flen = app.filename.len().min(26);
            path_buf[6..6 + flen].copy_from_slice(&app.filename.as_bytes()[..flen]);
            if let Ok(path) = core::str::from_utf8(&path_buf[..6 + flen]) {
                let _ = crate::apps::runtime::launch_krx_package(ctx, path);
            }
            return;
        }
    }

    let walker = crate::apps::runtime::get_global_walker();
    if pkg_id == 0 {
        walker.load_static_package(
            "Counter",
            &NODES[0..1],
            &COUNTER_ITEMS,
            &COUNTER_LABELS,
            &COUNTER_ACTIONS,
        );
    } else {
        walker.load_static_package(
            "Timer",
            &NODES[1..2],
            &TIMER_ITEMS,
            &TIMER_LABELS,
            &TIMER_ACTIONS,
        );
    }
    let dyn_id = crate::apps::AppRegistry::register_dynamic(crate::apps::DynamicAppRecord {
        metadata: kernel::app::AppMetadata {
            name: walker.app_name,
            icon: &assets::ICON_DIAG_16,
            icon_8: &assets::ICON_DIAG_8,
        },
        vtable: crate::apps::runtime::DECLARE_WALKER_VTABLE,
        instance: walker as *mut _ as *mut (),
        required_capabilities: kernel::caps::CapabilitySet::NONE,
    });
    ctx.post_event(Shortcut::LaunchApp(dyn_id as u8).into());
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

        for idx in start_row..end_row {
            let row_top = 9 + (idx as i16 * row_height) - scroll_y;
            if row_top + row_height <= 8 || row_top >= 64 {
                continue;
            }

            let y = row_top as u8;
            let is_sel = idx == self.model.selected_idx;
            let item = &self.model.items[idx];

            let fg_color = if is_sel {
                canvas.fill_rect(2, y, 120, 13, Color::Fill);
                canvas.clear_corners(2, y, 120, 13);
                Color::Clear
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

            // Right arrow on selected row (vertically centered at y+3, subtle 1px animation)
            if is_sel {
                let arrow_x = 111 + (((self.model.tick_counter >> 3) & 1) as i16);
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

        // Minimalist vertical scrollbar
        if total > visible_count {
            let track_h = 52u8;
            let thumb_h = ((visible_count as u32 * track_h as u32) / total as u32).max(6) as u8;
            let max_scroll = (total - visible_count) as u32;
            let thumb_y =
                9u8 + ((self.scroll_top as u32 * (track_h - thumb_h) as u32) / max_scroll) as u8;
            canvas.draw_vertical_line(126, 9, track_h, Color::Fill);
            canvas.fill_rect(125, thumb_y, 3, thumb_h, Color::Fill);
        }
    }
}

impl<'a> App<crate::services::FirmwareContext<'a, '_>> for LauncherApp {
    fn meta() -> AppMetadata {
        AppMetadata {
            name: "Launcher",
            icon: &[0; 32],
            icon_8: &[0; 8],
        }
    }

    fn on_start(&mut self, ctx: &mut SysContext) {
        if self.model.items.is_empty() {
            self.model = GridMenuModel::new(LAUNCHER_ENTRIES, "KERN OS");
            self.view = GridMenuView::new();
            self.scroll_top = 0;
            self.scroller.jump(0);
        } else {
            self.scroller.jump((self.scroll_top as i16) * 13);
        }
        self.model.nav_layout = ctx.config.keys.nav_layout;
        self.model.show_numbers = ctx.config.apps.show_numbers != 0;
        self.model.show_labels = ctx.config.apps.show_labels != 0;
        #[cfg(feature = "filesystem")]
        catalog::get_catalog().scan(ctx.storage);
    }

    #[inline(never)]
    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        self.model.nav_layout = ctx.config.keys.nav_layout;
        self.model.show_numbers = ctx.config.apps.show_numbers != 0;
        self.model.show_labels = ctx.config.apps.show_labels != 0;

        let mut animating = false;
        if let Event::Tick = event {
            animating = self.scroller.tick();
        }

        if let Event::Key {
            code,
            state: KeyState::Pressed,
        } = *event
        {
            if code == KeyCode::Exit {
                ctx.post_event(Shortcut::LaunchApp(APP_ID_VFO as u8).into());
                return EventResult::Consumed;
            }
            if code == KeyCode::Menu {
                if let Some(entry) = self.model.selected_item() {
                    self.execute_target(ctx, entry.target);
                    return EventResult::Consumed;
                }
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
                    self.execute_target(ctx, LauncherTarget::Shortcut(Shortcut::Flashlight));
                    return EventResult::Consumed;
                }
                _ => None,
            };
            if let Some(idx) = direct_idx {
                if idx < self.model.items.len() {
                    let entry = &self.model.items[idx];
                    self.execute_target(ctx, entry.target);
                    return EventResult::Consumed;
                }
            }
        }

        let is_list_mode = ctx.config.apps.launcher_style == 1;

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
                    let prev_sel = self.model.selected_idx;
                    let is_tick = matches!(event, Event::Tick);
                    match dir {
                        gfx::gui::modules::menu::ListNavDir::Down => {
                            if self.model.selected_idx + 1 < total {
                                self.model.selected_idx += 1;
                            } else if !is_tick || total > 4 {
                                // Wrap bottom -> top (only on single-press or long lists)
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
                                // Wrap top -> bottom (only on single-press or long lists)
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
                        // Windowed edge-push scrolling
                        let prev_scroll = self.scroll_top;
                        if self.model.selected_idx < self.scroll_top {
                            self.scroll_top = self.model.selected_idx;
                        } else if self.model.selected_idx >= self.scroll_top + 4 {
                            self.scroll_top = self.model.selected_idx - 3;
                        }
                        if self.scroll_top != prev_scroll {
                            if ctx.config.display.ui_animations != 0 {
                                self.scroller.set_target((self.scroll_top as i16) * 13);
                            } else {
                                self.scroller.jump((self.scroll_top as i16) * 13);
                            }
                        }
                        return EventResult::Consumed;
                    }
                }
            }
        } else {
            let prev_sel = self.model.selected_idx;
            if self.view.on_event(&mut self.model, &event) {
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

        EventResult::Ignored
    }

    fn status_bar_config(&self) -> kernel::app::StatusBarConfig<'_> {
        kernel::app::StatusBarConfig {
            visible: true,
            title: None,
            center_text: None,
            ..Default::default()
        }
    }

    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, ctx: &SysContext) {
        if ctx.config.apps.launcher_style == 1 {
            self.render_list(canvas);
        } else {
            self.view.render(canvas, &self.model);
        }
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}
