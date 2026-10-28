use crate::{Canvas, Color};

/// Smooth progress bar with sub-pixel 3x fractional optical grayscale shading
/// and optional outer frame border.
pub fn draw_progress_bar(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    percent: u8,
    border: bool,
) {
    let pct = percent.min(100);

    if border {
        canvas.draw_rect(x as u8, y as u8, w, h, Color::Fill);
        if w > 2 && h > 2 {
            let inner_x = (x + 1) as u8;
            let inner_y = (y + 1) as u8;
            let inner_w = w - 2;
            let inner_h = h - 2;

            canvas.fill_rect(inner_x, inner_y, inner_w, inner_h, Color::Clear);

            let total_steps = (inner_w as u32 * 3 * pct as u32 + 50) / 100;
            let full_cols = (total_steps / 3).min(inner_w as u32) as u8;
            let partial = (total_steps % 3) as u8;

            if full_cols > 0 {
                canvas.fill_rect(inner_x, inner_y, full_cols, inner_h, Color::Fill);
            }
            if partial > 0 && full_cols < inner_w {
                let partial_color = if partial == 2 {
                    Color::DarkGray
                } else {
                    Color::LightGray
                };
                canvas.draw_vertical_line(inner_x + full_cols, inner_y, inner_h, partial_color);
            }
        }
    } else {
        let total_steps = (w as u32 * 3 * pct as u32 + 50) / 100;
        let full_cols = (total_steps / 3).min(w as u32) as u8;
        let partial = (total_steps % 3) as u8;

        if full_cols > 0 {
            canvas.fill_rect(x as u8, y as u8, full_cols, h, Color::Fill);
        }
        if partial > 0 && full_cols < w {
            let partial_color = if partial == 2 {
                Color::DarkGray
            } else {
                Color::LightGray
            };
            canvas.draw_vertical_line(x as u8 + full_cols, y as u8, h, partial_color);
        }
    }
}
