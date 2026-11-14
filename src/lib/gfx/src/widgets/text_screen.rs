use crate::text::{self, Align};
use crate::{Canvas, Color};

/// Compact descriptor for one line of static text inside a `TextScreen`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct TextLine {
    pub y: u8,
    pub font: u8,
    pub align: u8,
    pub text: &'static str,
}

impl TextLine {
    pub const fn new(y: u8, font: u8, align: u8, text: &'static str) -> Self {
        Self {
            y,
            font,
            align,
            text,
        }
    }
}

/// Draw a full-screen static text screen with an optional header bar and dynamic hook.
#[inline(never)]
pub fn draw_text_screen(
    canvas: &mut Canvas,
    title: &str,
    lines: &[TextLine],
    dyn_fn: Option<fn(&mut Canvas)>,
) {
    canvas.clear();
    if !title.is_empty() {
        canvas.fill_rect(0, 0, 128, 10, Color::Fill);
        text::draw_str_align(
            canvas,
            64,
            7,
            title,
            &crate::fonts::tomthumb::FONT,
            Color::Clear,
            Align::Center,
        );
    }

    for line in lines {
        let font = match line.font {
            1 => &crate::fonts::mumatrix_regular::FONT,
            _ => &crate::fonts::tomthumb::FONT,
        };
        let (x, align) = match line.align {
            1 => (64i16, Align::Center),
            2 => (124i16, Align::Right),
            _ => (4i16, Align::Left),
        };
        text::draw_str_align(
            canvas,
            x,
            line.y as i16,
            line.text,
            font,
            Color::Fill,
            align,
        );
    }

    if let Some(hook) = dyn_fn {
        hook(canvas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_draw_text_screen_renders_header_and_lines() {
        static LINES: [TextLine; 2] = [
            TextLine::new(20, 0, 0, "Left"),
            TextLine::new(40, 1, 1, "Center"),
        ];
        let mut canvas = Canvas::new();
        draw_text_screen(&mut canvas, "SAFE", &LINES, None);
        assert_ne!(canvas.buffer[0][64], 0);
        assert!(
            canvas.buffer[2].iter().any(|&b| b != 0) || canvas.buffer[3].iter().any(|&b| b != 0)
        );
    }
}
