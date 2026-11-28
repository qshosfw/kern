//! Reusable Tag, Capsule Button, Key Hint, and Selection Capsule Elements.

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::icons;
use crate::text;
use crate::{Canvas, Color};

/// Visual styling for tags, capsule buttons, and toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeStyle {
    /// 1px crisp outline, clear background, filled text.
    Empty,
    /// Solid filled rectangle, inverted (clear) text.
    Filled,
    /// Bayer 4x4 matrix dithered fill (0..=16) for transitions or disabled states.
    Dither(u8),
    /// Subdued / secondary button: dark gray outline with filled text.
    Dimmed,
    /// Disabled button: light gray outline with light gray text.
    Disabled,
}

/// Standardized badge size tiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeSize {
    /// 7px height using TomThumb 3x5 font with square corners.
    Compact,
    /// 9px height using TomThumb 3x5 font with 1px corner cuts.
    #[default]
    Standard,
    /// 11px height using MuMatrix 5x7 font with 1px corner cuts.
    Large,
}

impl BadgeSize {
    pub const fn height(&self) -> u8 {
        match self {
            BadgeSize::Compact => 7,
            BadgeSize::Standard => 9,
            BadgeSize::Large => 11,
        }
    }

    pub const fn has_rounded_corners(&self) -> bool {
        !matches!(self, BadgeSize::Compact)
    }
}

pub fn draw_tag_badge(canvas: &mut Canvas, x: i16, y: i16, text: &str, style: BadgeStyle) {
    draw_tag_badge_sized(canvas, x, y, text, style, BadgeSize::Standard);
}

fn render_badge_box(
    canvas: &mut Canvas,
    xu: u8,
    yu: u8,
    w: u8,
    h: u8,
    style: BadgeStyle,
    rounded: bool,
) -> Color {
    let is_filled = matches!(style, BadgeStyle::Filled);
    canvas.fill_rect(
        xu,
        yu,
        w,
        h,
        if is_filled { Color::Fill } else { Color::Clear },
    );
    if let BadgeStyle::Dither(level) = style {
        canvas.fill_rect_dither(xu, yu, w, h, level);
    }
    let (border, tc) = match style {
        BadgeStyle::Empty => (Some(Color::Fill), Color::Fill),
        BadgeStyle::Filled => (None, Color::Clear),
        BadgeStyle::Dither(level) => (
            Some(Color::Fill),
            if level >= 12 {
                Color::Clear
            } else {
                Color::Fill
            },
        ),
        BadgeStyle::Dimmed => (Some(Color::DarkGray), Color::Fill),
        BadgeStyle::Disabled => (Some(Color::LightGray), Color::LightGray),
    };
    if let Some(c) = border {
        canvas.draw_rect(xu, yu, w, h, c);
    }
    if rounded {
        canvas.clear_corners(xu, yu, w, h);
    }
    tc
}

pub fn draw_tag_badge_sized(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    text: &str,
    style: BadgeStyle,
    size: BadgeSize,
) {
    let font = match size {
        BadgeSize::Large => &MUMATRIX,
        _ => &TOMTHUMB,
    };
    let text_w = text::str_width(text, font) as u8;
    let w = text_w + 6;
    let h = size.height();
    let xu = x.max(0) as u8;
    let yu = y.max(0) as u8;
    let text_y = match size {
        BadgeSize::Compact => y + 5,
        BadgeSize::Standard => y + 6,
        BadgeSize::Large => y + 8,
    };
    let tc = render_badge_box(canvas, xu, yu, w, h, style, size.has_rounded_corners());
    text::draw_str(canvas, x + 3, text_y, text, font, tc);
}

pub fn draw_tag_badge_with_icon(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    icon: &[u8; 8],
    text: &str,
    style: BadgeStyle,
) {
    let text_w = text::str_width(text, &TOMTHUMB) as u8;
    let w = text_w + 16;
    let xu = x.max(0) as u8;
    let yu = y.max(0) as u8;
    let tc = render_badge_box(canvas, xu, yu, w, 10, style, true);
    icons::draw_icon_8(canvas, (xu + 2) as i16, (yu + 1) as i16, icon, tc);
    text::draw_str(canvas, x + 12, y + 7, text, &TOMTHUMB, tc);
}

pub fn draw_capsule_button(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    w: u8,
    h: u8,
    label: &str,
    style: BadgeStyle,
) {
    let xu = x.max(0) as u8;
    let yu = y.max(0) as u8;
    let tc = render_badge_box(canvas, xu, yu, w, h, style, h >= 9);
    text::draw_str_align(
        canvas,
        x + ((w as i16) >> 1),
        y + ((h as i16) >> 1) + 2,
        label,
        &TOMTHUMB,
        tc,
        text::Align::Center,
    );
}

pub fn draw_toggle_tag_badge(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    label: &str,
    is_active: bool,
    dither_progress: u8,
) {
    let style = if is_active {
        if dither_progress >= 16 {
            BadgeStyle::Filled
        } else {
            BadgeStyle::Dither(dither_progress)
        }
    } else if dither_progress == 0 {
        BadgeStyle::Empty
    } else {
        BadgeStyle::Dither(16 - dither_progress)
    };
    draw_tag_badge(canvas, x, y, label, style);
}

pub const CAPSULE_FILL: u8 = 1;
pub const CAPSULE_ROUND: u8 = 2;
pub const CAPSULE_FILL_ROUND: u8 = CAPSULE_FILL | CAPSULE_ROUND;

pub fn draw_selection_capsule(canvas: &mut Canvas, bounds: [u8; 4], flags: u8) -> Color {
    let [x, y, w, h] = bounds;
    let filled = (flags & CAPSULE_FILL) != 0;
    if filled {
        canvas.fill_rect(x, y, w, h, Color::Fill);
    } else {
        canvas.draw_rect(x, y, w, h, Color::Fill);
    }
    if (flags & CAPSULE_ROUND) != 0 && h >= 4 {
        canvas.clear_corners(x, y, w, h);
    }
    if filled { Color::Clear } else { Color::Fill }
}

pub(crate) fn key_badge_key_width(key_str: &str) -> u8 {
    match key_str.as_bytes() {
        b"^" | b"v" | b"V" => 5,
        _ => text::str_width(key_str, &TOMTHUMB) as u8,
    }
}

/// Render an inverted key badge box at `(x, y)` (7px tall) and return its pixel width.
pub fn draw_key_badge_box(canvas: &mut Canvas, x: i16, y: i16, key_str: &str, on_dark: bool) -> u8 {
    let box_w = key_badge_key_width(key_str) + 4;
    let (box_col, fg_col) = if on_dark {
        (Color::Clear, Color::Fill)
    } else {
        (Color::Fill, Color::Clear)
    };
    canvas.fill_rect(x as u8, y as u8, box_w, 7, box_col);

    match key_str {
        "^" | "v" | "V" => {
            let is_up = key_str == "^";
            for dx in 0..5u8 {
                let d = dx.abs_diff(2) as i16;
                let py = if is_up { y + 2 + d } else { y + 4 - d };
                canvas.draw_pixel((x + 2 + dx as i16) as u8, py as u8, fg_col);
            }
        }
        _ => text::draw_str(canvas, x + 2, y + 5, key_str, &TOMTHUMB, fg_col),
    }
    box_w
}

pub fn draw_key_badge(canvas: &mut Canvas, x: i16, y: i16, key_str: &str, label: &str) {
    let label = text::localize_scratch(label, true);
    draw_key_badge_raw(canvas, x, y, key_str, label);
}

pub(crate) fn draw_key_badge_raw(canvas: &mut Canvas, x: i16, y: i16, key_str: &str, label: &str) {
    let box_w = draw_key_badge_box(canvas, x, y, key_str, false);
    if !label.is_empty() {
        text::draw_str(
            canvas,
            x + (box_w as i16) + 2,
            y + 5,
            label,
            &TOMTHUMB,
            Color::Fill,
        );
    }
}

pub fn draw_key_badge_right(canvas: &mut Canvas, right_x: i16, y: i16, key_str: &str, label: &str) {
    let label = text::localize_scratch(label, true);
    let key_w = key_badge_key_width(key_str) as i16;
    let label_w = if label.is_empty() {
        0
    } else {
        text::str_width(label, &TOMTHUMB) + 2
    };
    draw_key_badge_raw(canvas, right_x - (key_w + 4 + label_w), y, key_str, label);
}
