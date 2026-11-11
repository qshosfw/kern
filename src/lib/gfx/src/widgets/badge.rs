//! Reusable Tag, Capsule Button, and Status Badge widgets.
//!
//! Supports outline, solid inverted fill, and progressive Bayer 4x4 dither states
//! for micro-interactions, toggles, and state transitions.
//! Compact (7px) tags keep square corners to preserve 1px font padding,
//! while standard/large tags use standard 1px corner cuts (`clear_corners`).

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::icons;
use crate::text;
use crate::{Canvas, Color};

/// Visual styling for tags, capsule buttons, and toggles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeStyle {
    /// 1px crisp outline, clear background, filled text
    Empty,
    /// Solid filled rectangle, inverted (clear) text
    Filled,
    /// Bayer 4x4 matrix dithered fill (0..=16) for transitions or disabled states
    Dither(u8),
    /// Subdued / secondary button: dark gray outline with filled text
    Dimmed,
    /// Disabled button: light gray outline with light gray text
    Disabled,
}

/// Standardized badge size tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BadgeSize {
    /// 7px height using TomThumb 3x5 font with square corners (preserves 1px top/bottom padding)
    Compact,
    /// 9px height using TomThumb 3x5 font with 1px corner cuts
    #[default]
    Standard,
    /// 11px height using MuMatrix 5x7 font with 1px corner cuts
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
        match self {
            BadgeSize::Compact => false, // Square corners for 7px to preserve padding
            BadgeSize::Standard | BadgeSize::Large => true,
        }
    }
}

/// Draw a capsule tag badge with default standard sizing
pub fn draw_tag_badge(canvas: &mut Canvas, x: i16, y: i16, text: &str, style: BadgeStyle) {
    draw_tag_badge_sized(canvas, x, y, text, style, BadgeSize::Standard);
}

#[inline(never)]
fn render_badge_box(
    canvas: &mut Canvas,
    xu: u8,
    yu: u8,
    w: u8,
    h: u8,
    style: BadgeStyle,
    rounded: bool,
) -> Color {
    let (bg, border, tc) = match style {
        BadgeStyle::Empty => (Some(Color::Clear), Some(Color::Fill), Color::Fill),
        BadgeStyle::Filled => (Some(Color::Fill), None, Color::Clear),
        BadgeStyle::Dither(level) => {
            canvas.fill_rect(xu, yu, w, h, Color::Clear);
            canvas.fill_rect_dither(xu, yu, w, h, level);
            let tc = if level >= 12 {
                Color::Clear
            } else {
                Color::Fill
            };
            (None, Some(Color::Fill), tc)
        }
        BadgeStyle::Dimmed => (Some(Color::Clear), Some(Color::DarkGray), Color::Fill),
        BadgeStyle::Disabled => (Some(Color::Clear), Some(Color::LightGray), Color::LightGray),
    };
    if let Some(c) = bg {
        canvas.fill_rect(xu, yu, w, h, c);
    }
    if let Some(c) = border {
        canvas.draw_rect(xu, yu, w, h, c);
    }
    if rounded {
        canvas.clear_corners(xu, yu, w, h);
    }
    tc
}

/// Draw a capsule tag badge with specific size tier
#[inline(never)]
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
    let rounded = size.has_rounded_corners();

    let text_y = match size {
        BadgeSize::Compact => y + 5,
        BadgeSize::Standard => y + 6,
        BadgeSize::Large => y + 8,
    };

    let tc = render_badge_box(canvas, xu, yu, w, h, style, rounded);
    text::draw_str(canvas, x + 3, text_y, text, font, tc);
}

/// Draw a tag badge with an 8x8 icon followed by text
pub fn draw_tag_badge_with_icon(
    canvas: &mut Canvas,
    x: i16,
    y: i16,
    icon: &[u8; 8],
    text: &str,
    style: BadgeStyle,
) {
    let font = &TOMTHUMB;
    let text_w = text::str_width(text, font) as u8;
    let w = text_w + 16;
    let h = 10u8;
    let xu = x.max(0) as u8;
    let yu = y.max(0) as u8;
    let ix = (xu + 2) as i16;
    let iy = (yu + 1) as i16;

    let tc = render_badge_box(canvas, xu, yu, w, h, style, true);
    icons::draw_icon_8(canvas, ix, iy, icon, tc);
    text::draw_str(canvas, x + 12, y + 7, text, font, tc);
}

/// Draw a capsule action button with standard 1px corner cuts and centered label
#[inline(never)]
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
    let rounded = h >= 9;

    let center_x = x + (w as i16 / 2);
    let center_y = y + (h as i16 / 2) + 2;

    let tc = render_badge_box(canvas, xu, yu, w, h, style, rounded);
    text::draw_str_align(
        canvas,
        center_x,
        center_y,
        label,
        &TOMTHUMB,
        tc,
        text::Align::Center,
    );
}

/// Smoothly animate or display a toggle badge using dither progress
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
