use crate::{Canvas, Color};

/// Smooth progress bar with sub-pixel 3x fractional optical grayscale shading
/// and optional outer frame border.
#[inline(never)]
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
    // Fast reciprocal division by 100: ((x * 0x147B) >> 19) exact for x <= 40,000
    let step_val = bar_w as u32 * 3 * pct + 50;
    let total_steps = (step_val * 0x147B) >> 19;

    // Fast reciprocal division and modulo by 3: ((x * 0xAAAB) >> 17) exact for x <= 1,024
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
