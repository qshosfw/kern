//! Display & Graphics System Call Handlers (`Op::GfxDraw`, `Op::GfxText`, domain `0x6`).

use super::{active_canvas_mut, parse_str, ptr_ref, ptr_slice};
use kernel::proc::abi::KrxTextReq;

#[inline(always)]
fn font_from_u8(font_id: u8) -> &'static gfx::GfxFont {
    if font_id == 1 {
        &gfx::fonts::mumatrix_regular::FONT
    } else {
        &gfx::fonts::tomthumb::FONT
    }
}

pub fn system_icon_8(icon_id: u8) -> Option<&'static [u8; 8]> {
    gfx::icons::system_icon(icon_id)
}

pub fn draw_icon(x: i16, y: i16, icon_id: u8, color: u8) {
    if icon_id == kernel::proc::abi::KRX_ICON_NONE {
        return;
    }
    let col = gfx::Color::from_u8(color);
    let canvas = active_canvas_mut();
    if let Some(bits) = system_icon_8(icon_id) {
        gfx::icons::draw_icon_8(canvas, x, y, bits, col);
    } else if let Some(icon) = kernel::ui::Icon::from_u8(icon_id) {
        gfx::icons::draw_icon(canvas, x, y, icon, col);
    } else {
        gfx::icons::draw_icon_8(canvas, x, y, gfx::icons::FALLBACK_QMARK_8, col);
    }
}

pub unsafe fn draw_text_req(req: *const KrxTextReq) -> i16 {
    let Some(r) = (unsafe { ptr_ref(req) }) else {
        return 0;
    };
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
    let col = gfx::Color::from_u8(r.color);
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

pub unsafe fn op_gfx_draw(a0: usize, a1: usize, a2: usize) -> usize {
    use abi::sub::gfx_draw;
    let canvas = active_canvas_mut();
    let sub = a0 as u8;
    let col_u8 = (a0 >> 8) as u8;
    let extra = (a0 >> 16) as u8;
    let x = a1 as i16;
    let y = (a1 >> 16) as i16;
    let w = a2 as i16;
    let h = (a2 >> 16) as i16;
    let col = gfx::Color::from_u8(col_u8);
    match sub {
        gfx_draw::LINE => gfx::draw::draw_line(canvas, x, y, w, h, col),
        gfx_draw::RECT => canvas.draw_rect_i(x, y, w, h, col),
        gfx_draw::FILL_RECT => canvas.fill_rect_i(x, y, w, h, col),
        gfx_draw::CLR_CORNER => canvas.clear_corners_i(x, y, w, h),
        gfx_draw::DITHER => canvas.fill_rect_dither_i(x, y, w, h, extra),
        gfx_draw::CHAR => {
            return gfx::text::draw_char(canvas, x, y, a2 as u8 as char, font_from_u8(extra), col)
                as u16 as usize;
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
        gfx_draw::CLEAR => canvas.clear(),
        gfx_draw::FB => {
            canvas.mark_all_dirty();
            return canvas.buffer.as_mut_ptr() as usize;
        }
        gfx_draw::BITMAP => {
            let bw = extra;
            let bh = (a0 >> 24) as u8;
            let byte_len = (((bw as usize) + 7) >> 3) * (bh as usize);
            if byte_len > 0
                && let Some(slice) = unsafe { ptr_slice(a2 as *const u8, byte_len) }
            {
                gfx::draw::draw_bitmap(canvas, x, y, bw, bh, slice, col);
            }
        }
        gfx_draw::COMMIT => {
            canvas.dirty |= col_u8;
            #[cfg(not(test))]
            if !crate::system::is_headless() {
                crate::kern_hw().lcd.blit(canvas);
            }
        }
        _ => {}
    }
    0
}

pub unsafe fn op_gfx_text(a0: usize, _a1: usize, _a2: usize) -> usize {
    unsafe { draw_text_req(a0 as *const KrxTextReq) as u16 as usize }
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
        let fb = unsafe {
            super::super::kern_call(Op::GfxDraw.u32_id(), abi::sub::gfx_draw::FB as usize, 0, 0)
        } as *mut u8;
        assert_eq!(fb, canvas.buffer.as_mut_ptr() as *mut u8);
        assert_eq!(canvas.dirty, 0xFF);
    }
}
