//! Top Status Bar Chrome & Non-Blocking Toast Banner (`StatusBar`, `ToastBanner`).

use super::View;
use crate::elements::{SIGNAL_WIDTH, draw_signal};
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::text;
use crate::{Canvas, Color};
use kernel::event::Event;
use utils::format::InlineStr;

pub const STATUS_ITEM_SPACING: i16 = 2;

pub const STATUS_FLAG_CHARGING: u8 = 1 << 0;
pub const STATUS_FLAG_LOW: u8 = 1 << 1;
pub const STATUS_FLAG_USB: u8 = 1 << 2;
pub const STATUS_FLAG_SERIAL: u8 = 1 << 3;
pub const STATUS_FLAG_F_ACTIVE: u8 = 1 << 4;
pub const STATUS_FLAG_KEYPAD_LOCKED: u8 = 1 << 5;
pub const STATUS_FLAG_INVERTED: u8 = 1 << 6;
pub const STATUS_FLAG_PARTIAL: u8 = 1 << 7;

pub fn draw_f_key(canvas: &mut Canvas, x: i16, y: i16) {
    if x < 0 || y != 0 || x + 5 > 128 {
        return;
    }
    const F_KEY_COLS: [u8; 5] = [0x1F, 0x11, 0x19, 0x1D, 0x1F];
    let ux = x as usize;
    for i in 0..5 {
        canvas.buffer[0][ux + i] |= F_KEY_COLS[i];
    }
    canvas.mark_span(0, x as u8, (x + 4) as u8);
}

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

#[inline]
pub fn draw_status_bolt(
    canvas: &mut Canvas,
    right_limit: i16,
    y: i16,
    has_bat_icon: bool,
    color: Color,
) -> i16 {
    let pos = right_limit - if has_bat_icon { 7 } else { 5 };
    crate::icons::draw_icon_5x5(canvas, pos, y, crate::icons::Icon::Bolt, color);
    pos
}

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

pub fn draw_status_text(
    canvas: &mut Canvas,
    right_limit: i16,
    y_text: i16,
    s: &str,
    font: &crate::GfxFont,
    color: Color,
) -> i16 {
    let w = text::str_width(s, font);
    let pos = right_limit - (w + STATUS_ITEM_SPACING);
    text::draw_str(canvas, pos, y_text, s, font, color);
    pos
}

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

    /// 32-bit state digest for change detection across status bar frames.
    #[inline(always)]
    pub fn state_digest(&self) -> u32 {
        let anim = if self.is_charging() || self.is_low() {
            self.anim_step
        } else {
            0
        };
        (self.percentage as u32)
            | ((self.battery_format as u32) << 8)
            | ((self.flags as u32) << 12)
            | ((anim as u32) << 20)
            | ((self.dynamic_glyphs.len() as u32) << 28) ^ ((self.voltage_mv as u32) << 4)
    }

    pub fn draw_battery(&self, canvas: &mut Canvas, x: u8, y: u8) {
        let (fg, bg) = if self.inverted() {
            (Color::Clear, Color::Fill)
        } else {
            (Color::Fill, Color::Clear)
        };
        let work_end_x = x + 11;
        canvas.draw_rect(x, y, 12, 5, fg);
        if x > 0 {
            canvas.draw_vertical_line(x - 1, y + 1, 3, fg);
        }
        canvas.fill_rect(x + 1, y + 1, 10, 3, bg);

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
        let mut fill_bars = full_bars;
        if is_charging && full_bars < 10 {
            let range = 11 - full_bars;
            let mut rem = self.anim_step;
            while rem >= range {
                rem -= range;
            }
            fill_bars = full_bars + rem;
        }
        if fill_bars > 0 {
            canvas.fill_rect(work_end_x - fill_bars, y + 1, fill_bars, 3, fg);
        }
        if is_charging && fill_bars < 10 && fill_bars > full_bars {
            canvas.draw_vertical_line(work_end_x - fill_bars, y + 1, 3, Color::DarkGray);
        } else if !is_charging && partial > 0 && full_bars < 10 {
            let col = if partial == 2 {
                Color::DarkGray
            } else {
                Color::LightGray
            };
            canvas.draw_vertical_line(work_end_x - full_bars - 1, y + 1, 3, col);
        }
    }

    pub fn render(&self, canvas: &mut Canvas) -> i16 {
        let fg = if self.inverted() {
            Color::Clear
        } else {
            Color::Fill
        };
        let y_offset: u8 = if self.inverted() { 2 } else { 0 };
        let y_text: i16 = if self.inverted() { 6 } else { 4 };
        let mut right_limit = 126i16;
        let has_bat_icon = matches!(self.battery_format, 3..=5);

        if has_bat_icon {
            right_limit -= 12;
            let blink_off =
                self.is_low() && !self.is_charging() && (((self.anim_step >> 1) & 1) != 0);
            if !blink_off {
                self.draw_battery(canvas, right_limit as u8, y_offset);
            }
        }

        let y_icon = y_offset as i16;
        if self.is_charging() {
            right_limit = draw_status_bolt(canvas, right_limit, y_icon, has_bat_icon, fg);
        } else if self.is_low() {
            right_limit -= if has_bat_icon { 5 } else { 4 };
            text::draw_char(canvas, right_limit, y_text, '!', &TOMTHUMB, fg);
        }

        if self.is_usb() || self.serial_active() {
            right_limit = draw_status_plug(canvas, right_limit, y_icon, self.serial_active(), fg);
        }

        let mut nbuf_uninit = core::mem::MaybeUninit::<[u8; 12]>::uninit();
        let nbuf = unsafe { &mut *nbuf_uninit.as_mut_ptr() };
        let mut val = 0i32;
        let mut dec = 0u8;
        let suf = match self.battery_format {
            1 | 4 => {
                val = utils::math::div10_u16(self.voltage_mv).0 as i32;
                dec = 2;
                "v"
            }
            6 => {
                val = utils::math::div100_u16(self.voltage_mv).0 as i32;
                dec = 1;
                "v"
            }
            2 | 5 | 7 => {
                val = self.percentage as i32;
                "%"
            }
            8 => {
                val = utils::math::div10_u16(self.temp_deci_c.max(0) as u16).0 as i32;
                "C"
            }
            _ => "",
        };
        if !suf.is_empty() {
            let bat_str = text::format_int_fixed(val, dec, "", suf, nbuf);
            right_limit = draw_status_text(canvas, right_limit, y_text, bat_str, &TOMTHUMB, fg);
        }

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

pub struct StatusBar;

impl StatusBar {
    #[inline(always)]
    pub fn render_layout(
        canvas: &mut Canvas,
        config: &kernel::app::StatusBarConfig<'_>,
        right_limit: i16,
    ) {
        draw_status_bar_layout(canvas, config, right_limit);
    }
}

pub fn draw_status_bar_layout(
    canvas: &mut Canvas,
    config: &kernel::app::StatusBarConfig<'_>,
    right_limit: i16,
) {
    if config.show_divider {
        let clear_w = (right_limit.max(0) as u8).min(128);
        if clear_w > 0 {
            canvas.fill_rect(0, 0, clear_w, 7, Color::Clear);
        }
        canvas.draw_horizontal_line(0, 6, 128, Color::DarkGray);
    }

    let mut left_x = 1i16;
    if let Some(lvl) = config.signal_level {
        draw_signal(canvas, left_x, 0, lvl, Color::Fill);
        left_x += (SIGNAL_WIDTH as i16) + 2;
    }
    for opt in [config.title, config.left_badge] {
        if let Some(s) = opt
            && !s.is_empty()
        {
            left_x =
                text::draw_str_max_x(canvas, left_x, 4, s, &TOMTHUMB, Color::Fill, right_limit) + 2;
        }
    }
    for glyph in config.left_icons {
        let w = glyph.width() as i16;
        if left_x + w < right_limit {
            crate::icons::draw_status_glyph(canvas, left_x, 0, glyph, Color::Fill);
            left_x += w + STATUS_ITEM_SPACING;
        }
    }

    let center_str = config.center_text.unwrap_or("");
    let text_w = if center_str.is_empty() {
        0
    } else {
        text::str_width(center_str, &TOMTHUMB)
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
            text::draw_str_max_x(
                canvas,
                cur_x,
                4,
                center_str,
                &TOMTHUMB,
                Color::Fill,
                right_limit,
            );
        }
    }
}

// ── Toast Banner (`ToastState`, `ToastBanner`) ──────────────────────────────

pub struct ToastState {
    pub is_active: bool,
    pub duration_ms: u16,
    pub elapsed_ms: u16,
    pub message: InlineStr<32>,
}

impl ToastState {
    pub const fn new() -> Self {
        Self {
            is_active: false,
            duration_ms: 0,
            elapsed_ms: 0,
            message: InlineStr::new(),
        }
    }

    pub fn show(&mut self, msg: &str, duration_ms: u16) {
        self.message.set(msg);
        self.duration_ms = duration_ms;
        self.elapsed_ms = 0;
        self.is_active = true;
    }

    pub fn dismiss(&mut self) {
        self.is_active = false;
        self.elapsed_ms = 0;
    }

    pub fn tick(&mut self, dt_ms: u16) {
        if self.is_active {
            self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
            if self.elapsed_ms >= self.duration_ms {
                self.is_active = false;
            }
        }
    }
}

impl Default for ToastState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ToastBanner;

impl ToastBanner {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ToastBanner {
    fn default() -> Self {
        Self::new()
    }
}

impl View<ToastState> for ToastBanner {
    fn render(&self, canvas: &mut Canvas, model: &ToastState) {
        if !model.is_active || model.message.is_empty() {
            return;
        }
        let text_w = (model.message.len() as i16 * 4).min(112);
        let badge_w = (text_w + 12).min(124) as u8;
        let badge_x = (128 - badge_w) >> 1;
        canvas.fill_rect(badge_x, 26, badge_w, 13, Color::Fill);
        canvas.draw_rect(badge_x, 26, badge_w, 13, Color::Clear);
        text::draw_str_align(
            canvas,
            64,
            34,
            model.message.as_str(),
            &TOMTHUMB,
            Color::Clear,
            text::Align::Center,
        );
    }

    fn on_event(&mut self, model: &mut ToastState, event: &Event) -> bool {
        if let Event::Tick = *event {
            model.tick(50);
            return model.is_active;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::icons::Icon;

    #[test]
    fn status_bar_spacing_plug_tray_and_layout() {
        let mut canvas = Canvas::new();
        let next1 = draw_status_icon(&mut canvas, 120, 0, Icon::Lock, Color::Fill);
        assert_eq!(next1, 113);
        let next2 = draw_status_icon(&mut canvas, next1, 0, Icon::Plug, Color::Fill);
        assert_eq!(next2, 104);

        let cfg_unplugged = StatusRightConfig::new(85, 3, 8200);
        let limit_unplugged = cfg_unplugged.render(&mut canvas);
        let cfg_plugged = cfg_unplugged.with_flag(STATUS_FLAG_USB, true);
        let limit_plugged = cfg_plugged.render(&mut canvas);
        assert_eq!(limit_unplugged - limit_plugged, 9);
        assert_ne!(cfg_unplugged.state_digest(), cfg_plugged.state_digest());

        let dynamic = [
            crate::icons::StatusGlyph::Builtin(Icon::Scan),
            crate::icons::StatusGlyph::Builtin(Icon::Moon),
            crate::icons::StatusGlyph::Builtin(Icon::Beacon),
            crate::icons::StatusGlyph::Builtin(Icon::Storage),
        ];
        let partial_cfg = StatusRightConfig::new(90, 3, 8250)
            .with_flags(STATUS_FLAG_PARTIAL)
            .with_dynamic_glyphs(&dynamic);
        assert!(partial_cfg.render(&mut canvas) >= 86);
    }
}
