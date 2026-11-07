//! Display Subsystem System Call Handlers (`CapDisplayVTable`, `interface_id = 0x0001`).

use super::{active_ctx, parse_str};
use kernel::proc::abi::CapDisplayVTable;
use kernel::proc::primitives::DisplayPrimitive;

pub unsafe extern "C" fn api_display_clear() {
    unsafe { active_ctx().clear() }
}

pub unsafe extern "C" fn api_draw_pixel(x: i16, y: i16, color: u8) {
    unsafe { active_ctx().draw_pixel(x, y, color) }
}

pub unsafe extern "C" fn api_draw_line(x0: i16, y0: i16, x1: i16, y1: i16, color: u8) {
    unsafe { active_ctx().draw_line(x0, y0, x1, y1, color) }
}

pub unsafe extern "C" fn api_draw_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    unsafe { active_ctx().draw_rect(x, y, w, h, color) }
}

pub unsafe extern "C" fn api_fill_rect(x: i16, y: i16, w: i16, h: i16, color: u8) {
    unsafe { active_ctx().fill_rect(x, y, w, h, color) }
}

pub unsafe extern "C" fn api_draw_bitmap(x: i16, y: i16, w: u8, h: u8, data: *const u8, len: u16) {
    if !data.is_null() && len > 0 {
        let slice = unsafe { core::slice::from_raw_parts(data, len as usize) };
        unsafe { active_ctx().draw_bitmap(x, y, w, h, slice) };
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
        unsafe { active_ctx().draw_str(x, y, s, font, color) };
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
        unsafe { active_ctx().draw_str_align(x, y, s, font, color, align) };
    }
}

pub unsafe extern "C" fn api_commit_frame() {
    unsafe { active_ctx().commit_frame() }
}

pub unsafe extern "C" fn api_commit_rows(row_mask: u8) {
    unsafe { active_ctx().commit_rows(row_mask) }
}

pub unsafe extern "C" fn api_get_framebuffer() -> *mut u8 {
    unsafe { active_ctx().get_framebuffer_mut().as_mut_ptr() }
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
        unsafe { active_ctx().draw_str_max(x, y, s, font, color, max_w) };
    }
}

pub unsafe extern "C" fn api_set_inverted(inverted: u8) {
    unsafe { active_ctx().set_inverted(inverted != 0) };
}

pub unsafe extern "C" fn api_set_contrast(contrast: u8) {
    unsafe { active_ctx().set_contrast(contrast) };
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
};
