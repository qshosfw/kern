use crate::text;
use crate::{Canvas, Color, GfxFont};

/// Draw S-Meter widget (S1..S9 + 10dB..60dB)
pub fn draw_s_meter(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    rssi_dbm: i16,
    peak_hold_level: u8,
    font: &GfxFont,
) {
    let steps = if rssi_dbm < -121 {
        0
    } else if rssi_dbm >= -53 {
        12
    } else {
        (((rssi_dbm + 121) as u16 * 12) / 68) as u8
    };

    if steps <= 9 {
        text::draw_str(canvas, x, y, "S", font, Color::Fill);
        text::draw_char(canvas, x + 6, y, (b'0' + steps) as char, font, Color::Fill);
    } else {
        let s = match steps {
            10 => "S9+10",
            11 => "S9+20",
            _ => "S9+30",
        };
        text::draw_str(canvas, x, y, s, font, Color::Fill);
    }

    let bar_x_start = x + 34;
    for i in 0..12 {
        let bx = (bar_x_start + (i * 5) as i16) as u8;
        let by = (y + 1) as u8;
        if i < steps {
            canvas.fill_rect(bx, by, 4, 6, Color::Fill);
        } else {
            canvas.draw_rect(bx, by, 4, 6, Color::Fill);
        }
    }

    if peak_hold_level > 0 && peak_hold_level <= 12 {
        let px = bar_x_start + ((peak_hold_level - 1) * 5) as i16 + 1;
        canvas.draw_vertical_line(px as u8, (y + 1) as u8, 6, Color::Invert);
    }
}
