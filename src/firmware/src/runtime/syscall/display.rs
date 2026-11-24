//! Display & Graphics System Call Dispatcher (`Op::Gfx*`, domain `0x6`).

use super::{active_canvas_mut, parse_str, ptr_slice};
use crate::services::primitives::{color_from_u8, font_from_u8};
use kernel::proc::abi::{KrxTextReq, KrxWidget};

pub fn system_icon_8(icon_id: u8) -> Option<&'static [u8; 8]> {
    gfx::icons::system_icon(icon_id)
}

#[inline(never)]
pub fn draw_icon(x: i16, y: i16, icon_id: u8, color: u8) {
    if icon_id == kernel::proc::abi::KRX_ICON_NONE {
        return;
    }
    let col = color_from_u8(color);
    let canvas = active_canvas_mut();
    if let Some(bits) = system_icon_8(icon_id) {
        gfx::icons::draw_icon_8(canvas, x, y, bits, col);
    } else if let Some(icon) = kernel::ui::Icon::from_u8(icon_id) {
        gfx::icons::draw_icon(canvas, x, y, icon, col);
    } else {
        gfx::icons::draw_icon_8(canvas, x, y, gfx::icons::FALLBACK_QMARK_8, col);
    }
}

#[inline(never)]
pub unsafe fn draw_text_req(req: *const KrxTextReq) -> i16 {
    let r = unsafe { &*req };
    let Some(raw) = (unsafe { parse_str(r.text, r.len as usize) }) else {
        return 0;
    };
    let s = if (r.flags & kernel::proc::abi::KRX_TEXT_LOCALIZE) != 0 {
        gfx::text::localize_scratch(raw, (r.flags & kernel::proc::abi::KRX_TEXT_SHORT) != 0)
    } else {
        raw
    };
    let font = font_from_u8(r.font);
    if (r.flags & kernel::proc::abi::KRX_TEXT_MEASURE) != 0 {
        return gfx::text::str_width(s, font);
    }
    let col = color_from_u8(r.color);
    let canvas = active_canvas_mut();
    if (r.flags & kernel::proc::abi::KRX_TEXT_CLIP) != 0 {
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
    use abi::{
        idx as op,
        sub::{gfx_ctl, gfx_draw},
    };
    let canvas = active_canvas_mut();
    match idx {
        op::GfxClear => {
            canvas.clear();
            0
        }
        op::GfxDraw => {
            let sub = a0 as u8;
            let col_u8 = (a0 >> 8) as u8;
            let extra = (a0 >> 16) as u8;
            let x = a1 as i16;
            let y = (a1 >> 16) as i16;
            let w = a2 as i16;
            let h = (a2 >> 16) as i16;
            let col = color_from_u8(col_u8);
            match sub {
                gfx_draw::LINE => gfx::draw::draw_line(canvas, x, y, w, h, col),
                gfx_draw::RECT => canvas.draw_rect_i(x, y, w, h, col),
                gfx_draw::FILL_RECT => canvas.fill_rect_i(x, y, w, h, col),
                gfx_draw::CLR_CORNER => canvas.clear_corners_i(x, y, w, h),
                gfx_draw::DITHER => canvas.fill_rect_dither_i(x, y, w, h, extra),
                gfx_draw::CHAR => {
                    return gfx::text::draw_char(
                        canvas,
                        x,
                        y,
                        a2 as u8 as char,
                        font_from_u8(extra),
                        col,
                    ) as u16 as usize;
                }
                gfx_draw::ICON => draw_icon(x, y, a2 as u8, col_u8),
                gfx_draw::SCROLL => {
                    let dx = a1 as i8;
                    let dy = (a1 >> 8) as i8;
                    if dy != 0 {
                        canvas.scroll_vertical(dy, col_u8);
                    }
                    if dx != 0 {
                        canvas.scroll_horizontal(dx, col_u8);
                    }
                }
                gfx_draw::DOTTED_H => gfx::draw::draw_dotted_line_h(canvas, x, y, w, extra, col),
                _ => {}
            }
            0
        }
        op::GfxText => unsafe { draw_text_req(a0 as *const KrxTextReq) as u16 as usize },
        op::GfxCommit => {
            let row_mask = a0 as u8;
            canvas.dirty |= row_mask;
            #[cfg(not(test))]
            if !crate::system::is_headless() {
                crate::kern_hw().lcd.blit(canvas);
            }
            #[cfg(test)]
            if let Some(ctx) = super::active_ctx_opt() {
                if let Some(ref mut d) = ctx.display {
                    d.canvas.dirty |= row_mask;
                    d.lcd.blit(d.canvas);
                }
            }
            0
        }
        op::GfxFb => {
            canvas.mark_all_dirty();
            canvas.buffer.as_mut_ptr() as usize
        }
        op::GfxCtl => {
            match a0 as u8 {
                gfx_ctl::BITMAP => {
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
    use abi::Op;

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
