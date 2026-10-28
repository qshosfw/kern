//! Dynamic Package Walker Runtime Adapter (KRX-ABI 2.5.0 Chapter 10 — Declare 3.0).
//!
//! Provides the OS execution bridge and `AppVTable` for running Declare 3.0 programs
//! via the kernel's `DeclareWalker` engine without duplicating framework code.

use crate::services::FirmwareContext;
use crate::services::power::PowerService;
use gfx::Canvas;
use gfx::Color;
use kernel::app::{AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::event::{Event, KeyCode, KeyState};
use kernel::proc::declare::{DeclareWalker, ITEM_INFO, ITEM_TOGGLE, ITEM_VALUE, TRIGGER_BOOT};

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
        if *code == KeyCode::Exit && walker.current_node_idx == 0 {
            ctx.post_event(crate::apps::Shortcut::LaunchApp(0).into());
            return EventResult::Consumed;
        }

        if walker.handle_key(*code) {
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
fn draw_walker_item(
    canvas: &mut Canvas,
    walker: &DeclareWalker,
    ctx: &FirmwareContext<'static, 'static>,
    item: kernel::proc::declare::DeclareItem,
    i: usize,
    font: &gfx::GfxFont,
) {
    let y = 14 + (i as i16) * 8;
    let is_selected = i == walker.selected_item;

    if is_selected {
        canvas.fill_rect(1, (y - 5).max(0) as u8, 126, 7, Color::Fill);
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
    match item.kind {
        ITEM_VALUE => {
            let val = walker.state.get_i32(item.value_ref);
            let val_str = gfx::text::format_u32(val.max(0) as u32, &mut nbuf);
            let val_w = gfx::text::str_width(val_str, font);
            gfx::text::draw_str(canvas, 122 - (val_w as i16), y, val_str, font, text_color);
        }
        ITEM_TOGGLE => {
            let state = walker.state.get_bool(item.value_ref);
            let t_str = if state { "[ON]" } else { "[OFF]" };
            let t_w = gfx::text::str_width(t_str, font);
            gfx::text::draw_str(canvas, 122 - (t_w as i16), y, t_str, font, text_color);
        }
        ITEM_INFO => {
            let mut vbuf = [0u8; 12];
            let info_str = match item.value_ref {
                0 => gfx::text::format_voltage(ctx.power.battery_voltage_mv(), 2, &mut vbuf),
                1 => {
                    if ctx.config.battery.cpu_clock == 1 {
                        "72MHz"
                    } else {
                        "48MHz"
                    }
                }
                2 => gfx::text::format_u32(
                    kernel::sys::SystemDiagnostics::stack_margin_bytes() as u32,
                    &mut nbuf,
                ),
                3 => gfx::text::format_u32(
                    kernel::sys::SystemDiagnostics::stack_peak_used_bytes() as u32,
                    &mut nbuf,
                ),
                4 => gfx::text::format_u32(ctx.power.uptime_secs(), &mut nbuf),
                5 => gfx::text::format_u32(ctx.proc_table().active_count() as u32, &mut nbuf),
                _ => "",
            };
            let info_w = gfx::text::str_width(info_str, font);
            gfx::text::draw_str(canvas, 122 - (info_w as i16), y, info_str, font, text_color);
        }
        _ => {}
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

    canvas.clear();

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

    // 2. Body List View (y=12..52)
    let start_item = node.first_item_offset as usize;
    let count = (node.item_count as usize).min(5);

    for i in 0..count {
        let global_idx = start_item + i;
        if let Some(it) = walker.items.get(global_idx) {
            draw_walker_item(canvas, walker, ctx, *it, i, font);
        }
    }

    // 3. Footer (y=54..63)
    canvas.draw_horizontal_line(0, 53, 128, Color::Fill);
    gfx::widgets::draw_key_badge(canvas, 2, 55, "M", "Select");
    gfx::widgets::draw_key_badge_right(canvas, 126, 55, "EXIT", "Back");
}

pub unsafe fn walker_on_exit(_ptr: *mut (), _ctx: &mut FirmwareContext<'static, 'static>) {}

pub const DECLARE_WALKER_VTABLE: AppVTable<FirmwareContext<'static, 'static>> = AppVTable {
    on_start: walker_on_start,
    on_event: walker_on_event,
    on_modal_result: walker_on_modal_result,
    render: walker_render,
    on_exit: walker_on_exit,
};
