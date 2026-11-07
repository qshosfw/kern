//! System Process and Task Manager Application (§12.3 / KRX-ABI 2.5.0).
//!
//! Provides lightweight, zero-allocation process monitoring, stack watermark introspection,
//! CPU execution stats, and process control (freeze/suspend, hibernate, kill, switch).

use crate::SysContext;
use gfx::Canvas;
use gfx::Color;
use gfx::fonts::tomthumb::FONT;
use gfx::gui::modules::{CompactListState, TabbedSheet, TabbedSheetConfig, TabbedSheetState};
use gfx::text;
use kernel::app::{App, AppMetadata, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};
use kernel::proc::{ProcState, ProcessTable};
use kernel::sys::SystemDiagnostics;

#[inline(never)]
fn draw_stat(cv: &mut Canvas, x: i16, y: i16, label: &str, val: u32, suffix: &str) {
    text::draw_str(cv, x, y, label, &FONT, Color::Fill);
    let lx = x + text::str_width(label, &FONT);
    let mut buf = [0u8; 12];
    let s = text::format_int_fixed(val as i32, 0, "", suffix, &mut buf);
    text::draw_str(cv, lx, y, s, &FONT, Color::Fill);
}

const TAB_TITLES: [&str; 3] = ["Tasks", "Memory", "Services"];

pub struct TasksApp {
    tabs: TabbedSheetState,
    list: CompactListState,
    selected_pid: Option<u8>,
    show_info_modal: bool,
}

impl TasksApp {
    pub const fn new() -> Self {
        Self {
            tabs: TabbedSheetState::new(3),
            list: CompactListState::new(),
            selected_pid: None,
            show_info_modal: false,
        }
    }

    pub unsafe fn init_at(ptr: *mut Self) {
        unsafe {
            (*ptr).tabs = TabbedSheetState::new(3);
            (*ptr).list.reset();
            (*ptr).selected_pid = None;
            (*ptr).show_info_modal = false;
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.tabs.is_animating() || self.list.is_animating()
    }

    #[inline(always)]
    fn proc_table() -> &'static ProcessTable<crate::orchestrator::MAX_PROCESSES> {
        crate::orchestrator::proc_table()
    }

    #[inline(always)]
    fn proc_table_mut() -> &'static mut ProcessTable<crate::orchestrator::MAX_PROCESSES> {
        crate::orchestrator::proc_table_mut()
    }

    fn active_pids(&self, out: &mut [u8; crate::orchestrator::MAX_PROCESSES]) -> usize {
        let table = Self::proc_table();
        let mut count = 0;
        for slot in table.slots.iter() {
            if slot.state != ProcState::Dormant && slot.state != ProcState::Dead {
                out[count] = slot.pid.0;
                count += 1;
            }
        }
        count
    }
}

impl<'a> App<SysContext<'a, '_>> for TasksApp {
    fn meta() -> AppMetadata {
        AppMetadata {
            name: "Tasks",
            icon: &[0u8; 32],
            icon_8: &[0u8; 8],
        }
    }

    fn status_bar_config(&self) -> StatusBarConfig<'_> {
        let mut cfg = StatusBarConfig::default();
        cfg.visible = true;
        cfg.title = Some("");
        cfg.show_divider = false;
        cfg
    }

    fn on_start(&mut self, _ctx: &mut SysContext<'a, '_>) {
        self.show_info_modal = false;
    }

    fn on_event(&mut self, ctx: &mut SysContext<'a, '_>, event: &Event) -> EventResult {
        if let Event::Tick = *event {
            let mut anim = self.tabs.tick();
            if self.tabs.selected_idx == 0 {
                anim |= self.list.tick();
            }
            if anim {
                return EventResult::Consumed;
            }
        }

        let mut pids = [0u8; crate::orchestrator::MAX_PROCESSES];
        let count = self.active_pids(&mut pids);

        // 1. Process Info Modal handling
        if self.show_info_modal {
            if let Event::Key { code, state } = *event {
                if state == KeyState::Pressed {
                    match code {
                        KeyCode::Exit | KeyCode::Menu | KeyCode::Key1 => {
                            self.show_info_modal = false;
                            return EventResult::Consumed;
                        }
                        KeyCode::Key5 => {
                            if let Some(pid_val) = self.selected_pid {
                                let table = Self::proc_table_mut();
                                if let Some(pcb) = table.find_mut(kernel::proc::Pid(pid_val)) {
                                    if pcb.pid.0 > 4
                                        && pcb.app_id != crate::apps::APP_ID_TASKS
                                        && pcb.app_id < 0xF0
                                    {
                                        #[cfg(feature = "app_hibernation")]
                                        {
                                            let app_id = pcb.app_id;
                                            if pcb.is_hibernated() {
                                                crate::orchestrator::Orchestrator::resume_app_state(
                                                    app_id,
                                                );
                                                ctx.play_dialog_sound(
                                                    crate::services::melody::DialogSoundType::Success,
                                                );
                                            } else if pcb.state == ProcState::Suspended
                                                || pcb.state == ProcState::Ready
                                            {
                                                crate::orchestrator::Orchestrator::hibernate_app_state(
                                                    app_id,
                                                );
                                                ctx.play_dialog_sound(
                                                    crate::services::melody::DialogSoundType::Success,
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            return EventResult::Consumed;
                        }
                        KeyCode::Key8 => {
                            if let Some(pid_val) = self.selected_pid {
                                let table = Self::proc_table_mut();
                                if let Some(pcb) = table.find_mut(kernel::proc::Pid(pid_val)) {
                                    if pcb.pid.0 > 4 && pcb.app_id != crate::apps::APP_ID_TASKS {
                                        #[cfg(feature = "app_hibernation")]
                                        crate::runtime::AppSwapManager::clear_swap(pcb.app_id);
                                        pcb.set_state(ProcState::Dead);
                                        ctx.play_dialog_sound(
                                            crate::services::melody::DialogSoundType::Warning,
                                        );
                                    }
                                }
                            }
                            self.show_info_modal = false;
                            return EventResult::Consumed;
                        }
                        _ => return EventResult::Consumed,
                    }
                }
            }
            return EventResult::Ignored;
        }

        // 2. Tab switching handling
        if self.tabs.handle_key_event(event) {
            return EventResult::Consumed;
        }

        // 3. Tab-specific controls
        if self.tabs.selected_idx == 0 {
            // Tab 0: TASKS list navigation
            if self.list.handle_key_event_animated(event, count, 3, 9) {
                return EventResult::Consumed;
            }

            if let Event::Key { code, state } = *event {
                if state == KeyState::Pressed {
                    match code {
                        KeyCode::Menu => {
                            // Switch to selected process if it's an app, or open info modal for services
                            if count > 0 && self.list.selected_idx < count {
                                let pid_val = pids[self.list.selected_idx];
                                if let Some(pcb) =
                                    Self::proc_table().find(kernel::proc::Pid(pid_val))
                                {
                                    if pcb.app_id < 0xF0 {
                                        ctx.post_event(
                                            crate::apps::Shortcut::LaunchApp(pcb.app_id as u8)
                                                .into(),
                                        );
                                    } else {
                                        self.selected_pid = Some(pid_val);
                                        self.show_info_modal = true;
                                    }
                                }
                            }
                            return EventResult::Consumed;
                        }
                        KeyCode::Key5 => {
                            // Toggle Hibernate / Resume for selected app process
                            if count > 0 && self.list.selected_idx < count {
                                let pid_val = pids[self.list.selected_idx];
                                let table = Self::proc_table_mut();
                                if let Some(pcb) = table.find_mut(kernel::proc::Pid(pid_val)) {
                                    if pcb.pid.0 > 4
                                        && pcb.app_id != crate::apps::APP_ID_TASKS
                                        && pcb.app_id < 0xF0
                                    {
                                        #[cfg(feature = "app_hibernation")]
                                        {
                                            let app_id = pcb.app_id;
                                            if pcb.is_hibernated() {
                                                crate::orchestrator::Orchestrator::resume_app_state(
                                                    app_id,
                                                );
                                                ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Success);
                                            } else if pcb.state == ProcState::Suspended
                                                || pcb.state == ProcState::Ready
                                            {
                                                crate::orchestrator::Orchestrator::hibernate_app_state(app_id);
                                                ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Success);
                                            }
                                        }
                                    }
                                }
                            }
                            return EventResult::Consumed;
                        }
                        KeyCode::Key1 => {
                            // Open process telemetry modal
                            if count > 0 && self.list.selected_idx < count {
                                self.selected_pid = Some(pids[self.list.selected_idx]);
                                self.show_info_modal = true;
                                return EventResult::Consumed;
                            }
                        }
                        KeyCode::Key8 => {
                            // Terminate / Kill process (protect system services PIDs 1..4 and Tasks itself)
                            if count > 0 && self.list.selected_idx < count {
                                let pid_val = pids[self.list.selected_idx];
                                let table = Self::proc_table_mut();
                                if let Some(pcb) = table.find_mut(kernel::proc::Pid(pid_val)) {
                                    if pcb.pid.0 > 4 && pcb.app_id != crate::apps::APP_ID_TASKS {
                                        #[cfg(feature = "app_hibernation")]
                                        crate::runtime::AppSwapManager::clear_swap(pcb.app_id);
                                        pcb.set_state(ProcState::Dead);
                                        ctx.play_dialog_sound(
                                            crate::services::melody::DialogSoundType::Warning,
                                        );
                                    }
                                }
                            }
                            return EventResult::Consumed;
                        }
                        KeyCode::Exit => {
                            return EventResult::Ignored;
                        }
                        _ => {}
                    }
                }
            }
        } else {
            // Tabs 1 & 2: Navigation
            if let Event::Key { code, state } = *event {
                if state == KeyState::Pressed {
                    match code {
                        KeyCode::Up | KeyCode::Side1 => {
                            self.tabs.prev_tab();
                            return EventResult::Consumed;
                        }
                        KeyCode::Down | KeyCode::Side2 => {
                            self.tabs.next_tab();
                            return EventResult::Consumed;
                        }
                        KeyCode::Exit => {
                            return EventResult::Ignored;
                        }
                        _ => {}
                    }
                }
            }
        }

        EventResult::Ignored
    }

    fn on_modal_result(
        &mut self,
        _ctx: &mut SysContext<'a, '_>,
        _result: &ModalResult<'_>,
    ) -> EventResult {
        EventResult::Ignored
    }

    fn render(&self, canvas: &mut Canvas, ctx: &SysContext<'a, '_>) {
        let mut pids = [0u8; crate::orchestrator::MAX_PROCESSES];
        let count = self.active_pids(&mut pids);

        let tab_cfg = TabbedSheetConfig::new(0, 0, 128, 53).with_status_bar_integrated(86);

        TabbedSheet::render(
            canvas,
            &tab_cfg,
            &self.tabs,
            &TAB_TITLES,
            |cv, tab_idx, inner_x, inner_y, inner_w, inner_h, anim_offset| match tab_idx {
                0 => render_tasks_tab(
                    cv,
                    &self.list,
                    inner_x,
                    inner_y,
                    inner_w,
                    inner_h,
                    count,
                    &pids,
                    anim_offset,
                ),
                1 => render_memory_tab(cv, inner_x, inner_y, inner_w, anim_offset),
                2 => render_services_tab(cv, inner_x, inner_y, ctx, count, anim_offset),
                _ => {}
            },
        );

        // 2. Bottom Command Bar (Y=55..61, with 2px padding from tab border at Y=52)
        if self.tabs.selected_idx == 0 {
            let pid_val = if count > 0 && self.list.selected_idx < count {
                pids[self.list.selected_idx]
            } else {
                0
            };
            let is_app = if let Some(pcb) = Self::proc_table().find(kernel::proc::Pid(pid_val)) {
                pcb.app_id < 0xF0
            } else {
                false
            };

            if is_app {
                gfx::widgets::draw_key_badge(canvas, 2, 55, "M", "Switch");
                gfx::widgets::draw_key_badge(canvas, 42, 55, "1", "Info");
            } else {
                gfx::widgets::draw_key_badge(canvas, 2, 55, "M", "Info");
            }
        } else {
            gfx::widgets::draw_key_badge(canvas, 2, 55, "< >", "Tab");
        }
        gfx::widgets::draw_key_badge_right(canvas, 126, 55, "EXIT", "Back");

        // 3. Process Info Modal Dialog (when Key1 is pressed on Tab 0)
        if self.show_info_modal {
            let pid_to_inspect = self
                .selected_pid
                .unwrap_or(pids[self.list.selected_idx.min(count.saturating_sub(1))]);
            render_info_modal(canvas, pid_to_inspect);
        }
    }

    fn on_exit(&mut self, _ctx: &mut SysContext<'a, '_>) {}
}

#[inline(never)]
fn render_tasks_tab(
    cv: &mut Canvas,
    list: &CompactListState,
    inner_x: u8,
    inner_y: u8,
    inner_w: u8,
    inner_h: u8,
    count: usize,
    pids: &[u8; 10],
    anim_offset: i8,
) {
    let base_x = (inner_x as i16) + (anim_offset as i16);
    text::draw_str(
        cv,
        base_x + 2,
        (inner_y + 5) as i16,
        "Pid",
        &FONT,
        Color::Fill,
    );
    text::draw_str(
        cv,
        base_x + 16,
        (inner_y + 5) as i16,
        "Process",
        &FONT,
        Color::Fill,
    );
    text::draw_str(
        cv,
        base_x + 58,
        (inner_y + 5) as i16,
        "Stat",
        &FONT,
        Color::Fill,
    );
    text::draw_str(
        cv,
        base_x + 86,
        (inner_y + 5) as i16,
        "Ticks",
        &FONT,
        Color::Fill,
    );
    cv.draw_horizontal_line(inner_x, inner_y + 7, inner_w, Color::DarkGray);

    let list_cfg = gfx::widgets::CompactListConfig {
        x: inner_x,
        y: inner_y + 8,
        w: inner_w,
        h: inner_h.saturating_sub(8),
        row_height: 9,
        show_scrollbar: true,
        rounded_selection: false,
    };

    list.render_smooth(cv, &list_cfg, count, |rcv, idx, _x, y, _w, _h, is_sel| {
        let pid_val = pids[idx];
        if let Some(pcb) = TasksApp::proc_table().find(kernel::proc::Pid(pid_val)) {
            let fg = if is_sel { Color::Clear } else { Color::Fill };
            let mut pbuf = [0u8; 8];
            let pstr = text::format_int_fixed(pcb.pid.0 as i32, 0, "", "", &mut pbuf);
            text::draw_str(rcv, base_x + 3, y + 7, pstr, &FONT, fg);
            text::draw_str_max_x(rcv, base_x + 16, y + 7, pcb.name.as_str(), &FONT, fg, 40);

            let state_str = match pcb.state {
                ProcState::Running => "RUN",
                ProcState::Ready => "RDY",
                ProcState::Suspended => "SUSP",
                ProcState::ServiceIdle => "IDLE",
                ProcState::Hibernated => "HIB*",
                ProcState::Stopped => "STOP",
                _ => "---",
            };
            text::draw_str(rcv, base_x + 58, y + 7, state_str, &FONT, fg);

            let mut tbuf = [0u8; 12];
            let tstr = if pcb.ledger.cpu_ticks < 10000 {
                text::format_u32(pcb.ledger.cpu_ticks, &mut tbuf)
            } else {
                text::format_int_fixed((pcb.ledger.cpu_ticks / 1000) as i32, 0, "", "K", &mut tbuf)
            };
            text::draw_str(rcv, base_x + 86, y + 7, tstr, &FONT, fg);
        }
    });
}

#[inline(never)]
fn render_memory_tab(cv: &mut Canvas, inner_x: u8, inner_y: u8, inner_w: u8, anim_offset: i8) {
    let ram_used = SystemDiagnostics::ram_static_used_bytes();
    let stk_now = SystemDiagnostics::stack_used_now();
    let stk_peak = SystemDiagnostics::stack_peak_used_bytes();
    let stk_margin = SystemDiagnostics::stack_margin_bytes();
    let max_apps = SystemDiagnostics::dynamic_max_foreground_apps();
    let pressure = SystemDiagnostics::memory_pressure();

    let base_x = (inner_x as i16) + (anim_offset as i16);

    draw_stat(
        cv,
        base_x + 2,
        (inner_y + 6) as i16,
        "SRAM: ",
        ram_used as u32,
        " B",
    );
    draw_stat(
        cv,
        base_x + 2,
        (inner_y + 13) as i16,
        "Stack: ",
        stk_now as u32,
        " B",
    );
    draw_stat(
        cv,
        base_x + 2,
        (inner_y + 20) as i16,
        "Peak: ",
        stk_peak as u32,
        " B",
    );
    draw_stat(
        cv,
        base_x + 2,
        (inner_y + 27) as i16,
        "Margin: ",
        stk_margin as u32,
        " B",
    );

    draw_stat(
        cv,
        base_x + 64,
        (inner_y + 6) as i16,
        "Apps: ",
        max_apps as u32,
        " max",
    );
    text::draw_str(
        cv,
        base_x + 64,
        (inner_y + 13) as i16,
        "Tier: ",
        &FONT,
        Color::Fill,
    );
    let px = base_x + 64 + text::str_width("Tier: ", &FONT);
    text::draw_str(
        cv,
        px,
        (inner_y + 13) as i16,
        pressure.as_str(),
        &FONT,
        Color::Fill,
    );

    #[cfg(feature = "app_hibernation")]
    {
        let swap_used = crate::runtime::AppSwapManager::total_swap_bytes_used();
        draw_stat(
            cv,
            base_x + 64,
            (inner_y + 20) as i16,
            "Swap: ",
            swap_used as u32,
            " B",
        );
    }

    let bar_w = inner_w.saturating_sub(4);
    let bar_pct = ((stk_peak as u32 * 100) / 2048).min(100) as u8;
    gfx::widgets::draw_progress_bar(
        cv,
        base_x + 2,
        (inner_y + 29) as i16,
        bar_w,
        4,
        bar_pct,
        false,
    );
}

#[inline(never)]
fn render_services_tab(
    cv: &mut Canvas,
    inner_x: u8,
    inner_y: u8,
    ctx: &SysContext<'_, '_>,
    count: usize,
    anim_offset: i8,
) {
    let base_x = (inner_x as i16) + (anim_offset as i16);
    let rad_active = ctx.state.coord.is_transmitting || ctx.state.coord.carrier_active;
    text::draw_str(
        cv,
        base_x + 2,
        (inner_y + 6) as i16,
        if rad_active {
            "Radio: Active (RX/TX)"
        } else {
            "Radio: Idle (Listening)"
        },
        &FONT,
        Color::Fill,
    );

    let vbat = ctx.power.battery_mv;
    let temp = ctx.power.mcu_temp_deci_c;
    text::draw_str(
        cv,
        base_x + 2,
        (inner_y + 13) as i16,
        "Power: ",
        &FONT,
        Color::Fill,
    );
    let px1 = base_x + 2 + text::str_width("Power: ", &FONT);
    let mut b1 = [0u8; 12];
    let s_vbat = text::format_int_fixed(vbat as i32, 0, "", "mV ", &mut b1);
    text::draw_str(cv, px1, (inner_y + 13) as i16, s_vbat, &FONT, Color::Fill);
    let px2 = px1 + text::str_width(s_vbat, &FONT);
    let mut b2 = [0u8; 10];
    let s_temp = text::format_int_fixed((temp / 10) as i32, 0, "", "C", &mut b2);
    text::draw_str(cv, px2, (inner_y + 13) as i16, s_temp, &FONT, Color::Fill);

    let is_usb = ctx.usb.as_ref().map(|u| u.is_connected()).unwrap_or(false);
    text::draw_str(
        cv,
        base_x + 2,
        (inner_y + 20) as i16,
        if is_usb {
            "Input: 200Hz USB: On"
        } else {
            "Input: 200Hz USB: Off"
        },
        &FONT,
        Color::Fill,
    );

    draw_stat(
        cv,
        base_x + 2,
        (inner_y + 27) as i16,
        "Kernel: Realtime (Procs: ",
        count as u32,
        ")",
    );
}

#[inline(never)]
fn render_info_modal(canvas: &mut Canvas, pid: u8) {
    if let Some(pcb) = TasksApp::proc_table().find(kernel::proc::Pid(pid)) {
        canvas.fill_rect(6, 9, 116, 50, Color::Clear);
        canvas.draw_rect(6, 9, 116, 50, Color::Fill);
        canvas.clear_corners(6, 9, 116, 50);

        canvas.fill_rect(7, 10, 114, 8, Color::Fill);
        text::draw_str(canvas, 10, 16, "Process Telemetry", &FONT, Color::Clear);

        text::draw_str(canvas, 10, 25, "PID: ", &FONT, Color::Fill);
        let mut pbuf = [0u8; 8];
        let pstr = text::format_int_fixed(pcb.pid.0 as i32, 0, "", " ", &mut pbuf);
        text::draw_str(canvas, 26, 25, pstr, &FONT, Color::Fill);
        let nx = 26 + text::str_width(pstr, &FONT);
        text::draw_str(canvas, nx, 25, pcb.name.as_str(), &FONT, Color::Fill);

        let pri_str = match pcb.priority {
            kernel::sched::Priority::Safety => "Safety",
            kernel::sched::Priority::Capture => "Capture",
            kernel::sched::Priority::Radio => "Radio",
            kernel::sched::Priority::Ui => "UI",
            kernel::sched::Priority::Idle => "Idle",
        };
        let stat_str = match pcb.state {
            ProcState::Running => "Running",
            ProcState::Ready => "Ready",
            ProcState::Suspended => "Suspended",
            ProcState::ServiceIdle => "ServiceIdle",
            ProcState::Hibernated => "Hibernated",
            ProcState::Stopped => "Stopped",
            _ => "Dead",
        };
        text::draw_str(canvas, 10, 32, pri_str, &FONT, Color::Fill);
        let sx = 10 + text::str_width(pri_str, &FONT);
        text::draw_str(canvas, sx, 32, " / ", &FONT, Color::Fill);
        let sx2 = sx + text::str_width(" / ", &FONT);
        text::draw_str(canvas, sx2, 32, stat_str, &FONT, Color::Fill);

        draw_stat(canvas, 10, 39, "CPU: ", pcb.ledger.cpu_ticks, "");
        draw_stat(canvas, 64, 39, "IPC: ", pcb.ledger.ipc_events as u32, "");

        draw_stat(
            canvas,
            10,
            46,
            "Stk: ",
            pcb.ledger.stack_watermark as u32,
            "B",
        );
        draw_stat(canvas, 64, 46, "RAM: ", pcb.mem_bytes as u32, "B");

        if pcb.pid.0 > 4 && pcb.app_id < 0xF0 && pcb.app_id != crate::apps::APP_ID_TASKS {
            gfx::widgets::draw_key_badge(canvas, 10, 50, "5", "Hib");
            gfx::widgets::draw_key_badge(canvas, 42, 50, "8", "Kill");
        }
        gfx::widgets::draw_key_badge_right(canvas, 120, 50, "EXIT", "Back");
    }
}
