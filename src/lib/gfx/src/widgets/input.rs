use crate::text;
use crate::{Canvas, Color, GfxFont};

/// Numeric Keypad Input Box with Blinking Cursor
pub fn draw_num_keypad_input(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    prompt: &str,
    input_str: &str,
    cursor_visible: bool,
    font: &GfxFont,
) {
    canvas.draw_rect(x as u8, y as u8, w, h, Color::Fill);
    text::draw_str(canvas, x + 4, y + 3, prompt, font, Color::Fill);

    let input_x = x + 4 + text::str_width(prompt, font) + 2;
    text::draw_str(canvas, input_x, y + 3, input_str, font, Color::Fill);

    if cursor_visible {
        let cur_x = input_x + text::str_width(input_str, font);
        canvas.draw_vertical_line(cur_x as u8, (y + 3) as u8, 8, Color::Fill);
    }
}
