//! Advanced graphical primitives (Bresenham lines, circles, rounded rects, dotted lines).

use crate::{Canvas, Color};

/// Draw line using Bresenham algorithm
pub fn draw_line(canvas: &mut Canvas, mut x0: i16, mut y0: i16, x1: i16, y1: i16, color: Color) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        canvas.draw_pixel_i(x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

// Circle & rounded rect primitives removed to save 1.3KB flash

/// Draw horizontal dotted line for grid or graph dividers
pub fn draw_dotted_line_h(canvas: &mut Canvas, x0: i16, x1: i16, y: i16, step: u8, color: Color) {
    if y < 0 || y >= canvas.height() as i16 {
        return;
    }
    let start = x0.min(x1).max(0);
    let end = x0.max(x1).min(canvas.width() as i16 - 1);
    let step = step.max(1) as i16;

    let mut x = start;
    while x <= end {
        canvas.draw_pixel(x as u8, y as u8, color);
        x += step;
    }
}

/// Draw 1-bit monochrome bitmap
pub fn draw_bitmap(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    width: u8,
    height: u8,
    data: &[u8],
    color: Color,
) {
    let bytes_per_row = (width as usize).div_ceil(8);
    for row in 0..height as usize {
        for col in 0..width as usize {
            let byte = data[row * bytes_per_row + (col / 8)];
            if (byte & (0x80 >> (col % 8))) != 0 {
                draw_pixel_safe(canvas, x + col as i16, y + row as i16, color);
            }
        }
    }
}

// ── Private Helpers ──────────────────────────────────────────────────────────

#[inline(always)]
fn draw_pixel_safe(canvas: &mut Canvas, x: i16, y: i16, color: Color) {
    if x >= 0 && x < canvas.width() as i16 && y >= 0 && y < canvas.height() as i16 {
        canvas.draw_pixel(x as u8, y as u8, color);
    }
}
