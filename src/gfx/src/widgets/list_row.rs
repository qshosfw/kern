//! Canonical List Row Widget (`draw_list_row`).
//!
//! Single shared row renderer used by `MenuView`, `OptionSheet`, and the
//! `KRX_WIDGET_MENU_ITEM` syscall — ensuring consistent layout, selection
//! capsules, key-badge shortcuts, toggles, directional edit arrows, marquee
//! scrolling, and localization across firmware and external `.krx` apps.

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::icons;
use crate::text;
use crate::widgets::{CAPSULE_FILL, CAPSULE_ROUND, draw_key_badge_box, draw_selection_capsule};
use crate::{Canvas, Color};

pub const ROW_SELECTED: u8 = 1 << 0;
pub const ROW_CHEVRON: u8 = 1 << 1;
pub const ROW_OUTLINE: u8 = 1 << 2;
pub const ROW_TOGGLE: u8 = 1 << 3;
pub const ROW_TOGGLE_ON: u8 = 1 << 4;
pub const ROW_ARROW_LEFT: u8 = 1 << 5;
pub const ROW_ARROW_RIGHT: u8 = 1 << 6;
pub const ROW_SHORTCUT: u8 = 1 << 7;

/// Layout and content descriptor for a single horizontal list row.
#[derive(Clone, Copy)]
pub struct ListRowSpec<'a> {
    pub x: u8,
    pub y: i16,
    pub w: u8,
    pub h: u8,
    pub flags: u8,
    pub title: &'a str,
    pub value: &'a str,
    pub prefix_icon: Option<&'a [u8; 8]>,
    pub icon: Option<&'a [u8; 8]>,
    pub marquee_x: i16,
    pub chevron_dx: i8,
    pub localize_title: bool,
    pub localize_value: bool,
    pub marquee_enabled: bool,
    pub clip_top: i16,
}

impl<'a> ListRowSpec<'a> {
    pub const fn new(x: u8, y: i16, w: u8, h: u8, title: &'a str) -> Self {
        Self {
            x,
            y,
            w,
            h,
            flags: 0,
            title,
            value: "",
            prefix_icon: None,
            icon: None,
            marquee_x: 0,
            chevron_dx: 0,
            localize_title: true,
            localize_value: false,
            marquee_enabled: false,
            clip_top: 0,
        }
    }
}

#[inline(never)]
fn draw_row_right_value_str(
    canvas: &mut Canvas,
    s: &ListRowSpec<'_>,
    val_str: &str,
    right_edge: i16,
    text_dy: i16,
    sw: i16,
    tc: Color,
) {
    let is_outline = (s.flags & ROW_OUTLINE) != 0;
    let arrow_left = (s.flags & ROW_ARROW_LEFT) != 0;
    let arrow_right = (s.flags & ROW_ARROW_RIGHT) != 0;
    let val_right = if is_outline && (arrow_left || arrow_right) {
        if arrow_right {
            icons::draw_icon_8(canvas, right_edge - 8, s.y + 3, icons::ARROW_RIGHT, tc);
        }
        if arrow_left {
            icons::draw_icon_8(
                canvas,
                (right_edge - 18 - sw).max(40),
                s.y + 3,
                icons::ARROW_LEFT,
                tc,
            );
        }
        right_edge - 10
    } else {
        right_edge - 3
    };
    text::draw_str(
        canvas,
        val_right - sw,
        s.y + text_dy - 1,
        val_str,
        &TOMTHUMB,
        tc,
    );
}

#[inline(never)]
fn draw_row_right_value(
    canvas: &mut Canvas,
    s: &ListRowSpec<'_>,
    right_edge: i16,
    text_dy: i16,
    sw: i16,
    tc: Color,
) {
    if s.localize_value && text::has_localize_hook() {
        let mut lbuf = [0u8; 32];
        let val_str = text::localize(s.value, true, &mut lbuf);
        draw_row_right_value_str(canvas, s, val_str, right_edge, text_dy, sw, tc);
    } else {
        draw_row_right_value_str(canvas, s, s.value, right_edge, text_dy, sw, tc);
    }
}
#[inline(never)]
fn draw_shortcut_row(
    canvas: &mut Canvas,
    s: &ListRowSpec<'_>,
    mut tx: i16,
    text_dy: i16,
    icon_dy: i16,
    right_edge: i16,
    tc: Color,
) {
    tx += draw_key_badge_box(canvas, tx, s.y + 3, s.title, tc == Color::Clear) as i16 + 3;
    if let Some(bmp) = s.icon {
        if !core::ptr::eq(bmp, &icons::ICON_NONE) {
            icons::draw_icon_8(canvas, tx, s.y + icon_dy, bmp, tc);
            tx += 11;
        }
    }
    let mut lbuf = [0u8; 36];
    let act_lbl = if s.localize_title {
        text::localize(s.value, false, &mut lbuf)
    } else {
        s.value
    };
    let max_x = right_edge - 10;
    text::draw_str_ellipsis(canvas, tx, s.y + text_dy, act_lbl, &MUMATRIX, tc, max_x);
    icons::draw_icon_8(canvas, right_edge - 8, s.y + 3, icons::ARROW_RIGHT, tc);
}

#[inline(never)]
fn measure_row_value_localized(s: &ListRowSpec<'_>) -> i16 {
    let mut lbuf = [0u8; 32];
    let v = text::localize(s.value, true, &mut lbuf);
    text::str_width(v, &TOMTHUMB)
}

#[inline(never)]
fn draw_row_title_str(
    canvas: &mut Canvas,
    s: &ListRowSpec<'_>,
    title_str: &str,
    tx: i16,
    title_max: i16,
    text_dy: i16,
    is_sel: bool,
    tc: Color,
) -> u8 {
    let avail = (title_max - tx).max(0);
    let title_w = text::str_width(title_str, &MUMATRIX);
    let overflow = (title_w - avail).clamp(0, 255) as u8;

    if is_sel && s.marquee_enabled && overflow > 0 {
        text::draw_str_clipped(
            canvas,
            tx,
            title_max,
            s.marquee_x.min(overflow as i16),
            s.y + text_dy,
            title_str,
            &MUMATRIX,
            tc,
        );
    } else if overflow > 0 {
        text::draw_str_ellipsis(
            canvas,
            tx,
            s.y + text_dy,
            title_str,
            &MUMATRIX,
            tc,
            title_max,
        );
    } else {
        text::draw_str_max_x(
            canvas,
            tx,
            s.y + text_dy,
            title_str,
            &MUMATRIX,
            tc,
            title_max,
        );
    }
    overflow
}

#[inline(never)]
fn draw_row_title_localized(
    canvas: &mut Canvas,
    s: &ListRowSpec<'_>,
    prefer_short: bool,
    tx: i16,
    title_max: i16,
    text_dy: i16,
    is_sel: bool,
    tc: Color,
) -> u8 {
    let mut lbuf = [0u8; 32];
    let title_str = text::localize(s.title, prefer_short, &mut lbuf);
    draw_row_title_str(canvas, s, title_str, tx, title_max, text_dy, is_sel, tc)
}

/// Render a canonical list row and return the title's horizontal overflow in pixels (`0` when it fits).
#[inline(never)]
pub fn draw_list_row(canvas: &mut Canvas, s: &ListRowSpec<'_>) -> u8 {
    let is_sel = (s.flags & ROW_SELECTED) != 0;
    let is_outline = (s.flags & ROW_OUTLINE) != 0;
    let is_toggle = (s.flags & ROW_TOGGLE) != 0;
    let toggle_on = (s.flags & ROW_TOGGLE_ON) != 0;
    let has_chevron = (s.flags & ROW_CHEVRON) != 0;
    let arrow_left = (s.flags & ROW_ARROW_LEFT) != 0;
    let arrow_right = (s.flags & ROW_ARROW_RIGHT) != 0;
    let is_shortcut = (s.flags & ROW_SHORTCUT) != 0;

    let h_i16 = s.h as i16;
    let mut tc = Color::Fill;
    if is_sel {
        let top = s.y.max(s.clip_top);
        let bot = (s.y + h_i16).min(64);
        if bot > top {
            let mut cap_flags = 0u8;
            if !is_outline {
                cap_flags |= CAPSULE_FILL;
            }
            if s.y >= s.clip_top && s.y + h_i16 <= 64 {
                cap_flags |= CAPSULE_ROUND;
            }
            tc =
                draw_selection_capsule(canvas, [s.x, top as u8, s.w, (bot - top) as u8], cap_flags);
        }
    }

    let comfortable = s.h >= 12;
    let icon_dy: i16 = if comfortable { 2 } else { 1 };
    let text_dy: i16 = if s.h >= 13 {
        9
    } else if s.h >= 11 {
        8
    } else {
        7
    };
    let right_edge = (s.x as i16) + (s.w as i16);
    let mut tx = (s.x as i16) + 3;

    if is_shortcut {
        draw_shortcut_row(canvas, s, tx, text_dy, icon_dy, right_edge, tc);
        return 0;
    }

    if let Some(bmp) = s.prefix_icon {
        icons::draw_icon_8(canvas, tx, s.y + icon_dy, bmp, tc);
        tx += 11;
    }
    if let Some(bmp) = s.icon {
        if !core::ptr::eq(bmp, &icons::ICON_NONE) {
            icons::draw_icon_8(canvas, tx, s.y + icon_dy, bmp, tc);
            tx += 11;
        }
    }

    let sw = if s.value.is_empty() {
        0
    } else if s.localize_value && text::has_localize_hook() {
        measure_row_value_localized(s)
    } else {
        text::str_width(s.value, &TOMTHUMB)
    };
    let title_max = if is_toggle || has_chevron {
        right_edge - 14
    } else if sw > 0 {
        let pad = if is_outline && (arrow_left || arrow_right) {
            22
        } else {
            8
        };
        (right_edge - pad - sw).max(36)
    } else {
        right_edge - 3
    };

    let overflow = if s.localize_title && text::has_localize_hook() {
        draw_row_title_localized(canvas, s, sw > 0, tx, title_max, text_dy, is_sel, tc)
    } else {
        draw_row_title_str(canvas, s, s.title, tx, title_max, text_dy, is_sel, tc)
    };

    if is_toggle {
        let bmp = if toggle_on {
            icons::CHECKBOX_ON
        } else {
            icons::CHECKBOX_OFF
        };
        icons::draw_icon_8(canvas, right_edge - 10, s.y + icon_dy, bmp, tc);
    } else if sw > 0 {
        draw_row_right_value(canvas, s, right_edge, text_dy, sw, tc);
    } else if has_chevron {
        icons::draw_icon_8(
            canvas,
            right_edge - 10 + (s.chevron_dx as i16),
            s.y + icon_dy + i16::from(comfortable),
            icons::ARROW_RIGHT,
            tc,
        );
    }

    overflow
}

/// Self-clearing bounding-box primitive: clears `[x..x+w, y..y+h)` to `bg` in a single pass
/// so dynamic metrics update in place without full-screen clear flicker.
#[inline(always)]
pub fn draw_field(canvas: &mut Canvas, x: u8, y: u8, w: u8, h: u8, bg: Color) {
    canvas.fill_rect(x, y, w, h, bg);
}

/// Render a 2-line (`21 px` tall) virtual list row (`[Badge] Title  RightMetric` / `Subtitle`).
#[inline(never)]
pub fn draw_detail_row(
    canvas: &mut Canvas,
    y: i16,
    selected: bool,
    badge: &str,
    title: &str,
    right_metric: &str,
    subtitle: &str,
) {
    if y <= -22 || y >= 64 {
        return;
    }
    let tc = if selected {
        canvas.fill_rect_i(2, y, 121, 21, Color::Fill);
        canvas.clear_corners_i(2, y, 121, 21);
        Color::Clear
    } else {
        canvas.fill_rect_i(2, y, 121, 21, Color::Clear);
        Color::Fill
    };

    let mut tx: i16 = 5;
    if !badge.is_empty() {
        tx += draw_key_badge_box(canvas, tx, y + 2, badge, selected) as i16 + 3;
    }

    let rw = text::str_width(right_metric, &TOMTHUMB);
    let title_max_x = (120 - rw - 4).max(tx + 12);
    text::draw_str_ellipsis(canvas, tx, y + 9, title, &MUMATRIX, tc, title_max_x);
    if !right_metric.is_empty() {
        text::draw_str_align(
            canvas,
            120,
            y + 8,
            right_metric,
            &TOMTHUMB,
            tc,
            text::Align::Right,
        );
    }
    if !subtitle.is_empty() {
        text::draw_str_ellipsis(canvas, 5, y + 18, subtitle, &TOMTHUMB, tc, 120);
    }
}
