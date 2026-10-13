use crate::SysContext;
#[cfg(feature = "filesystem")]
use crate::apps::APP_ID_EXPLORER;
use crate::apps::{APP_ID_SETTINGS, APP_ID_VFO, Shortcut};
use crate::services::power::PowerService;
use gfx::Canvas;
use gfx::Color;
use gfx::gui::modules::grid_menu::{GridItem, GridMenuModel, GridMenuView};
use gfx::gui::view::View;
#[cfg(feature = "filesystem")]
use gfx::icons::ICON_16_SCANNER;
use gfx::icons::{
    ICON_16_DIAGNOSTICS, ICON_16_FLASHLIGHT, ICON_16_INFO, ICON_16_RADIO, ICON_16_SETTINGS,
};
use kernel::app::{App, AppMetadata, EventResult};
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherTarget {
    App(usize),
    Shortcut(Shortcut),
    Diagnostics,
    About,
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
        icon_8: &gfx::icons::ICON_APP_RADIO,
        icon_16: &ICON_16_RADIO,
        target: LauncherTarget::App(APP_ID_VFO),
    },
    LauncherEntry {
        title: "Settings",
        icon_8: &gfx::icons::ICON_APP_SETTINGS,
        icon_16: &ICON_16_SETTINGS,
        target: LauncherTarget::App(APP_ID_SETTINGS),
    },
    #[cfg(feature = "filesystem")]
    LauncherEntry {
        title: "Explorer",
        icon_8: &gfx::icons::ICON_APP_EXPLORER,
        icon_16: &ICON_16_SCANNER,
        target: LauncherTarget::App(APP_ID_EXPLORER),
    },
    LauncherEntry {
        title: "Torch",
        icon_8: &gfx::icons::ICON_APP_TORCH,
        icon_16: &ICON_16_FLASHLIGHT,
        target: LauncherTarget::Shortcut(Shortcut::Flashlight),
    },
    LauncherEntry {
        title: "Diag",
        icon_8: &gfx::icons::ICON_APP_DIAG,
        icon_16: &ICON_16_DIAGNOSTICS,
        target: LauncherTarget::Diagnostics,
    },
    LauncherEntry {
        title: "About",
        icon_8: &gfx::icons::ICON_APP_ABOUT,
        icon_16: &ICON_16_INFO,
        target: LauncherTarget::About,
    },
];

pub struct LauncherApp {
    model: GridMenuModel<LauncherEntry>,
    view: GridMenuView,
    scroll_top: usize,
}

impl LauncherApp {
    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).model = GridMenuModel::new(LAUNCHER_ENTRIES, "KERN OS");
            (*ptr).view = GridMenuView::new();
            (*ptr).scroll_top = 0;
        }
    }

    fn show_diagnostics(&self, ctx: &mut SysContext) {
        let mut msg = heapless::String::<140>::new();
        let v_mv = ctx.power.battery_voltage_mv();
        let pct = ctx.power.battery_percentage();
        let clk = if ctx.config.battery.cpu_clock == 1 {
            "72MHz"
        } else {
            "48MHz"
        };
        let up = ctx.power.uptime_secs();
        let stk = kernel::sys::SystemDiagnostics::stack_margin_bytes();
        let mut buf = [0u8; 12];
        let _ = msg.push_str("Battery: ");
        let _ = msg.push_str(gfx::text::format_voltage(v_mv, 2, &mut buf));
        let _ = msg.push_str(" (");
        let _ = msg.push_str(gfx::text::format_u32(pct as u32, &mut buf));
        let _ = msg.push_str("%)\nCPU Clock: ");
        let _ = msg.push_str(clk);
        let _ = msg.push_str("\nStack Free: ");
        let _ = msg.push_str(gfx::text::format_u32(stk as u32, &mut buf));
        let _ = msg.push_str("B\nUptime: ");
        let _ = msg.push_str(gfx::text::format_u32(up, &mut buf));
        let _ = msg.push('s');
        ctx.show_popup_str(0, "Diagnostics", msg.as_str());
    }

    fn show_about(&self, ctx: &mut SysContext) {
        ctx.show_popup_str(0, "About KERN", crate::build_info::ABOUT_TEXT);
    }

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
        }
    }

    fn render_list(&self, canvas: &mut Canvas) {
        let total = self.model.items.len();
        if total == 0 {
            return;
        }

        let visible_count = 4;
        let mut scroll = self.scroll_top;
        if self.model.selected_idx < scroll {
            scroll = self.model.selected_idx;
        } else if self.model.selected_idx >= scroll + visible_count {
            scroll = self.model.selected_idx - (visible_count - 1);
        }

        let row_height = 13u8;
        let start_y = 9u8;

        for i in 0..visible_count {
            let idx = scroll + i;
            if idx >= total {
                break;
            }

            let y = start_y + (i as u8 * row_height);
            let is_sel = idx == self.model.selected_idx;
            let item = &self.model.items[idx];

            let fg_color = if is_sel {
                canvas.fill_rect(2, y, 120, 13, Color::Fill);
                canvas.draw_pixel(2, y, Color::Clear);
                canvas.draw_pixel(121, y, Color::Clear);
                canvas.draw_pixel(2, y + 12, Color::Clear);
                canvas.draw_pixel(121, y + 12, Color::Clear);
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
                    (y + 9) as i16,
                    badge_char,
                    &gfx::fonts::mumatrix_regular::FONT,
                    fg_color,
                );
                cur_x = 16;
            }

            // 8x8 icon
            gfx::icons::draw_icon_8(canvas, cur_x, (y + 2) as i16, item.icon_8, fg_color);
            cur_x += 14;

            // Title in MuMatrix
            if self.model.show_labels {
                gfx::text::draw_str(
                    canvas,
                    cur_x,
                    (y + 9) as i16,
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
                    (y + 3) as i16,
                    &gfx::icons::ARROW_RIGHT,
                    fg_color,
                );
            }
        }

        // Minimalist vertical scrollbar
        if total > visible_count {
            let track_h = 52u8;
            let thumb_h = ((visible_count as u32 * track_h as u32) / total as u32).max(6) as u8;
            let max_scroll = (total - visible_count) as u32;
            let thumb_y = 9u8 + ((scroll as u32 * (track_h - thumb_h) as u32) / max_scroll) as u8;
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
        }
    }

    fn on_start(&mut self, ctx: &mut SysContext) {
        self.model.nav_layout = ctx.config.keys.nav_layout;
        self.model.show_numbers = ctx.config.apps.show_numbers != 0;
        self.model.show_labels = ctx.config.apps.show_labels != 0;
    }

    fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
        self.model.nav_layout = ctx.config.keys.nav_layout;
        self.model.show_numbers = ctx.config.apps.show_numbers != 0;
        self.model.show_labels = ctx.config.apps.show_labels != 0;

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

        if self.view.on_event(&mut self.model, &event) {
            // Keep scroll_top in sync with selected index
            if self.model.selected_idx < self.scroll_top {
                self.scroll_top = self.model.selected_idx;
            } else if self.model.selected_idx >= self.scroll_top + 4 {
                self.scroll_top = self.model.selected_idx - 3;
            }
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

    fn render(&self, canvas: &mut Canvas, ctx: &SysContext) {
        if ctx.config.apps.launcher_style == 1 {
            self.render_list(canvas);
        } else {
            self.view.render(canvas, &self.model);
        }
    }

    fn on_exit(&mut self, _ctx: &mut SysContext) {}
}
