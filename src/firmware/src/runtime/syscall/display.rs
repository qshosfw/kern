//! Display & Graphics System Call Dispatcher (`Op::Gfx*`, domain `0x6`).

use super::{active_canvas_mut, parse_str, ptr_slice};
use crate::services::primitives::{color_from_u8, font_from_u8};
use kernel::proc::abi::{KrxTextReq, KrxWidget};

pub fn system_icon_8(icon_id: u8) -> Option<&'static [u8; 8]> {
    gfx::icons::SYSTEM_ICONS_8.get(icon_id.wrapping_sub(22) as usize)
}

#[inline(never)]
pub fn draw_icon(x: i16, y: i16, icon_id: u8, color: u8) {
    let col = color_from_u8(color);
    let canvas = active_canvas_mut();
    if let Some(bits) = system_icon_8(icon_id) {
        gfx::icons::draw_icon_8(canvas, x, y, bits, col);
    } else if let Some(icon) = kernel::canvas::Icon::from_u8(icon_id) {
        gfx::icons::draw_icon(canvas, x, y, icon, col);
    }
}

#[inline(never)]
pub unsafe fn draw_text_req(req: *const KrxTextReq) -> i16 {
    if req.is_null() {
        return 0;
    }
    let r = unsafe { &*req };
    let Some(s) = (unsafe { parse_str(r.text, r.len as usize) }) else {
        return 0;
    };
    let font = font_from_u8(r.font);
    if (r.flags & 1) != 0 {
        return gfx::text::str_width(s, font);
    }
    let col = color_from_u8(r.color);
    let canvas = active_canvas_mut();
    if (r.flags & 2) != 0 {
        gfx::text::draw_str_max_x(canvas, r.x, r.y, s, font, col, (r.x + r.arg).max(0));
    } else {
        let al = match r.arg & 3 {
            1 => gfx::text::Align::Center,
            2 => gfx::text::Align::Right,
            _ => gfx::text::Align::Left,
        };
        gfx::text::draw_str_align(canvas, r.x, r.y, s, font, col, al);
    }
    0
}

#[inline(never)]
pub unsafe fn dispatch_gfx(idx: u8, a0: usize, a1: usize, a2: usize) -> usize {
    let canvas = active_canvas_mut();
    match idx {
        0 => {
            canvas.clear();
            0
        }
        1 => {
            let sub = a0 as u8;
            let col_u8 = (a0 >> 8) as u8;
            let extra = (a0 >> 16) as u8;
            let x = a1 as i16;
            let y = (a1 >> 16) as i16;
            let w = a2 as i16;
            let h = (a2 >> 16) as i16;
            let col = color_from_u8(col_u8);
            match sub {
                0 => canvas.draw_pixel_i(x, y, col),
                1 => gfx::draw::draw_line(canvas, x, y, w, h, col),
                2 => canvas.draw_rect_i(x, y, w, h, col),
                3 => canvas.fill_rect_i(x, y, w, h, col),
                4 => canvas.draw_horizontal_line_i(x, y, w, col),
                5 => canvas.draw_vertical_line_i(x, y, w, col),
                6 => canvas.clear_corners_i(x, y, w, h),
                7 => canvas.fill_rect_dither_i(x, y, w, h, extra),
                8 => canvas.fill_rect_i(x, y, w, h, gfx::Color::Invert),
                9 => {
                    return gfx::text::draw_char(
                        canvas,
                        x,
                        y,
                        a2 as u8 as char,
                        font_from_u8(extra),
                        col,
                    ) as u16 as usize;
                }
                10 => draw_icon(x, y, a2 as u8, col_u8),
                11 => {
                    let dx = a1 as i8;
                    let dy = (a1 >> 8) as i8;
                    if dy != 0 {
                        canvas.scroll_vertical(dy, col_u8);
                    }
                    if dx != 0 {
                        canvas.scroll_horizontal(dx, col_u8);
                    }
                }
                12 => gfx::draw::draw_dotted_line_h(canvas, x, y, w, extra, col),
                _ => {}
            }
            0
        }
        2 => unsafe { draw_text_req(a0 as *const KrxTextReq) as u16 as usize },
        3 => {
            let row_mask = a0 as u8;
            canvas.dirty |= row_mask;
            #[cfg(not(test))]
            crate::kern_hw().lcd.blit(canvas);
            #[cfg(test)]
            if let Some(ctx) = super::active_ctx_opt() {
                if let Some(ref mut d) = ctx.display {
                    d.canvas.dirty |= row_mask;
                    d.lcd.blit(d.canvas);
                }
            }
            0
        }
        4 => {
            canvas.mark_all_dirty();
            canvas.buffer.as_mut_ptr() as usize
        }
        5 => {
            match a0 as u8 {
                0 => {
                    #[cfg(not(test))]
                    crate::kern_hw().lcd.set_inverted(a1 != 0);
                    #[cfg(test)]
                    if let Some(ctx) = super::active_ctx_opt() {
                        if let Some(ref mut d) = ctx.display {
                            kernel::service::DisplayService::set_inverted(*d, a1 != 0);
                        }
                    }
                }
                1 => {
                    #[cfg(not(test))]
                    crate::kern_hw().lcd.set_contrast(a1 as u8);
                    #[cfg(test)]
                    if let Some(ctx) = super::active_ctx_opt() {
                        if let Some(ref mut d) = ctx.display {
                            kernel::service::DisplayService::set_contrast(*d, a1 as u8);
                        }
                    }
                }
                2 => {
                    let desc = a1 as *const KrxWidget;
                    if !desc.is_null() {
                        let d = unsafe { &*desc };
                        if let Some(slice) = unsafe { ptr_slice(d.p0, d.len0 as usize) } {
                            gfx::draw::draw_bitmap(
                                canvas,
                                d.x,
                                d.y,
                                d.w,
                                d.h,
                                slice,
                                gfx::Color::Fill,
                            );
                        }
                    }
                }
                _ => {}
            }
            0
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::abi::Op;

    #[test]
    fn get_framebuffer_marks_all_pages_dirty() {
        let canvas = active_canvas_mut();
        canvas.clear();
        canvas.dirty = 0;
        let fb = unsafe { super::super::kern_call(Op::GfxFb.u32_id(), 0, 0, 0) } as *mut u8;
        assert_eq!(fb, canvas.buffer.as_mut_ptr() as *mut u8);
        assert_eq!(canvas.dirty, 0xFF);
    }
}
