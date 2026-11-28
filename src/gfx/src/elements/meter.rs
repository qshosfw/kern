//! Stateless Progress Bar, Scrollbar, Battery Gauge & Signal Bar Elements.

use crate::{Canvas, Color};

pub const SIGNAL_WIDTH: u8 = 12;

/// Smooth progress bar with sub-pixel 3x fractional optical grayscale shading.
pub fn draw_progress_bar(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    percent: u8,
    border: bool,
) {
    let (bar_x, bar_y, bar_w, bar_h) = if border {
        canvas.draw_rect(x as u8, y as u8, w, h, Color::Fill);
        if w <= 2 || h <= 2 {
            return;
        }
        let bx = (x + 1) as u8;
        let by = (y + 1) as u8;
        let bw = w - 2;
        let bh = h - 2;
        canvas.fill_rect(bx, by, bw, bh, Color::Clear);
        (bx, by, bw, bh)
    } else {
        (x as u8, y as u8, w, h)
    };

    let pct = percent.min(100) as u32;
    let step_val = bar_w as u32 * 3 * pct + 50;
    let total_steps = (step_val * 0x147B) >> 19;
    let full_cols = (((total_steps * 0xAAAB) >> 17) as u8).min(bar_w);
    let partial = (total_steps - full_cols as u32 * 3) as u8;

    if full_cols > 0 {
        canvas.fill_rect(bar_x, bar_y, full_cols, bar_h, Color::Fill);
    }
    if partial > 0 && full_cols < bar_w {
        let partial_color = if partial == 2 {
            Color::DarkGray
        } else {
            Color::LightGray
        };
        canvas.draw_vertical_line(bar_x + full_cols, bar_y, bar_h, partial_color);
    }
}

/// Shared ultra-compact scrollbar renderer.
pub fn draw_scrollbar(
    canvas: &mut Canvas,
    track_x: u8,
    track_y: u8,
    track_h: u8,
    offset: usize,
    content: usize,
    viewport: usize,
) {
    if content <= viewport || viewport == 0 {
        return;
    }
    canvas.draw_vertical_line(track_x + 1, track_y, track_h, Color::DarkGray);
    let (thumb_y, thumb_h) = scrollbar_thumb(track_y, track_h, offset, content, viewport);
    canvas.fill_rect(track_x, thumb_y, 3, thumb_h, Color::Fill);
}

/// Thumb `(top, length)` for a scrollbar track, rounded to the nearest pixel.
pub fn scrollbar_thumb(
    track_y: u8,
    track_h: u8,
    offset: usize,
    content: usize,
    viewport: usize,
) -> (u8, u8) {
    let track = track_h as u32;
    let content = content as u32;
    let max_offset = content - viewport as u32;
    let thumb_h = ((viewport as u32 * track + (content >> 1)) / content).clamp(4, track);
    let travel = track - thumb_h;
    let pos = (offset as u32).min(max_offset);
    let thumb_y = (pos * travel + (max_offset >> 1)) / max_offset;
    (track_y + thumb_y as u8, thumb_h as u8)
}

/// Draw antenna and 5-bar signal strength widget (12x5 pixels).
pub fn draw_signal(canvas: &mut Canvas, x: i16, y: i16, level: u8, color: Color) -> u8 {
    if x < 0 || x + (SIGNAL_WIDTH as i16) > 128 || y < 0 {
        return SIGNAL_WIDTH;
    }
    let ux = x as u8;
    let uy = y as u8;
    canvas.draw_pixel(ux, uy, color);
    canvas.draw_vertical_line(ux + 1, uy + 1, 4, color);
    canvas.draw_pixel(ux + 2, uy, color);

    let bars = level.min(5);
    for i in 1..=bars {
        canvas.draw_vertical_line(ux + 1 + (i * 2), uy + 5 - i, i, color);
    }
    SIGNAL_WIDTH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrollbar_thumb_tracks_viewport_and_ends_only_at_last_row() {
        let (top, len) = scrollbar_thumb(8, 56, 0, 9, 4);
        assert_eq!((top, len), (8, 25));

        for offset in 0..5 {
            let (y, h) = scrollbar_thumb(8, 56, offset, 9, 4);
            assert!(y + h < 8 + 56, "offset {offset} reached the end early");
        }
        let (y, h) = scrollbar_thumb(8, 56, 5, 9, 4);
        assert_eq!(y + h, 8 + 56);
        assert_eq!(scrollbar_thumb(8, 56, 99, 9, 4), (y, h));
    }

    #[test]
    fn scrollbar_thumb_is_monotonic_in_pixel_units_and_keeps_minimum_length() {
        let mut last = 0u8;
        for px in 0..=(5 * 13) {
            let (y, _) = scrollbar_thumb(8, 56, px, 9 * 13, 4 * 13);
            assert!(y >= last);
            last = y;
        }
        let (y, h) = scrollbar_thumb(0, 40, 196, 200, 4);
        assert_eq!(h, 4);
        assert_eq!(y + h, 40);
    }

    #[test]
    fn draw_scrollbar_skips_content_that_fits() {
        let mut canvas = Canvas::new();
        draw_scrollbar(&mut canvas, 124, 8, 56, 0, 4, 4);
        draw_scrollbar(&mut canvas, 124, 8, 56, 0, 3, 0);
        assert!(canvas.buffer.iter().all(|page| page[125] == 0));
    }
}
