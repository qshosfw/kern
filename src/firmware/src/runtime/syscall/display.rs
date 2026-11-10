//! Display Subsystem System Call Handlers (`CapDisplayVTable`, `interface_id = 0x0001`).

use super::{active_canvas_opt, active_ctx_opt, parse_str};
use crate::services::primitives::color_from_u8;
use kernel::proc::abi::CapDisplayVTable;
use kernel::proc::primitives::DisplayPrimitive;

pub unsafe extern "C" fn api_display_clear() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.clear();
    }
}

pub unsafe extern "C" fn api_draw_pixel(x: i16, y: i16, color: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.draw_pixel(x, y, color);
    }
}

pub unsafe extern "C" fn api_draw_line(x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.draw_line(x0, y0, x1, y1, color);
    }
}

pub unsafe extern "C" fn api_draw_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.draw_rect(x, y, w, h, color);
    }
}

pub unsafe extern "C" fn api_fill_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.fill_rect(x, y, w, h, color);
    }
}

pub unsafe extern "C" fn api_draw_bitmap(x: i16, y: i16, w: u8, h: u8, data: *const u8, len: u16) {
    if !data.is_null() && len > 0 {
        if let Some(ctx) = active_ctx_opt() {
            let slice = unsafe { core::slice::from_raw_parts(data, len as usize) };
            ctx.draw_bitmap(x, y, w, h, slice);
        }
    }
}

pub unsafe extern "C" fn api_draw_str(
    x: i16,
    y: i16,
    text: *const u8,
    len: u16,
    font: u8,
    color: u8,
) {
    if let Some(s) = unsafe { parse_str(text, len as usize) } {
        if let Some(ctx) = active_ctx_opt() {
            ctx.draw_str(x, y, s, font, color);
        }
    }
}

pub unsafe extern "C" fn api_draw_str_align(
    x: i16,
    y: i16,
    text: *const u8,
    len: u16,
    font: u8,
    color: u8,
    align: u8,
) {
    if let Some(s) = unsafe { parse_str(text, len as usize) } {
        if let Some(ctx) = active_ctx_opt() {
            ctx.draw_str_align(x, y, s, font, color, align);
        }
    }
}

pub unsafe extern "C" fn api_commit_frame() {
    if let Some(ctx) = active_ctx_opt() {
        ctx.commit_frame();
    }
}

pub unsafe extern "C" fn api_commit_rows(row_mask: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.commit_rows(row_mask);
    }
}

pub unsafe extern "C" fn api_get_framebuffer() -> *mut u8 {
    if let Some(ctx) = active_ctx_opt() {
        ctx.get_framebuffer_mut().as_mut_ptr()
    } else {
        core::ptr::null_mut()
    }
}

pub unsafe extern "C" fn api_draw_str_max(
    x: i16,
    y: i16,
    text: *const u8,
    len: u16,
    font: u8,
    color: u8,
    max_w: i16,
) {
    if let Some(s) = unsafe { parse_str(text, len as usize) } {
        if let Some(ctx) = active_ctx_opt() {
            ctx.draw_str_max(x, y, s, font, color, max_w);
        }
    }
}

pub unsafe extern "C" fn api_set_inverted(inverted: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_inverted(inverted != 0);
    }
}

pub unsafe extern "C" fn api_set_contrast(contrast: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.set_contrast(contrast);
    }
}

pub unsafe extern "C" fn api_draw_line_h(x: i16, y: i16, len: i16, color: u8) {
    if len <= 0 || y < 0 || y >= 64 {
        return;
    }
    let col = color_from_u8(color);
    if let Some(canvas) = active_canvas_opt() {
        let x_clamped = x.max(0) as u8;
        let len_clamped = (len.min(128 - x.max(0))) as u8;
        canvas.draw_horizontal_line(x_clamped, y as u8, len_clamped, col);
    }
}

pub unsafe extern "C" fn api_draw_line_v(x: i16, y: i16, len: i16, color: u8) {
    if len <= 0 || x < 0 || x >= 128 {
        return;
    }
    let col = color_from_u8(color);
    if let Some(canvas) = active_canvas_opt() {
        let y_clamped = y.max(0) as u8;
        let len_clamped = (len.min(64 - y.max(0))) as u8;
        canvas.draw_vertical_line(x as u8, y_clamped, len_clamped, col);
    }
}

pub unsafe extern "C" fn api_draw_dotted_line_h(x0: i16, x1: i16, y: i16, step: u8, color: u8) {
    let col = color_from_u8(color);
    if let Some(canvas) = active_canvas_opt() {
        gfx::draw::draw_dotted_line_h(canvas, x0, x1, y, step, col);
    }
}

pub unsafe extern "C" fn api_clear_corners(x: i16, y: i16, w: i16, h: i16) {
    if w <= 0 || h <= 0 || x < 0 || y < 0 {
        return;
    }
    if let Some(canvas) = active_canvas_opt() {
        canvas.clear_corners(x as u8, y as u8, w as u8, h as u8);
    }
}

pub unsafe extern "C" fn api_fill_rect_dither(x: i16, y: i16, w: i16, h: i16, progress: u8) {
    if w <= 0 || h <= 0 || x < 0 || y < 0 {
        return;
    }
    if let Some(canvas) = active_canvas_opt() {
        canvas.fill_rect_dither(x as u8, y as u8, w as u8, h as u8, progress);
    }
}

pub unsafe extern "C" fn api_invert_rect(x: i16, y: i16, w: i16, h: i16) {
    if w <= 0 || h <= 0 || x < 0 || y < 0 {
        return;
    }
    if let Some(canvas) = active_canvas_opt() {
        canvas.fill_rect(x as u8, y as u8, w as u8, h as u8, gfx::Color::Invert);
    }
}

pub unsafe extern "C" fn api_str_width(text: *const u8, len: u16, font: u8) -> i16 {
    if let Some(s) = unsafe { parse_str(text, len as usize) } {
        let f = match font {
            1 => &gfx::fonts::mumatrix_regular::FONT,
            _ => &gfx::fonts::tomthumb::FONT,
        };
        gfx::text::str_width(s, f)
    } else {
        0
    }
}

pub unsafe extern "C" fn api_draw_char(x: i16, y: i16, ch: u8, font: u8, color: u8) -> i16 {
    let col = color_from_u8(color);
    let f = match font {
        1 => &gfx::fonts::mumatrix_regular::FONT,
        _ => &gfx::fonts::tomthumb::FONT,
    };
    if let Some(canvas) = active_canvas_opt() {
        gfx::text::draw_char(canvas, x, y, ch as char, f, col)
    } else {
        4
    }
}

const SYSTEM_ICONS_8: [&[u8; 8]; 27] = [
    &gfx::icons::ICON_FOLDER,
    &gfx::icons::ICON_CH,
    &gfx::icons::ICON_VFO,
    &gfx::icons::ICON_APP_RADIO,
    &gfx::icons::ICON_APP_SETTINGS,
    &gfx::icons::ICON_APP_EXPLORER,
    &gfx::icons::ICON_APP_TORCH,
    &gfx::icons::ICON_APP_DIAG,
    &gfx::icons::ICON_APP_ABOUT,
    &gfx::icons::CHECKBOX_OFF,
    &gfx::icons::CHECKBOX_ON,
    &gfx::icons::RADIO_OFF,
    &gfx::icons::RADIO_ON,
    &gfx::icons::ARROW_RIGHT,
    &gfx::icons::ARROW_LEFT,
    &gfx::icons::SELECT_ARROWS,
    &gfx::icons::ICON_GEAR,
    &gfx::icons::FILE_DOC,
    &gfx::icons::FILE_FOLDER,
    &gfx::icons::ICON_DISPLAY,
    &gfx::icons::ICON_BUTTONS,
    &gfx::icons::ICON_AUDIO,
    &gfx::icons::ICON_BATTERY,
    &gfx::icons::ICON_DEVICE,
    &gfx::icons::ICON_APPS,
    &gfx::icons::ICON_SECURITY,
    &gfx::icons::ICON_SYSTEM,
];

pub fn system_icon_8(icon_id: u8) -> Option<&'static [u8; 8]> {
    let idx = icon_id.wrapping_sub(22) as usize;
    if idx < SYSTEM_ICONS_8.len() {
        Some(SYSTEM_ICONS_8[idx])
    } else {
        None
    }
}

pub unsafe extern "C" fn api_draw_icon(x: i16, y: i16, icon_id: u8, color: u8) {
    let col = color_from_u8(color);
    if let Some(canvas) = active_canvas_opt() {
        if let Some(bits) = system_icon_8(icon_id) {
            gfx::icons::draw_icon_8(canvas, x, y, bits, col);
        } else if let Some(icon) = kernel::canvas::Icon::from_u8(icon_id) {
            gfx::icons::draw_icon(canvas, x, y, icon, col);
        }
    }
}

pub unsafe extern "C" fn api_scroll(dx: i8, dy: i8, fill_byte: u8) {
    if let Some(canvas) = active_canvas_opt() {
        if dy != 0 {
            canvas.scroll_vertical(dy, fill_byte);
        }
        if dx != 0 {
            canvas.scroll_horizontal(dx, fill_byte);
        }
    }
}

pub static DISPLAY_VTABLE: CapDisplayVTable = CapDisplayVTable {
    clear: api_display_clear,
    draw_pixel: api_draw_pixel,
    draw_line: api_draw_line,
    draw_rect: api_draw_rect,
    fill_rect: api_fill_rect,
    draw_bitmap: api_draw_bitmap,
    draw_str: api_draw_str,
    draw_str_align: api_draw_str_align,
    commit_frame: api_commit_frame,
    commit_rows: api_commit_rows,
    get_framebuffer: api_get_framebuffer,
    draw_str_max: api_draw_str_max,
    set_inverted: api_set_inverted,
    set_contrast: api_set_contrast,
    draw_line_h: api_draw_line_h,
    draw_line_v: api_draw_line_v,
    draw_dotted_line_h: api_draw_dotted_line_h,
    clear_corners: api_clear_corners,
    fill_rect_dither: api_fill_rect_dither,
    invert_rect: api_invert_rect,
    str_width: api_str_width,
    draw_char: api_draw_char,
    draw_icon: api_draw_icon,
    scroll: api_scroll,
};
