//! Advanced graphical primitives (Bresenham lines, circles, rounded rects, dotted lines).

use crate::{Canvas, Color};

/// Draw line using Bresenham algorithm
#[inline(never)]
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
#[inline(never)]
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

/// Draw 1-bit monochrome bitmap.
#[inline(never)]
pub fn draw_bitmap(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    width: u8,
    height: u8,
    data: &[u8],
    color: Color,
) {
    let bytes_per_row = (width as usize + 7) >> 3;
    if data.len() < (height as usize).saturating_mul(bytes_per_row) {
        return;
    }
    crate::media::draw_bitmap_clipped(canvas, x, y, width, height, data, 0, 128, color);
}

/// Decompress a 128x64 PackBits RLE page-packed bitmap into `canvas`.
#[inline(never)]
pub fn draw_rle_splash(canvas: &mut Canvas, rle: &[u8]) {
    canvas.clear();
    let flat = canvas.buffer_flat_mut();
    let mut idx = 0usize;
    let mut i = 0usize;
    while i < rle.len() && idx < 1024 {
        let hdr = rle[i] as i8;
        i += 1;
        if hdr >= 0 {
            let mut count = (hdr as usize) + 1;
            while count > 0 && i < rle.len() && idx < 1024 {
                flat[idx] = rle[i];
                idx += 1;
                i += 1;
                count -= 1;
            }
        } else if hdr != -128 && i < rle.len() {
            let mut count = (1 - hdr as i16) as usize;
            let val = rle[i];
            i += 1;
            while count > 0 && idx < 1024 {
                flat[idx] = val;
                idx += 1;
                count -= 1;
            }
        }
    }
    canvas.sync_planes();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_bitmap_ignores_truncated_buffer() {
        let mut canvas = Canvas::new();
        draw_bitmap(&mut canvas, 0, 0, 16, 4, &[0xFF, 0xFF], Color::Fill);
        assert_eq!(canvas.buffer[0][0], 0);
    }
}
