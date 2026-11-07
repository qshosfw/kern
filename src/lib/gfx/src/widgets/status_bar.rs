use crate::{Canvas, Color};

/// Draw DeltaFW-style inverted F-key badge (5x5 pixels with outer border and inverted 'F' inside)
pub fn draw_f_key(canvas: &mut Canvas, x: i16, y: i16) {
    if x < 0 || y < 0 {
        return;
    }
    let ux = x as u8;
    let uy = y as u8;

    canvas.fill_rect(ux, uy, 5, 5, Color::Fill);
    canvas.draw_horizontal_line(ux + 1, uy + 1, 3, Color::Clear);
    canvas.draw_vertical_line(ux + 1, uy + 1, 3, Color::Clear);
    canvas.draw_horizontal_line(ux + 1, uy + 2, 2, Color::Clear);
}

/// Draw DeltaFW-style high precision battery icon with customizable foreground, background, and shading mode
pub fn draw_battery_colored(
    canvas: &mut Canvas,
    x: u8,
    y: u8,
    percentage: u8,
    is_charging: bool,
    anim_step: u8,
    fg: Color,
    bg: Color,
    shading_mode: u8,
) {
    let start_x = x;
    let work_end_x = start_x + 11;

    canvas.draw_horizontal_line(start_x + 1, y, 10, fg);
    canvas.draw_horizontal_line(start_x + 1, y + 4, 10, fg);
    canvas.draw_vertical_line(start_x - 1, y + 1, 3, fg);
    canvas.draw_vertical_line(start_x, y, 5, fg);
    canvas.draw_vertical_line(work_end_x, y, 5, fg);

    canvas.fill_rect(start_x + 1, y + 1, 10, 3, bg);

    if shading_mode == 2 {
        // Pure Monochrome Mode (1-bit binary): 100% faithful to standard DeltaFW
        let max_bars = (((percentage as u16 + 5) * 205) >> 11).min(10) as u8;
        let fill_bars = if is_charging {
            if max_bars < 10 {
                let range = 11 - max_bars;
                let step = if range > 0 { anim_step % range } else { 0 };
                max_bars + step
            } else {
                10
            }
        } else {
            max_bars
        };

        if fill_bars > 0 {
            canvas.fill_rect(work_end_x - fill_bars, y + 1, fill_bars, 3, fg);
        }
    } else {
        // High-Precision 30-Step Optical Grayscale / Dither Mode
        let p = percentage.min(100) as u32;
        let total_steps = (((p * 1229) + 614) >> 12).min(30) as u8;
        let full_bars = ((total_steps as u16 * 86) >> 8).min(10) as u8;
        let partial = total_steps - (full_bars * 3);

        if is_charging {
            let fill_bars = if full_bars < 10 {
                let range = 11 - full_bars;
                let step = if range > 0 { anim_step % range } else { 0 };
                full_bars + step
            } else {
                10
            };

            if fill_bars > 0 {
                canvas.fill_rect(work_end_x - fill_bars, y + 1, fill_bars, 3, fg);
            }
            if fill_bars < 10 && fill_bars > full_bars {
                let lead_col = work_end_x - fill_bars;
                canvas.draw_vertical_line(lead_col, y + 1, 3, Color::DarkGray);
            }
        } else {
            if full_bars > 0 {
                canvas.fill_rect(work_end_x - full_bars, y + 1, full_bars, 3, fg);
            }
            if partial > 0 && full_bars < 10 {
                let partial_col = work_end_x - full_bars - 1;
                let partial_color = if partial == 2 {
                    Color::DarkGray
                } else {
                    Color::LightGray
                };
                canvas.draw_vertical_line(partial_col, y + 1, 3, partial_color);
            }
        }
    }
}

/// Draw DeltaFW-style high precision battery icon (13x5 pixels, x: 115..=127, y: 0..=4)
pub fn draw_battery(
    canvas: &mut Canvas,
    x: u8,
    y: u8,
    percentage: u8,
    is_charging: bool,
    anim_step: u8,
) {
    draw_battery_colored(
        canvas,
        x,
        y,
        percentage,
        is_charging,
        anim_step,
        Color::Fill,
        Color::Clear,
        1, // Dither by default
    );
}

/// Width in pixels occupied by DeltaFW signal antenna and 5 progressive bars
pub const SIGNAL_WIDTH: u8 = 12;

/// Draw DeltaFW-style antenna and signal bar widget (12x5 pixels).
pub fn draw_signal(canvas: &mut Canvas, x: i16, y: i16, level: u8, color: Color) -> u8 {
    if x < 0 || x + (SIGNAL_WIDTH as i16) > 128 || y < 0 {
        return SIGNAL_WIDTH;
    }
    let ux = x as u8;
    let uy = y as u8;

    // 1. Antenna body (Y-shape: tips at 0 and 2, stem at 1)
    canvas.draw_pixel(ux, uy, color);
    canvas.draw_vertical_line(ux + 1, uy + 1, 4, color);
    canvas.draw_pixel(ux + 2, uy, color);

    // 2. Progressive signal bars (1 to 5) with 1px spacers
    if level >= 1 {
        canvas.draw_vertical_line(ux + 3, uy + 4, 1, color);
    }
    if level >= 2 {
        canvas.draw_vertical_line(ux + 5, uy + 3, 2, color);
    }
    if level >= 3 {
        canvas.draw_vertical_line(ux + 7, uy + 2, 3, color);
    }
    if level >= 4 {
        canvas.draw_vertical_line(ux + 9, uy + 1, 4, color);
    }
    if level >= 5 {
        canvas.draw_vertical_line(ux + 11, uy, 5, color);
    }

    SIGNAL_WIDTH
}

/// Draw a modular status badge / indicator text (e.g. "DW", "LOCK", "MSG", "VOX")
pub fn draw_badge(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    text: &str,
    font: &crate::GfxFont,
    inverted: bool,
) -> u8 {
    let w = crate::text::str_width(text, font);
    if w <= 0 || x < 0 || x + w > 128 {
        return 0;
    }

    if inverted {
        canvas.fill_rect(x as u8, y as u8, (w + 2) as u8, 5, Color::Fill);
        crate::text::draw_str(canvas, x + 1, y + 4, text, font, Color::Clear);
        (w + 3) as u8
    } else {
        crate::text::draw_str(canvas, x, y + 4, text, font, Color::Fill);
        (w + 1) as u8
    }
}

// Backward-compatibility aliases
pub use draw_badge as draw_status_badge;
pub use draw_battery as draw_battery_icon;
pub use draw_f_key as draw_f_key_icon;
pub use draw_signal as draw_signal_antenna;
pub const SIGNAL_ANTENNA_WIDTH: u8 = SIGNAL_WIDTH;
