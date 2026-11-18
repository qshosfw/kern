//! Display Subsystem System Call Handlers (`DisplayApi`, `interface_id = 0x0001`).

use super::{active_canvas_mut, active_ctx_opt, parse_str};
use crate::services::primitives::{color_from_u8, font_from_u8};
use kernel::proc::abi::DisplayApi;

#[inline(always)]
fn canvas_col(color: u8) -> (&'static mut gfx::Canvas, gfx::Color) {
    (active_canvas_mut(), color_from_u8(color))
}

pub unsafe extern "C" fn api_display_clear() {
    active_canvas_mut().clear();
}

pub unsafe extern "C" fn api_draw_pixel(x: i16, y: i16, color: u8) {
    let (c, col) = canvas_col(color);
    c.draw_pixel_i(x, y, col);
}

pub unsafe extern "C" fn api_draw_line(x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    let (c, col) = canvas_col(color);
    gfx::draw::draw_line(c, x0, y0, x1, y1, col);
}

pub unsafe extern "C" fn api_draw_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    let (c, col) = canvas_col(color);
    c.draw_rect_i(x, y, w, h, col);
}

pub unsafe extern "C" fn api_fill_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    let (c, col) = canvas_col(color);
    c.fill_rect_i(x, y, w, h, col);
}

pub unsafe extern "C" fn api_draw_bitmap(x: i16, y: i16, w: u8, h: u8, data: *const u8, len: u16) {
    if let Some(slice) = unsafe { super::ptr_slice(data, len as usize) } {
        gfx::draw::draw_bitmap(active_canvas_mut(), x, y, w, h, slice, gfx::Color::Fill);
    }
}

#[inline(never)]
unsafe fn draw_str_dispatch(xy: u32, text: *const u8, len: u16, cfg: u32) {
    let Some(s) = (unsafe { parse_str(text, len as usize) }) else {
        return;
    };
    let x = xy as i16;
    let y = (xy >> 16) as i16;
    let font = font_from_u8(cfg as u8);
    let col = color_from_u8((cfg >> 8) as u8);
    let mode_and_arg = (cfg >> 16) as i16;
    let canvas = active_canvas_mut();
    if mode_and_arg < 0 {
        let al = match mode_and_arg & 3 {
            1 => gfx::text::Align::Center,
            2 => gfx::text::Align::Right,
            _ => gfx::text::Align::Left,
        };
        gfx::text::draw_str_align(canvas, x, y, s, font, col, al);
    } else {
        gfx::text::draw_str_max_x(canvas, x, y, s, font, col, (x + mode_and_arg).max(0));
    }
}

#[inline(always)]
fn pack_xy(x: i16, y: i16) -> u32 {
    (x as u16 as u32) | ((y as u16 as u32) << 16)
}

pub unsafe extern "C" fn api_draw_str(
    x: i16,
    y: i16,
    text: *const u8,
    len: u16,
    font: u8,
    color: u8,
) {
    unsafe { api_draw_str_align(x, y, text, len, font, color, 0) }
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
    let cfg = (font as u32)
        | ((color as u32) << 8)
        | ((((-16i16 | ((align as i16) & 3)) as u16) as u32) << 16);
    unsafe { draw_str_dispatch(pack_xy(x, y), text, len, cfg) }
}

pub unsafe extern "C" fn api_commit_frame() {
    unsafe { api_commit_rows(0) }
}

pub static mut BOOT_LCD: *mut drivers::st7565::St7565<'static> = core::ptr::null_mut();

pub unsafe extern "C" fn api_commit_rows(row_mask: u8) {
    if let Some(ctx) = active_ctx_opt() {
        ctx.commit_rows(row_mask);
    } else if unsafe { !BOOT_LCD.is_null() } {
        unsafe { (*BOOT_LCD).blit(active_canvas_mut()) };
    }
}

pub unsafe extern "C" fn api_get_framebuffer() -> *mut u8 {
    let canvas = active_canvas_mut();
    canvas.mark_all_dirty();
    canvas.buffer.as_mut_ptr() as *mut u8
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
    let cfg = (font as u32) | ((color as u32) << 8) | (((max_w.max(0) as u16) as u32) << 16);
    unsafe { draw_str_dispatch(pack_xy(x, y), text, len, cfg) }
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
    let (c, col) = canvas_col(color);
    c.draw_horizontal_line_i(x, y, len, col);
}

pub unsafe extern "C" fn api_draw_line_v(x: i16, y: i16, len: i16, color: u8) {
    let (c, col) = canvas_col(color);
    c.draw_vertical_line_i(x, y, len, col);
}

pub unsafe extern "C" fn api_draw_dotted_line_h(x0: i16, x1: i16, y: i16, step: u8, color: u8) {
    let (c, col) = canvas_col(color);
    gfx::draw::draw_dotted_line_h(c, x0, y, x1, step, col);
}

pub unsafe extern "C" fn api_clear_corners(x: i16, y: i16, w: i16, h: i16) {
    active_canvas_mut().clear_corners_i(x, y, w, h);
}

pub unsafe extern "C" fn api_fill_rect_dither(x: i16, y: i16, w: i16, h: i16, progress: u8) {
    active_canvas_mut().fill_rect_dither_i(x, y, w, h, progress);
}

pub unsafe extern "C" fn api_invert_rect(x: i16, y: i16, w: i16, h: i16) {
    active_canvas_mut().fill_rect_i(x, y, w, h, gfx::Color::Invert);
}

pub unsafe extern "C" fn api_str_width(text: *const u8, len: u16, font: u8) -> i16 {
    if let Some(s) = unsafe { parse_str(text, len as usize) } {
        gfx::text::str_width(s, font_from_u8(font))
    } else {
        0
    }
}

pub unsafe extern "C" fn api_draw_char(x: i16, y: i16, ch: u8, font: u8, color: u8) -> i16 {
    gfx::text::draw_char(
        active_canvas_mut(),
        x,
        y,
        ch as char,
        font_from_u8(font),
        color_from_u8(color),
    )
}

pub fn system_icon_8(icon_id: u8) -> Option<&'static [u8; 8]> {
    gfx::icons::SYSTEM_ICONS_8.get(icon_id.wrapping_sub(22) as usize)
}

pub unsafe extern "C" fn api_draw_icon(x: i16, y: i16, icon_id: u8, color: u8) {
    let col = color_from_u8(color);
    let canvas = active_canvas_mut();
    if let Some(bits) = system_icon_8(icon_id) {
        gfx::icons::draw_icon_8(canvas, x, y, bits, col);
    } else if let Some(icon) = kernel::canvas::Icon::from_u8(icon_id) {
        gfx::icons::draw_icon(canvas, x, y, icon, col);
    }
}

pub unsafe extern "C" fn api_scroll(dx: i8, dy: i8, fill_byte: u8) {
    let canvas = active_canvas_mut();
    if dy != 0 {
        canvas.scroll_vertical(dy, fill_byte);
    }
    if dx != 0 {
        canvas.scroll_horizontal(dx, fill_byte);
    }
}

pub static DISPLAY_VTABLE: DisplayApi = DisplayApi {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_framebuffer_marks_all_pages_dirty() {
        let canvas = active_canvas_mut();
        canvas.clear();
        canvas.dirty = 0;
        let fb = unsafe { api_get_framebuffer() };
        assert_eq!(fb, canvas.buffer.as_mut_ptr() as *mut u8);
        assert_eq!(canvas.dirty, 0xFF);
    }
}
