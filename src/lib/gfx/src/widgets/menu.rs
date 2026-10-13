use crate::text;
use crate::{Canvas, Color, GfxFont};

/// Scrollable Menu List Widget with highlight bar & index counter
pub fn draw_menu_list(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    items: &[&str],
    selected_idx: usize,
    font: &GfxFont,
) {
    if items.is_empty() {
        return;
    }

    let line_height = (font.y_advance + 2) as i16;
    let visible_count = (h as i16 / line_height).max(1) as usize;

    let scroll_top = if selected_idx >= visible_count {
        selected_idx - visible_count + 1
    } else {
        0
    };

    for i in 0..visible_count {
        let item_idx = scroll_top + i;
        if item_idx >= items.len() {
            break;
        }

        let item_y = y + (i as i16 * line_height);

        if item_idx == selected_idx {
            canvas.fill_rect(x as u8, item_y as u8, w, line_height as u8, Color::Fill);
            text::draw_str(
                canvas,
                x + 4,
                item_y + 1,
                items[item_idx],
                font,
                Color::Clear,
            );
        } else {
            text::draw_str(
                canvas,
                x + 4,
                item_y + 1,
                items[item_idx],
                font,
                Color::Fill,
            );
        }
    }

    if items.len() > visible_count {
        let sb_x = (x + w as i16 - 3) as u8;
        canvas.draw_vertical_line(sb_x, y as u8, h, Color::Fill);
        let thumb_h = ((visible_count as u16 * h as u16) / items.len() as u16).max(3) as u8;
        let thumb_y = y as u8
            + ((scroll_top as u16 * (h - thumb_h) as u16) / (items.len() - visible_count) as u16)
                as u8;
        canvas.fill_rect(sb_x - 1, thumb_y, 3, thumb_h, Color::Fill);
    }
}
