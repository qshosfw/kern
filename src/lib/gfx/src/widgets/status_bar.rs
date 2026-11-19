use crate::{Canvas, Color};

/// Normal spacing in pixels between status bar items/icons flowing right-to-left
pub const STATUS_ITEM_SPACING: i16 = 2;

/// Draw DeltaFW-style inverted F-key badge (5x5 pixels with outer border and inverted 'F' inside)
pub fn draw_f_key(canvas: &mut Canvas, x: i16, y: i16) {
    if x < 0 || y != 0 || x + 5 > 128 {
        return;
    }
    const F_KEY_COLS: [u8; 5] = [0x1F, 0x11, 0x19, 0x1D, 0x1F];
    let ux = x as usize;
    for i in 0..5 {
        canvas.buffer[0][ux + i] |= F_KEY_COLS[i];
    }
}

/// Width in pixels occupied by DeltaFW signal antenna and 5 progressive bars
pub const SIGNAL_WIDTH: u8 = 12;

/// Draw DeltaFW-style antenna and signal bar widget (12x5 pixels).
#[inline(never)]
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
    for i in 1..(bars + 1) {
        canvas.draw_vertical_line(ux + 1 + (i * 2), uy + 5 - i, i, color);
    }

    SIGNAL_WIDTH
}

/// Dynamically places and renders a status glyph flowing right-to-left.
/// Automatically calculates glyph width and applies standard item spacing (`width + 2`).
/// Returns the updated `right_limit`.
#[inline(never)]
pub fn draw_status_glyph_rtl(
    canvas: &mut Canvas,
    right_limit: i16,
    y: i16,
    glyph: &crate::icons::StatusGlyph,
    color: Color,
) -> i16 {
    let w = glyph.width() as i16;
    let pos = right_limit - (w + STATUS_ITEM_SPACING);
    crate::icons::draw_status_glyph(canvas, pos, y, glyph, color);
    pos
}

/// Dynamically places and renders a built-in status icon flowing right-to-left.
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
    draw_status_glyph_rtl(
        canvas,
        right_limit,
        y,
        &crate::icons::StatusGlyph::Builtin(icon),
        color,
    )
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

pub const STATUS_FLAG_CHARGING: u8 = 1 << 0;
pub const STATUS_FLAG_LOW: u8 = 1 << 1;
pub const STATUS_FLAG_USB: u8 = 1 << 2;
pub const STATUS_FLAG_SERIAL: u8 = 1 << 3;
pub const STATUS_FLAG_F_ACTIVE: u8 = 1 << 4;
pub const STATUS_FLAG_KEYPAD_LOCKED: u8 = 1 << 5;
pub const STATUS_FLAG_INVERTED: u8 = 1 << 6;
pub const STATUS_FLAG_PARTIAL: u8 = 1 << 7;

/// Modular configuration and automatic renderer for the right side of the status bar.
#[derive(Debug, Clone, Copy)]
pub struct StatusRightConfig<'a> {
    pub percentage: u8,
    pub battery_format: u8,
    pub shading_mode: u8,
    pub anim_step: u8,
    pub flags: u8,
    pub voltage_mv: u16,
    pub temp_deci_c: i16,
    pub dynamic_glyphs: &'a [crate::icons::StatusGlyph],
}

impl<'a> StatusRightConfig<'a> {
    pub const fn new(percentage: u8, battery_format: u8, voltage_mv: u16) -> Self {
        Self {
            percentage,
            battery_format,
            shading_mode: 1,
            anim_step: 0,
            flags: 0,
            voltage_mv,
            temp_deci_c: 250,
            dynamic_glyphs: &[],
        }
    }

    #[inline(always)]
    pub const fn with_flags(mut self, flags: u8) -> Self {
        self.flags = flags;
        self
    }

    #[inline(always)]
    pub const fn with_flag(mut self, flag: u8, val: bool) -> Self {
        if val {
            self.flags |= flag;
        } else {
            self.flags &= !flag;
        }
        self
    }

    #[inline(always)]
    pub const fn with_dynamic_glyphs(mut self, glyphs: &'a [crate::icons::StatusGlyph]) -> Self {
        self.dynamic_glyphs = glyphs;
        self
    }

    #[inline(always)]
    pub fn is_charging(&self) -> bool {
        (self.flags & STATUS_FLAG_CHARGING) != 0
    }

    #[inline(always)]
    pub fn is_low(&self) -> bool {
        (self.flags & STATUS_FLAG_LOW) != 0
    }

    #[inline(always)]
    pub fn is_usb(&self) -> bool {
        (self.flags & STATUS_FLAG_USB) != 0
    }

    #[inline(always)]
    pub fn serial_active(&self) -> bool {
        (self.flags & STATUS_FLAG_SERIAL) != 0
    }

    #[inline(always)]
    pub fn is_f_active(&self) -> bool {
        (self.flags & STATUS_FLAG_F_ACTIVE) != 0
    }

    #[inline(always)]
    pub fn is_keypad_locked(&self) -> bool {
        (self.flags & STATUS_FLAG_KEYPAD_LOCKED) != 0
    }

    #[inline(always)]
    pub fn is_partial(&self) -> bool {
        (self.flags & STATUS_FLAG_PARTIAL) != 0
    }

    #[inline(always)]
    pub fn inverted(&self) -> bool {
        #[cfg(feature = "inverted_status")]
        {
            (self.flags & STATUS_FLAG_INVERTED) != 0
        }
        #[cfg(not(feature = "inverted_status"))]
        {
            false
        }
    }

    /// Draw DeltaFW-style high precision battery icon directly using config parameters
    #[inline(never)]
    pub fn draw_battery(&self, canvas: &mut Canvas, x: u8, y: u8) {
        let (fg, bg) = if self.inverted() {
            (Color::Clear, Color::Fill)
        } else {
            (Color::Fill, Color::Clear)
        };
        let start_x = x;
        let work_end_x = start_x + 11;

        canvas.draw_rect(start_x, y, 12, 5, fg);
        if start_x > 0 {
            canvas.draw_vertical_line(start_x - 1, y + 1, 3, fg);
        }
        canvas.fill_rect(start_x + 1, y + 1, 10, 3, bg);

        let (full_bars, partial) = if self.shading_mode == 2 {
            (
                (((self.percentage as u16 + 5) * 205) >> 11).min(10) as u8,
                0u8,
            )
        } else {
            let p = self.percentage.min(100) as u32;
            let total_steps = (((p * 1229) + 614) >> 12).min(30) as u8;
            let fb = ((total_steps as u16 * 86) >> 8).min(10) as u8;
            (fb, total_steps - (fb * 3))
        };

        let is_charging = self.is_charging();
        let fill_bars = if is_charging && full_bars < 10 {
            let range = 11 - full_bars;
            let step = if range > 0 {
                let mut rem = self.anim_step;
                while rem >= range {
                    rem -= range;
                }
                rem
            } else {
                0
            };
            full_bars + step
        } else {
            full_bars
        };

        if fill_bars > 0 {
            canvas.fill_rect(work_end_x - fill_bars, y + 1, fill_bars, 3, fg);
        }

        if is_charging {
            if fill_bars < 10 && fill_bars > full_bars {
                canvas.draw_vertical_line(work_end_x - fill_bars, y + 1, 3, Color::DarkGray);
            }
        } else if partial > 0 && full_bars < 10 {
            let partial_color = if partial == 2 {
                Color::DarkGray
            } else {
                Color::LightGray
            };
            canvas.draw_vertical_line(work_end_x - full_bars - 1, y + 1, 3, partial_color);
        }
    }
}

/// Shared standalone battery icon widget reusing the OS status bar renderer.
#[inline(never)]
pub fn draw_battery(canvas: &mut Canvas, x: i16, y: i16, pct: u8, charging: bool) {
    if (x as u16) > 115 || (y as u16) > 58 {
        return;
    }
    let ux = (x as u8).max(1);
    let uy = y as u8;
    let mut cfg = StatusRightConfig::new(pct, 3, 0);
    cfg.shading_mode = 2;
    cfg.draw_battery(canvas, ux, uy);
    if charging {
        crate::icons::draw_icon_5x5(
            canvas,
            ux as i16 + 4,
            uy as i16,
            crate::icons::Icon::Bolt,
            Color::Fill,
        );
    }
}

impl<'a> StatusRightConfig<'a> {
    /// Renders all right-side status elements dynamically flowing right-to-left.
    /// Preserves battery and bolt geometry while handling USB plug and dynamic icons.
    #[inline(never)]
    pub fn render(&self, canvas: &mut Canvas) -> i16 {
        use crate::fonts::tomthumb::FONT;

        let (fg, _bg) = if self.inverted() {
            (Color::Clear, Color::Fill)
        } else {
            (Color::Fill, Color::Clear)
        };

        let y_offset: u8 = if self.inverted() { 2 } else { 0 };
        let y_text: i16 = if self.inverted() { 6 } else { 4 };

        let mut right_limit = 126i16; // 127 - 1px screen padding
        let has_bat_icon = matches!(self.battery_format, 3..=5);

        // 1. High precision battery icon
        if has_bat_icon {
            right_limit -= 12;
            let blink_off =
                self.is_low() && !self.is_charging() && (((self.anim_step >> 1) & 1) != 0);
            if !blink_off {
                self.draw_battery(canvas, right_limit as u8, y_offset);
            }
        }

        // 2. Charging bolt (closer spacing preserved) or low battery warning
        let y_icon = y_offset as i16;
        if self.is_charging() {
            right_limit = draw_status_bolt(canvas, right_limit, y_icon, has_bat_icon, fg);
        } else if self.is_low() {
            right_limit -= if has_bat_icon { 5 } else { 4 };
            crate::text::draw_char(canvas, right_limit, y_text, '!', &FONT, fg);
        }

        // 3. USB / Serial Plug Icon
        // Shows when USB is connected or serial is active; disappears cleanly when unplugged/inactive.
        // Spacing is normal (7px width + 2px spacing = 9px).
        if self.is_usb() || self.serial_active() {
            right_limit = draw_status_plug(canvas, right_limit, y_icon, self.serial_active(), fg);
        }

        // 4. Battery Text Reading (Voltage, Percentage, or MCU Temperature)
        let mut nbuf_uninit = core::mem::MaybeUninit::<[u8; 12]>::uninit();
        let nbuf = unsafe { &mut *nbuf_uninit.as_mut_ptr() };
        let mut val = 0i32;
        let mut dec = 0u8;
        let suf = match self.battery_format {
            1 | 4 => {
                val = (((self.voltage_mv as u32) * 52429) >> 19) as i32;
                dec = 2;
                "v"
            }
            6 => {
                val = (((self.voltage_mv as u32) * 5243) >> 19) as i32;
                dec = 1;
                "v"
            }
            2 | 5 | 7 => {
                val = self.percentage as i32;
                "%"
            }
            8 => {
                val = (((self.temp_deci_c as u32) * 52429) >> 19) as i32;
                "C"
            }
            _ => "",
        };
        if !suf.is_empty() {
            let bat_str = crate::text::format_int_fixed(val, dec, "", suf, nbuf);
            right_limit = draw_status_text(canvas, right_limit, y_text, bat_str, &FONT, fg);
        }

        // 5. System Indicators (F-key badge and Keypad Lock)
        if !self.inverted() {
            if self.is_f_active() {
                right_limit -= 7;
                draw_f_key(canvas, right_limit, y_icon);
            }
            if self.is_keypad_locked() {
                right_limit = draw_status_icon(
                    canvas,
                    right_limit,
                    y_icon,
                    crate::icons::Icon::Lock,
                    Color::Fill,
                );
            }
        }

        // 6. Dynamic Application & Subsystem Icons (tray icons)
        let min_tray_limit = if self.is_partial() { 86 } else { 32 };
        for glyph in self.dynamic_glyphs {
            let w = glyph.width() as i16;
            if right_limit - (w + STATUS_ITEM_SPACING) >= min_tray_limit {
                right_limit = draw_status_glyph_rtl(canvas, right_limit, y_icon, glyph, fg);
            }
        }

        right_limit
    }
}

/// Renders the complete status bar frame with automatic layout for left, center, and right elements.
#[inline(never)]
pub fn draw_status_bar_layout(
    canvas: &mut Canvas,
    config: &kernel::app::StatusBarConfig<'_>,
    right_limit: i16,
) {
    use crate::fonts::tomthumb::FONT;

    // Clear status bar background & draw dimmed divider line if enabled
    if config.show_divider {
        let clear_w = (right_limit.max(0) as u8).min(128);
        if clear_w > 0 {
            canvas.fill_rect(0, 0, clear_w, 7, Color::Clear);
        }
        canvas.draw_horizontal_line(0, 6, 128, Color::DarkGray);
    }

    // 1. Left items (flowing left-to-right)
    let mut left_x = 1i16;
    if let Some(lvl) = config.signal_level {
        draw_signal(canvas, left_x, 0, lvl, Color::Fill);
        left_x += (SIGNAL_WIDTH as i16) + 2;
    }
    let titles = [config.title, config.left_badge];
    for opt in titles {
        if let Some(s) = opt {
            if !s.is_empty() {
                left_x = crate::text::draw_str_max_x(
                    canvas,
                    left_x,
                    4,
                    s,
                    &FONT,
                    Color::Fill,
                    right_limit,
                ) + 2;
            }
        }
    }
    for glyph in config.left_icons {
        let w = glyph.width() as i16;
        if left_x + w < right_limit {
            crate::icons::draw_status_glyph(canvas, left_x, 0, glyph, Color::Fill);
            left_x += w + STATUS_ITEM_SPACING;
        }
    }

    // 2. Center items (coordinated, auto-justified, and spaced)
    let center_str = config.center_text.unwrap_or("");
    let text_w = if center_str.is_empty() {
        0
    } else {
        crate::text::str_width(center_str, &FONT) as i16
    };

    let mut icons_w: i16 = 0;
    for glyph in config.center_icons {
        icons_w += (glyph.width() as i16) + STATUS_ITEM_SPACING;
    }
    let total_w = icons_w + text_w;

    if total_w > 0 {
        let ideal_x = 64i16 - (total_w >> 1);
        let mut cur_x = if ideal_x >= left_x && ideal_x + total_w <= right_limit {
            ideal_x
        } else {
            left_x + ((right_limit - left_x - total_w).max(0) >> 1)
        };

        for glyph in config.center_icons {
            let w = glyph.width() as i16;
            if cur_x >= left_x && cur_x + w <= right_limit {
                crate::icons::draw_status_glyph(canvas, cur_x, 0, glyph, Color::Fill);
            }
            cur_x += w + STATUS_ITEM_SPACING;
        }

        if text_w > 0 && cur_x < right_limit {
            crate::text::draw_str_max_x(
                canvas,
                cur_x,
                4,
                center_str,
                &FONT,
                Color::Fill,
                right_limit,
            );
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

        let cfg_unplugged = StatusRightConfig::new(85, 3, 8200);
        let limit_unplugged = cfg_unplugged.render(&mut canvas);

        let cfg_plugged = cfg_unplugged.with_flag(STATUS_FLAG_USB, true);
        let limit_plugged = cfg_plugged.render(&mut canvas);

        // When plugged in, limit should be reduced by exactly 9px (normal plug icon spacing: 7 + 2)
        assert_eq!(limit_unplugged - limit_plugged, 9);
    }

    #[test]
    fn test_status_dynamic_icons_flow() {
        let mut canvas = Canvas::new();
        let dynamic = [
            crate::icons::StatusGlyph::Builtin(Icon::Scan),
            crate::icons::StatusGlyph::Builtin(Icon::Moon),
        ];

        let cfg = StatusRightConfig::new(90, 3, 8250).with_dynamic_glyphs(&dynamic);
        let base_cfg = StatusRightConfig::new(90, 3, 8250);

        let limit_dyn = cfg.render(&mut canvas);
        let limit_base = base_cfg.render(&mut canvas);

        // Two 5x5 icons should each take 7px (total 14px)
        assert_eq!(limit_base - limit_dyn, 14);
    }

    #[test]
    fn test_custom_5x5_glyph_rendering_and_spacing() {
        let mut canvas = Canvas::new();
        // Custom 5x5 plus sign '+' bitmap: row 0: 0x20, row 1: 0x20, row 2: 0xF8, row 3: 0x20, row 4: 0x20
        let plus_bmp: [u8; 5] = [0b00100000, 0b00100000, 0b11111000, 0b00100000, 0b00100000];
        let custom_glyph = crate::icons::StatusGlyph::Custom(plus_bmp);
        assert_eq!(custom_glyph.width(), 5);

        // Render via RTL status bar glyph function
        let next = draw_status_glyph_rtl(&mut canvas, 100, 0, &custom_glyph, Color::Fill);
        // Expected pos = 100 - (5 + 2) = 93
        assert_eq!(next, 93);

        // Verify pixels were drawn at x=95 (the vertical bar of plus sign at x = 93 + 2)
        assert_ne!(canvas.buffer[0][95], 0);
    }

    #[test]
    fn test_center_icons_group_spacing_and_centering() {
        let mut canvas = Canvas::new();
        let icons = [
            crate::icons::StatusGlyph::Builtin(Icon::Scan),
            crate::icons::StatusGlyph::Builtin(Icon::Moon),
            crate::icons::StatusGlyph::Builtin(Icon::Lock),
        ];

        let sb_cfg = kernel::app::StatusBarConfig::default().with_center_icons(&icons);
        draw_status_bar_layout(&mut canvas, &sb_cfg, 126);

        // Total width of 3 icons (5px each + 2px spacing) = 5 + 2 + 5 + 2 + 5 = 19px.
        // Screen center is 64. Ideal start x = 64 - (19 / 2) = 55.
        // Icon 1 at x=55..60 (center column at x=57)
        // Icon 2 at x=62..67 (center column at x=64)
        // Icon 3 at x=69..74 (center column at x=71)
        // All three icons MUST have non-zero pixels at their respective distinct positions!
        assert_ne!(
            canvas.buffer[0][57], 0,
            "Icon 1 not rendered at expected x=57"
        );
        assert_ne!(
            canvas.buffer[0][64], 0,
            "Icon 2 not rendered at expected x=64"
        );
        assert_ne!(
            canvas.buffer[0][71], 0,
            "Icon 3 not rendered at expected x=71"
        );
    }

    #[test]
    fn test_composite_center_icons_and_text_coordination() {
        let mut canvas = Canvas::new();
        let icons = [crate::icons::StatusGlyph::Builtin(Icon::Lock)];
        let sb_cfg = kernel::app::StatusBarConfig::default()
            .with_center("KERN")
            .with_center_icons(&icons);

        draw_status_bar_layout(&mut canvas, &sb_cfg, 126);

        // Lock icon (5px) + 2px spacing + "KERN" (approx 4 chars * 4px = 16px) = ~23px
        // Ideal start x = 64 - (23 / 2) = ~53.
        // Lock icon rendered near x=53, text starts near x=60.
        // They must not collide and must both render non-zero pixels!
        assert_ne!(canvas.buffer[0][54], 0, "Lock icon pixels missing");
        assert_ne!(canvas.buffer[0][60], 0, "Center text pixels missing");
    }

    #[test]
    fn test_partial_mode_protects_tab_zone_from_excess_tray_icons() {
        let mut canvas = Canvas::new();
        // 4 active tray icons
        let dynamic = [
            crate::icons::StatusGlyph::Builtin(Icon::Scan),
            crate::icons::StatusGlyph::Builtin(Icon::Moon),
            crate::icons::StatusGlyph::Builtin(Icon::Beacon),
            crate::icons::StatusGlyph::Builtin(Icon::Storage),
        ];

        let cfg = StatusRightConfig::new(90, 3, 8250)
            .with_flags(STATUS_FLAG_PARTIAL)
            .with_dynamic_glyphs(&dynamic);

        let right_limit = cfg.render(&mut canvas);
        // Partial mode MUST prevent right limit from entering tab space (x < 86)
        assert!(
            right_limit >= 86,
            "Partial mode failed to protect tab zone: right_limit={}",
            right_limit
        );
    }

    #[test]
    fn test_status_bar_layout_preserves_right_items() {
        let mut canvas = Canvas::new();
        let cfg = StatusRightConfig::new(90, 3, 8250);
        let right_limit = cfg.render(&mut canvas);
        // Verify battery icon drew pixels on page 0 at x=120
        assert_ne!(canvas.buffer[0][120], 0);

        // Layout status bar with divider enabled
        let mut sb_cfg = kernel::app::StatusBarConfig::new("Home");
        sb_cfg.show_divider = true;
        draw_status_bar_layout(&mut canvas, &sb_cfg, right_limit);

        // Battery icon pixels on the right MUST still be preserved!
        assert_ne!(
            canvas.buffer[0][120], 0,
            "Right status bar items were erased by draw_status_bar_layout"
        );
    }

    #[test]
    fn test_partial_status_bar_leaves_tab_area_intact() {
        let mut canvas = Canvas::new();
        // Simulate app tab header drawn in x=0..60, y=0..7
        canvas.fill_rect(0, 0, 60, 7, Color::Fill);
        assert_eq!(canvas.buffer[0][10], 0x7F);

        let cfg = StatusRightConfig::new(85, 3, 8100);
        let right_limit = cfg.render(&mut canvas);
        assert!(
            right_limit >= 60,
            "Right limit should not collide with tabs"
        );

        // Partial status bar layout: empty title, no divider
        let sb_cfg = kernel::app::StatusBarConfig::partial();
        assert!(sb_cfg.partial);
        assert!(!sb_cfg.show_divider);

        // When partial is handled (layout skipped), tab area must remain untouched
        assert_eq!(
            canvas.buffer[0][10], 0x7F,
            "Tab header pixels must remain intact"
        );
    }

    #[test]
    fn test_status_bar_config_variants() {
        let default_cfg = kernel::app::StatusBarConfig::default();
        assert!(default_cfg.visible);
        assert!(!default_cfg.partial);
        assert!(default_cfg.show_divider);

        let partial_cfg = kernel::app::StatusBarConfig::partial();
        assert!(partial_cfg.visible);
        assert!(partial_cfg.partial);
        assert!(!partial_cfg.show_divider);

        let hidden_cfg = kernel::app::StatusBarConfig::hidden();
        assert!(!hidden_cfg.visible);
        assert!(!hidden_cfg.partial);
        assert!(!hidden_cfg.show_divider);
    }
}
