use crate::{Canvas, Color};

/// Smooth progress bar with optional outer frame border
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
    let fill_w = ((w as u16 * pct as u16) / 100) as u8;

    if border {
        canvas.draw_rect(x as u8, y as u8, w, h, Color::Fill);
        if fill_w > 2 && h > 2 {
            canvas.fill_rect(
                (x + 1) as u8,
                (y + 1) as u8,
                fill_w.saturating_sub(2),
                h - 2,
                Color::Fill,
            );
        }
    } else {
        canvas.fill_rect(x as u8, y as u8, fill_w, h, Color::Fill);
    }
}
