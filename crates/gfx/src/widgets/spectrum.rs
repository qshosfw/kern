use crate::{Canvas, Color};

#[inline(never)]
fn draw_bitmap_columns(canvas: &mut Canvas, x: i16, y: i16, cols: &[u8], color: Color) {
    for (col_idx, &byte) in cols.iter().enumerate() {
        let px = (x + col_idx as i16) as u8;
        for row in 0..8 {
            if (byte & (1 << row)) != 0 {
                canvas.draw_pixel(px, (y + row) as u8, color);
            }
        }
    }
}

#[inline(never)]
pub fn draw_digit(canvas: &mut Canvas, x: i16, y: i16, digit: u8, color: Color, big: bool) {
    if digit < 10 {
        let idx = digit as usize;
        if big {
            let data = &crate::fonts::deltafw::FONT_BIG_DIGITS[idx];
            draw_bitmap_columns(canvas, x + 1, y, &data[0..11], color);
            draw_bitmap_columns(canvas, x + 1, y + 8, &data[11..22], color);
        } else {
            let data = &crate::fonts::deltafw::FONT_SMALL_DIGITS[idx];
            draw_bitmap_columns(canvas, x, y, data, color);
        }
    } else if digit == 10 {
        draw_bitmap_columns(
            canvas,
            x + 1,
            y + if big { 8 } else { 0 },
            &[0x60, 0x60],
            color,
        );
    }
}

pub fn draw_big_digit(canvas: &mut Canvas, x: i16, y: i16, digit: u8, color: Color) {
    draw_digit(canvas, x, y, digit, color, true);
}

pub fn draw_small_digit(canvas: &mut Canvas, x: i16, y: i16, digit: u8, color: Color) {
    draw_digit(canvas, x, y, digit, color, false);
}
