use crate::{Canvas, Color};

/// Normal spacing in pixels between status bar items/icons flowing right-to-left
pub const STATUS_ITEM_SPACING: i16 = 2;

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

/// Dynamically places and renders a status icon flowing right-to-left.
/// Automatically calculates icon width and applies standard item spacing (`width + 2`).
/// Returns the updated `right_limit`.
#[inline]
pub fn draw_status_icon(
    canvas: &mut Canvas,
    right_limit: i16,
    y: i16,
    icon: crate::icons::Icon,
    color: Color,
) -> i16 {
    let w = icon.width() as i16;
    let pos = right_limit - (w + STATUS_ITEM_SPACING);
    crate::icons::draw_icon_5x5(canvas, pos, y, icon, color);
    pos
}

/// Draws the charging bolt icon with preserved closer spacing to the battery.
/// Bolt is deliberately placed closer to battery terminal (7px if battery icon present, 5px otherwise).
#[inline]
pub fn draw_status_bolt(
    canvas: &mut Canvas,
    right_limit: i16,
    y: i16,
    has_bat_icon: bool,
    color: Color,
) -> i16 {
    let step = if has_bat_icon { 7 } else { 5 };
    let pos = right_limit - step;
    crate::icons::draw_icon_5x5(canvas, pos, y, crate::icons::Icon::Bolt, color);
    pos
}

/// Draws the USB/serial plug icon with normal icon spacing (7px width + 2px gap = 9px).
/// Selects `Icon::SerialActive` when serial activity is actively occurring, else `Icon::Plug`.
#[inline]
pub fn draw_status_plug(
    canvas: &mut Canvas,
    right_limit: i16,
    y: i16,
    serial_active: bool,
    color: Color,
) -> i16 {
    let plug_icon = if serial_active {
        crate::icons::Icon::SerialActive
    } else {
        crate::icons::Icon::Plug
    };
    draw_status_icon(canvas, right_limit, y, plug_icon, color)
}

/// Draws text flowing right-to-left with standard spacing.
/// Returns the updated `right_limit`.
#[inline]
pub fn draw_status_text(
    canvas: &mut Canvas,
    right_limit: i16,
    y_text: i16,
    text: &str,
    font: &crate::GfxFont,
    color: Color,
) -> i16 {
    let w = crate::text::str_width(text, font) as i16;
    let pos = right_limit - (w + STATUS_ITEM_SPACING);
    crate::text::draw_str(canvas, pos, y_text, text, font, color);
    pos
}

/// Modular configuration and automatic renderer for the right side of the status bar.
#[derive(Debug, Clone, Copy)]
pub struct StatusRightConfig<'a> {
    pub percentage: u8,
    pub voltage_mv: u16,
    pub temp_deci_c: i16,
    pub battery_format: u8,
    pub is_charging: bool,
    pub is_low: bool,
    pub is_usb: bool,
    pub serial_active: bool,
    pub is_f_active: bool,
    pub is_keypad_locked: bool,
    pub shading_mode: u8,
    pub anim_step: u8,
    pub now_ms: u32,
    pub inverted: bool,
    pub dynamic_icons: &'a [crate::icons::Icon],
}

impl<'a> StatusRightConfig<'a> {
    /// Renders all right-side status elements dynamically flowing right-to-left.
    /// Preserves battery and bolt geometry while handling USB plug and dynamic icons.
    pub fn render(&self, canvas: &mut Canvas) -> i16 {
        use crate::fonts::tomthumb::FONT;

        let (fg, bg) = if self.inverted {
            (Color::Clear, Color::Fill)
        } else {
            (Color::Fill, Color::Clear)
        };

        let y_offset: u8 = if self.inverted { 2 } else { 0 };
        let y_text: i16 = if self.inverted { 6 } else { 4 };

        let mut right_limit = 126i16; // 127 - 1px screen padding
        let has_bat_icon = matches!(self.battery_format, 3..=5);

        // 1. High precision battery icon
        if has_bat_icon {
            right_limit -= 12;
            let blink_off = self.is_low && !self.is_charging && (((self.now_ms >> 9) & 1) != 0);
            if !blink_off {
                draw_battery_colored(
                    canvas,
                    right_limit as u8,
                    y_offset,
                    self.percentage,
                    self.is_charging,
                    self.anim_step,
                    fg,
                    bg,
                    self.shading_mode,
                );
            }
        }

        // 2. Charging bolt (closer spacing preserved) or low battery warning
        if self.is_charging {
            right_limit = draw_status_bolt(canvas, right_limit, y_offset as i16, has_bat_icon, fg);
        } else if self.is_low {
            right_limit -= if has_bat_icon { 5 } else { 4 };
            crate::text::draw_char(canvas, right_limit, y_text, '!', &FONT, fg);
        }

        // 3. USB / Serial Plug Icon
        // Shows when USB is connected or serial is active; disappears cleanly when unplugged/inactive.
        // Spacing is normal (7px width + 2px spacing = 9px).
        if self.is_usb || self.serial_active {
            right_limit =
                draw_status_plug(canvas, right_limit, y_offset as i16, self.serial_active, fg);
        }

        // 4. Battery Text Reading (Voltage, Percentage, or MCU Temperature)
        let mut nbuf = [0u8; 12];
        let (val, dec, suf) = match self.battery_format {
            1 | 4 => ((((self.voltage_mv as u32) * 52429) >> 19) as i32, 2, "v"),
            6 => ((((self.voltage_mv as u32) * 5243) >> 19) as i32, 1, "v"),
            2 | 5 | 7 => (self.percentage as i32, 0, "%"),
            8 => ((((self.temp_deci_c as u32) * 52429) >> 19) as i32, 0, "C"),
            _ => (0, 0, ""),
        };
        if !suf.is_empty() {
            let bat_str = crate::text::format_int_fixed(val, dec, "", suf, &mut nbuf);
            right_limit = draw_status_text(canvas, right_limit, y_text, bat_str, &FONT, fg);
        }

        // 5. System Indicators (F-key badge and Keypad Lock)
        if !self.inverted {
            if self.is_f_active {
                right_limit -= 7;
                draw_f_key(canvas, right_limit, y_offset as i16);
            }
            if self.is_keypad_locked {
                right_limit = draw_status_icon(
                    canvas,
                    right_limit,
                    y_offset as i16,
                    crate::icons::Icon::Lock,
                    Color::Fill,
                );
            }
        }

        // 6. Dynamic Application & Subsystem Icons
        for &icon in self.dynamic_icons {
            right_limit = draw_status_icon(canvas, right_limit, y_offset as i16, icon, fg);
        }

        right_limit
    }
}

/// Renders the complete status bar frame with automatic layout for left, center, and right elements.
pub fn draw_status_bar_layout(
    canvas: &mut Canvas,
    show_divider: bool,
    signal_level: Option<u8>,
    title: Option<&str>,
    left_badge: Option<&str>,
    center_text: Option<&str>,
    right_limit: i16,
) {
    use crate::fonts::tomthumb::FONT;

    // Clear status bar background & draw dimmed divider line if enabled
    if show_divider {
        let clear_w = (right_limit.max(0) as u8).min(128);
        if clear_w > 0 {
            canvas.fill_rect(0, 0, clear_w, 7, Color::Clear);
        }
        canvas.draw_horizontal_line(0, 6, 128, Color::DarkGray);
    }

    // 1. Left items (flowing left-to-right)
    let mut left_x = 1i16;
    if let Some(lvl) = signal_level {
        draw_signal(canvas, left_x, 0, lvl, Color::Fill);
        left_x += (SIGNAL_WIDTH as i16) + 2;
    }
    if let Some(t) = title {
        if !t.is_empty() {
            let w = crate::text::str_width(t, &FONT) as i16;
            crate::text::draw_str_max_x(canvas, left_x, 4, t, &FONT, Color::Fill, right_limit);
            left_x += w + 2;
        }
    }
    if let Some(badge) = left_badge {
        if !badge.is_empty() {
            let bw = draw_badge(canvas, left_x, 0, badge, &FONT, false) as i16;
            left_x += bw + 2;
        }
    }

    // 2. Center items (auto-centered in remaining dynamic space)
    if let Some(center) = center_text {
        if !center.is_empty() {
            let w = crate::text::str_width(center, &FONT) as i16;
            let ideal_x = 64i16 - (w >> 1);
            let x = if ideal_x >= left_x && ideal_x + w <= right_limit {
                ideal_x
            } else {
                left_x + ((right_limit - left_x - w).max(0) >> 1)
            };
            if x + w <= right_limit {
                crate::text::draw_str(canvas, x, 4, center, &FONT, Color::Fill);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::Icon;

    #[test]
    fn test_status_icon_dynamic_spacing() {
        let mut canvas = Canvas::new();
        let start = 120i16;

        // 5x5 icon (Lock, width 5) -> stride is 5 + 2 = 7
        let next1 = draw_status_icon(&mut canvas, start, 0, Icon::Lock, Color::Fill);
        assert_eq!(next1, 113);

        // 7x5 icon (Plug, width 7) -> stride is 7 + 2 = 9 (normal spacing)
        let next2 = draw_status_icon(&mut canvas, next1, 0, Icon::Plug, Color::Fill);
        assert_eq!(next2, 104);

        // 6x5 icon (Beacon, width 6) -> stride is 6 + 2 = 8
        let next3 = draw_status_icon(&mut canvas, next2, 0, Icon::Beacon, Color::Fill);
        assert_eq!(next3, 96);
    }

    #[test]
    fn test_bolt_spacing_preserved() {
        let mut canvas = Canvas::new();
        // With battery icon: bolt is placed closer to battery (7px)
        let next_with_bat = draw_status_bolt(&mut canvas, 114, 0, true, Color::Fill);
        assert_eq!(next_with_bat, 107);

        // Without battery icon: 5px
        let next_no_bat = draw_status_bolt(&mut canvas, 114, 0, false, Color::Fill);
        assert_eq!(next_no_bat, 109);
    }

    #[test]
    fn test_plug_show_and_disappear() {
        let mut canvas = Canvas::new();

        let cfg_unplugged = StatusRightConfig {
            percentage: 85,
            voltage_mv: 8200,
            temp_deci_c: 250,
            battery_format: 3, // Icon only
            is_charging: false,
            is_low: false,
            is_usb: false,
            serial_active: false,
            is_f_active: false,
            is_keypad_locked: false,
            shading_mode: 1,
            anim_step: 0,
            now_ms: 1000,
            inverted: false,
            dynamic_icons: &[],
        };
        let limit_unplugged = cfg_unplugged.render(&mut canvas);

        let cfg_plugged = StatusRightConfig {
            is_usb: true,
            ..cfg_unplugged
        };
        let limit_plugged = cfg_plugged.render(&mut canvas);

        // When plugged in, limit should be reduced by exactly 9px (normal plug icon spacing: 7 + 2)
        assert_eq!(limit_unplugged - limit_plugged, 9);
    }

    #[test]
    fn test_status_dynamic_icons_flow() {
        let mut canvas = Canvas::new();
        let dynamic = [Icon::Scan, Icon::Moon];

        let cfg = StatusRightConfig {
            percentage: 90,
            voltage_mv: 8250,
            temp_deci_c: 220,
            battery_format: 3,
            is_charging: false,
            is_low: false,
            is_usb: false,
            serial_active: false,
            is_f_active: false,
            is_keypad_locked: false,
            shading_mode: 1,
            anim_step: 0,
            now_ms: 500,
            inverted: false,
            dynamic_icons: &dynamic,
        };
        let base_cfg = StatusRightConfig {
            dynamic_icons: &[],
            ..cfg
        };

        let limit_dyn = cfg.render(&mut canvas);
        let limit_base = base_cfg.render(&mut canvas);

        // Two 5x5 icons should each take 7px (total 14px)
        assert_eq!(limit_base - limit_dyn, 14);
    }

    #[test]
    fn test_status_bar_layout_preserves_right_items() {
        let mut canvas = Canvas::new();
        let cfg = StatusRightConfig {
            percentage: 90,
            voltage_mv: 8250,
            temp_deci_c: 220,
            battery_format: 3, // Battery icon
            is_charging: false,
            is_low: false,
            is_usb: false,
            serial_active: false,
            is_f_active: false,
            is_keypad_locked: false,
            shading_mode: 1,
            anim_step: 0,
            now_ms: 500,
            inverted: false,
            dynamic_icons: &[],
        };
        let right_limit = cfg.render(&mut canvas);
        // Verify battery icon drew pixels on page 0 at x=120
        assert_ne!(canvas.buffer[0][120], 0);

        // Layout status bar with divider enabled
        draw_status_bar_layout(
            &mut canvas,
            true,
            None,
            Some("Home"),
            None,
            None,
            right_limit,
        );

        // Battery icon pixels on the right MUST still be preserved!
        assert_ne!(
            canvas.buffer[0][120], 0,
            "Right status bar items were erased by draw_status_bar_layout"
        );
    }
}
