//! Dynamic Package Walker Runtime Adapter (KRX-ABI 2.5.0 Chapter 10 — Declare 3.0).
//!
//! Provides the OS execution bridge and `AppVTable` for running Declare 3.0 programs
//! via the kernel's `DeclareWalker` engine without duplicating framework code.
//! Supports declarative ABI telemetry data binding, interactive forms, menu scrolling,
//! info cards, and multi-level node navigation.

use crate::services::FirmwareContext;
use gfx::Canvas;
use gfx::Color;
use kernel::app::{AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};
use kernel::proc::declare::{
    DeclareWalker, DYN_ACTIVE_PROCS, DYN_BATTERY_MV, DYN_BATTERY_PCT, DYN_CPU_CLOCK,
    DYN_RADIO_FREQ_A, DYN_RADIO_FREQ_B, DYN_RADIO_RSSI, DYN_RAM_FREE, DYN_RAM_USED,
    DYN_STACK_MARGIN, DYN_STACK_PEAK, DYN_STORAGE_FREE, DYN_SWAP_USAGE, DYN_UPTIME,
    FLAG_DYNAMIC, FLAG_READONLY, ITEM_ACTION, ITEM_INFO, ITEM_LINK, ITEM_SUBMENU, ITEM_TOGGLE,
    ITEM_VALUE, LAYOUT_FORM, LAYOUT_INFO_CARD, LAYOUT_MENU, LAYOUT_SCROLL_TEXT, TRIGGER_BOOT,
};

#[inline(never)]
pub fn get_global_walker() -> &'static mut DeclareWalker {
    unsafe {
        let walker_ptr = super::native_runner::arena_base_ptr() as *mut DeclareWalker;
        core::ptr::write_bytes(
            walker_ptr as *mut u8,
            0,
            core::mem::size_of::<DeclareWalker>(),
        );
        (*walker_ptr).app_name = "App";
        &mut *walker_ptr
    }
}

pub unsafe fn walker_on_start(ptr: *mut (), _ctx: &mut FirmwareContext<'static, 'static>) {
    let walker = unsafe { &mut *(ptr as *mut DeclareWalker) };
    walker.current_node_idx = 0;
    walker.selected_item = 0;
    walker.scroll_top = 0;
    walker.nav_stack.clear();
    walker.trigger_rules(TRIGGER_BOOT, 0);
}

pub unsafe fn walker_on_event(
    ptr: *mut (),
    ctx: &mut FirmwareContext<'static, 'static>,
    ev: &Event,
) -> EventResult {
    let walker = unsafe { &mut *(ptr as *mut DeclareWalker) };

    if let Event::Key {
        code,
        state: KeyState::Pressed,
    } = ev
    {
        if *code == KeyCode::Exit && walker.current_node_idx == 0 && walker.nav_stack.is_empty() {
            ctx.post_event(crate::apps::Shortcut::LaunchApp(0).into());
            return EventResult::Consumed;
        }

        if walker.handle_key(*code) {
            EventResult::Consumed
        } else if *code == KeyCode::Exit {
            ctx.post_event(crate::apps::Shortcut::LaunchApp(0).into());
            EventResult::Consumed
        } else {
            EventResult::Ignored
        }
    } else {
        EventResult::Ignored
    }
}

pub unsafe fn walker_on_modal_result(
    _ptr: *mut (),
    _ctx: &mut FirmwareContext<'static, 'static>,
    _r: &ModalResult<'_>,
) -> EventResult {
    EventResult::Consumed
}

#[inline(never)]
fn resolve_telemetry_str<'a>(
    ctx: &FirmwareContext<'static, 'static>,
    telem_id: u16,
    buf: &'a mut [u8; 16],
) -> &'a str {
    let mut tmp = [0u8; 16];
    let len = match telem_id {
        DYN_BATTERY_MV => {
            let mut vbuf = [0u8; 12];
            let v = gfx::text::format_voltage(ctx.power.battery_mv, 2, &mut vbuf);
            let l = v.len().min(16);
            buf[..l].copy_from_slice(&v.as_bytes()[..l]);
            return core::str::from_utf8(&buf[..l]).unwrap_or("");
        }
        DYN_BATTERY_PCT => {
            let pct = ctx.power.battery_pct.min(100);
            let s = gfx::text::format_u32(pct as u32, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            if l < 15 {
                buf[l] = b'%';
                l + 1
            } else {
                l
            }
        }
        DYN_CPU_CLOCK => {
            if ctx.config.battery.cpu_clock == 1 {
                return "72MHz";
            } else {
                return "48MHz";
            }
        }
        DYN_RAM_FREE => {
            let free_kb = kernel::sys::SystemDiagnostics::ram_available_bytes() >> 10;
            let s = gfx::text::format_u32(free_kb as u32, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            if l < 14 {
                buf[l] = b' ';
                buf[l + 1] = b'K';
                l + 2
            } else {
                l
            }
        }
        DYN_RAM_USED => {
            let used_kb = kernel::sys::SystemDiagnostics::ram_static_used_bytes() >> 10;
            let s = gfx::text::format_u32(used_kb as u32, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            l
        }
        DYN_STACK_MARGIN => {
            let val = kernel::sys::SystemDiagnostics::stack_margin_bytes() as u32;
            let s = gfx::text::format_u32(val, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            l
        }
        DYN_STACK_PEAK => {
            let val = kernel::sys::SystemDiagnostics::stack_peak_used_bytes() as u32;
            let s = gfx::text::format_u32(val, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            l
        }
        DYN_UPTIME => {
            let s = gfx::text::format_u32(ctx.power.uptime_secs, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            if l < 15 {
                buf[l] = b's';
                l + 1
            } else {
                l
            }
        }
        DYN_ACTIVE_PROCS => {
            let s = gfx::text::format_u32(ctx.proc_table().active_count() as u32, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            l
        }
        DYN_STORAGE_FREE => {
            #[cfg(feature = "filesystem")]
            let free_kb = (ctx.storage.stats().free_sectors as u32) * 4;
            #[cfg(not(feature = "filesystem"))]
            let free_kb = 0;
            let s = gfx::text::format_u32(free_kb, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            l
        }
        DYN_RADIO_RSSI => {
            let rssi = ctx.state.rssi_dbm;
            let mut sbuf = [0u8; 12];
            let num = gfx::text::format_i32(rssi as i32, &mut sbuf);
            let l = num.len();
            buf[..l].copy_from_slice(num.as_bytes());
            if l + 3 <= 16 {
                buf[l..l + 3].copy_from_slice(b"dBm");
                l + 3
            } else {
                l
            }
        }
        DYN_RADIO_FREQ_A => {
            let mhz = ctx.state.vfo_a.frequency.raw() / 1_000_000;
            let s = gfx::text::format_u32(mhz, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            l
        }
        DYN_RADIO_FREQ_B => {
            let mhz = ctx.state.vfo_b.frequency.raw() / 1_000_000;
            let s = gfx::text::format_u32(mhz, &mut tmp);
            let l = s.len();
            buf[..l].copy_from_slice(s.as_bytes());
            l
        }
        DYN_SWAP_USAGE => {
            return "0K";
        }
        _ => return "",
    };

    core::str::from_utf8(&buf[..len]).unwrap_or("")
}

#[inline(never)]
fn draw_walker_item(
    canvas: &mut Canvas,
    walker: &DeclareWalker,
    ctx: &FirmwareContext<'static, 'static>,
    item: kernel::proc::declare::DeclareItem,
    row_idx: usize,
    font: &gfx::GfxFont,
) {
    let y = 15 + (row_idx as i16) * 8;
    let is_selected = (walker.scroll_top + row_idx) == walker.selected_item;

    if is_selected {
        canvas.fill_rect(2, (y - 5).max(0) as u8, 122, 7, Color::Fill);
        canvas.clear_corners(2, (y - 5).max(0) as u8, 122, 7);
    }

    let text_color = if is_selected {
        Color::Clear
    } else {
        Color::Fill
    };

    let label = walker
        .labels
        .get(item.label_offset as usize)
        .copied()
        .unwrap_or("Item");
    gfx::text::draw_str(canvas, 6, y, label, font, text_color);

    let mut nbuf = [0u8; 16];
    let mut adj_buf = [0u8; 16];
    let val_str = match item.kind {
        ITEM_VALUE => {
            let val = walker.state.get_i32(item.value_ref);
            let raw_num = gfx::text::format_u32(val.max(0) as u32, &mut nbuf);
            if is_selected {
                adj_buf[0] = b'<';
                adj_buf[1] = b' ';
                let n_len = raw_num.len().min(11);
                adj_buf[2..2 + n_len].copy_from_slice(&raw_num.as_bytes()[..n_len]);
                adj_buf[2 + n_len] = b' ';
                adj_buf[3 + n_len] = b'>';
                core::str::from_utf8(&adj_buf[..4 + n_len]).unwrap_or(raw_num)
            } else {
                raw_num
            }
        }
        ITEM_TOGGLE => {
            let state = walker.state.get_bool(item.value_ref);
            if state { "[ON]" } else { "[OFF]" }
        }
        ITEM_ACTION => ">",
        ITEM_SUBMENU => ">>",
        ITEM_LINK => "->",
        ITEM_INFO => {
            if item.value_ref >= 0x0100 {
                resolve_telemetry_str(ctx, item.value_ref, &mut nbuf)
            } else if (item.value_ref as usize) < walker.labels.len()
                && (item.flags & FLAG_READONLY != 0 || item.flags & FLAG_DYNAMIC == 0)
            {
                walker
                    .labels
                    .get(item.value_ref as usize)
                    .copied()
                    .unwrap_or("")
            } else {
                // Fallback for legacy 0..5 and state values
                match item.value_ref {
                    0 => resolve_telemetry_str(ctx, DYN_BATTERY_MV, &mut nbuf),
                    1 => resolve_telemetry_str(ctx, DYN_CPU_CLOCK, &mut nbuf),
                    2 => resolve_telemetry_str(ctx, DYN_STACK_MARGIN, &mut nbuf),
                    3 => resolve_telemetry_str(ctx, DYN_STACK_PEAK, &mut nbuf),
                    4 => resolve_telemetry_str(ctx, DYN_UPTIME, &mut nbuf),
                    5 => resolve_telemetry_str(ctx, DYN_ACTIVE_PROCS, &mut nbuf),
                    _ => {
                        let val = walker.state.get_i32(item.value_ref);
                        gfx::text::format_u32(val.max(0) as u32, &mut nbuf)
                    }
                }
            }
        }
        _ => "",
    };

    if !val_str.is_empty() {
        let val_w = gfx::text::str_width(val_str, font) as i16;
        gfx::text::draw_str(canvas, 122 - val_w, y, val_str, font, text_color);
    }
}

pub unsafe fn walker_render(
    ptr: *const (),
    canvas: &mut Canvas,
    ctx: &FirmwareContext<'static, 'static>,
    sb: &mut StatusBarConfig<'_>,
) {
    let walker = unsafe { &*(ptr as *const DeclareWalker) };
    let font = &gfx::fonts::tomthumb::FONT;

    let node = if let Some(n) = walker.nodes.get(walker.current_node_idx) {
        *n
    } else {
        gfx::text::draw_str(canvas, 10, 30, "No Node Data", font, Color::Fill);
        return;
    };

    let title = walker
        .labels
        .get(node.title_offset as usize)
        .copied()
        .unwrap_or("");

    *sb = StatusBarConfig {
        visible: true,
        title: Some(walker.app_name),
        center_text: if !title.is_empty() { Some(title) } else { None },
        signal_level: None,
        left_badge: None,
        show_divider: true,
    };

    let total_items = node.item_count as usize;
    let visible_count = 5usize;

    match node.layout {
        LAYOUT_INFO_CARD => {
            // Boxed System Card View
            canvas.draw_rect(2, 9, 124, 43, Color::Fill);
            canvas.clear_corners(2, 9, 124, 43);
            canvas.draw_horizontal_line(2, 17, 124, Color::DarkGray);

            if !title.is_empty() {
                gfx::text::draw_str(canvas, 6, 15, title, font, Color::Fill);
            }

            let start_item = node.first_item_offset as usize;
            let display_count = total_items.min(4);
            for i in 0..display_count {
                let global_idx = start_item + i;
                if let Some(it) = walker.items.get(global_idx) {
                    let y = 24 + (i as i16) * 8;
                    let label = walker
                        .labels
                        .get(it.label_offset as usize)
                        .copied()
                        .unwrap_or("Info");
                    gfx::text::draw_str(canvas, 6, y, label, font, Color::Fill);

                    let mut nbuf = [0u8; 16];
                    let val = if it.value_ref >= 0x0100 {
                        resolve_telemetry_str(ctx, it.value_ref, &mut nbuf)
                    } else {
                        match it.value_ref {
                            0 => resolve_telemetry_str(ctx, DYN_BATTERY_MV, &mut nbuf),
                            1 => resolve_telemetry_str(ctx, DYN_CPU_CLOCK, &mut nbuf),
                            2 => resolve_telemetry_str(ctx, DYN_STACK_MARGIN, &mut nbuf),
                            3 => resolve_telemetry_str(ctx, DYN_STACK_PEAK, &mut nbuf),
                            4 => resolve_telemetry_str(ctx, DYN_UPTIME, &mut nbuf),
                            5 => resolve_telemetry_str(ctx, DYN_ACTIVE_PROCS, &mut nbuf),
                            _ => "",
                        }
                    };
                    if !val.is_empty() {
                        let w = gfx::text::str_width(val, font) as i16;
                        gfx::text::draw_str(canvas, 122 - w, y, val, font, Color::Fill);
                    }
                }
            }
        }
        LAYOUT_SCROLL_TEXT => {
            // Text Document Reader View
            let start_item = node.first_item_offset as usize;
            let display_count = total_items.saturating_sub(walker.scroll_top).min(visible_count);
            for i in 0..display_count {
                let global_idx = start_item + walker.scroll_top + i;
                if let Some(it) = walker.items.get(global_idx) {
                    let y = 15 + (i as i16) * 8;
                    let line = walker
                        .labels
                        .get(it.label_offset as usize)
                        .copied()
                        .unwrap_or("");
                    gfx::text::draw_str(canvas, 4, y, line, font, Color::Fill);
                }
            }
        }
        LAYOUT_MENU | LAYOUT_FORM | _ => {
            // LAYOUT_MENU and LAYOUT_FORM: standard interactive list with scrolling & selection
            let start_item = node.first_item_offset as usize;
            let display_count = total_items.saturating_sub(walker.scroll_top).min(visible_count);

            for i in 0..display_count {
                let global_idx = start_item + walker.scroll_top + i;
                if let Some(it) = walker.items.get(global_idx) {
                    draw_walker_item(canvas, walker, ctx, *it, i, font);
                }
            }

            // Minimalist vertical scrollbar when total items exceed 5 visible rows
            if total_items > visible_count {
                let track_h = 42u8;
                let thumb_h = ((visible_count as u32 * track_h as u32) / total_items as u32).max(4) as u8;
                let max_scroll = (total_items - visible_count) as u32;
                let thumb_offset = if max_scroll > 0 {
                    ((walker.scroll_top as u32 * (track_h - thumb_h) as u32) / max_scroll) as u8
                } else {
                    0
                };
                let thumb_y = 10u8 + thumb_offset;
                canvas.draw_vertical_line(126, 10, track_h, Color::DarkGray);
                canvas.fill_rect(125, thumb_y, 3, thumb_h, Color::Fill);
            }
        }
    }

    // 3. Footer Bar (y=54..63)
    canvas.draw_horizontal_line(0, 53, 128, Color::Fill);

    let sel_idx = (node.first_item_offset as usize) + walker.selected_item;
    let sel_kind = walker.items.get(sel_idx).map(|it| it.kind).unwrap_or(ITEM_ACTION);

    let m_action = match sel_kind {
        ITEM_TOGGLE => "Toggle",
        ITEM_VALUE => "Adjust",
        ITEM_SUBMENU => "Open",
        _ => "Select",
    };
    gfx::widgets::draw_key_badge(canvas, 2, 55, "M", m_action);

    let back_label = if walker.nav_stack.is_empty() && walker.current_node_idx == 0 {
        "Home"
    } else {
        "Back"
    };
    gfx::widgets::draw_key_badge_right(canvas, 126, 55, "EXIT", back_label);
}

pub unsafe fn walker_on_exit(_ptr: *mut (), _ctx: &mut FirmwareContext<'static, 'static>) {}

pub const DECLARE_WALKER_VTABLE: AppVTable<FirmwareContext<'static, 'static>> = AppVTable {
    on_start: walker_on_start,
    on_event: walker_on_event,
    on_modal_result: walker_on_modal_result,
    render: walker_render,
    on_exit: walker_on_exit,
};
