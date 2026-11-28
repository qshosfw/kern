//! Canonical List Row & Detail Row Elements (`draw_list_row`, `draw_detail_row`).

use super::badge::{CAPSULE_FILL, CAPSULE_ROUND, draw_key_badge_box, draw_selection_capsule};
use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::icons;
use crate::text;
use crate::{Canvas, Color};

pub const ROW_SELECTED: u8 = 1 << 0;
pub const ROW_CHEVRON: u8 = 1 << 1;
pub const ROW_OUTLINE: u8 = 1 << 2;
pub const ROW_TOGGLE: u8 = 1 << 3;
pub const ROW_TOGGLE_ON: u8 = 1 << 4;
pub const ROW_ARROW_LEFT: u8 = 1 << 5;
pub const ROW_ARROW_RIGHT: u8 = 1 << 6;
pub const ROW_SHORTCUT: u8 = 1 << 7;

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

fn draw_row_right_value(
    canvas: &mut Canvas,
    s: &ListRowSpec<'_>,
    right_edge: i16,
    text_dy: i16,
    sw: i16,
    tc: Color,
) {
    let val_str = if s.localize_value {
        text::localize_scratch(s.value, true)
    } else {
        s.value
    };
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
    if let Some(bmp) = s.icon
        && !core::ptr::eq(bmp, &icons::ICON_NONE)
    {
        icons::draw_icon_8(canvas, tx, s.y + icon_dy, bmp, tc);
        tx += 11;
    }
    let act_lbl = if s.localize_title {
        text::localize_scratch(s.value, false)
    } else {
        s.value
    };
    text::draw_str_ellipsis(
        canvas,
        tx,
        s.y + text_dy,
        act_lbl,
        &MUMATRIX,
        tc,
        right_edge - 10,
    );
    icons::draw_icon_8(canvas, right_edge - 8, s.y + 3, icons::ARROW_RIGHT, tc);
}

fn draw_row_title(
    canvas: &mut Canvas,
    s: &ListRowSpec<'_>,
    prefer_short: bool,
    tx: i16,
    title_max: i16,
    text_dy: i16,
    is_sel: bool,
    tc: Color,
) -> u8 {
    let title_str = if s.localize_title {
        text::localize_scratch(s.title, prefer_short)
    } else {
        s.title
    };
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
pub fn draw_list_row(canvas: &mut Canvas, s: &ListRowSpec<'_>) -> u8 {
    let is_sel = (s.flags & ROW_SELECTED) != 0;
    let is_outline = (s.flags & ROW_OUTLINE) != 0;
    let is_toggle = (s.flags & ROW_TOGGLE) != 0;
    let toggle_on = (s.flags & ROW_TOGGLE_ON) != 0;
    let has_chevron = (s.flags & ROW_CHEVRON) != 0;
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

    for opt in [s.prefix_icon, s.icon] {
        if let Some(bmp) = opt
            && !core::ptr::eq(bmp, &icons::ICON_NONE)
        {
            icons::draw_icon_8(canvas, tx, s.y + icon_dy, bmp, tc);
            tx += 11;
        }
    }

    let sw = if s.value.is_empty() {
        0
    } else if s.localize_value && text::has_localize_hook() {
        text::str_width(text::localize_scratch(s.value, true), &TOMTHUMB)
    } else {
        text::str_width(s.value, &TOMTHUMB)
    };
    let title_max = if is_toggle || has_chevron {
        right_edge - 14
    } else if sw > 0 {
        let pad = if is_outline && (s.flags & (ROW_ARROW_LEFT | ROW_ARROW_RIGHT)) != 0 {
            22
        } else {
            8
        };
        (right_edge - pad - sw).max(36)
    } else {
        right_edge - 3
    };

    let overflow = draw_row_title(canvas, s, sw > 0, tx, title_max, text_dy, is_sel, tc);

    if is_toggle || (sw == 0 && has_chevron) {
        let (rx, ry, bmp) = if is_toggle {
            let b = if toggle_on {
                icons::CHECKBOX_ON
            } else {
                icons::CHECKBOX_OFF
            };
            (right_edge - 10, s.y + icon_dy, b)
        } else {
            (
                right_edge - 10 + (s.chevron_dx as i16),
                s.y + icon_dy + i16::from(comfortable),
                icons::ARROW_RIGHT,
            )
        };
        icons::draw_icon_8(canvas, rx, ry, bmp, tc);
    } else if sw > 0 {
        draw_row_right_value(canvas, s, right_edge, text_dy, sw, tc);
    }

    overflow
}

#[inline(always)]
pub fn draw_field(canvas: &mut Canvas, x: u8, y: u8, w: u8, h: u8, bg: Color) {
    canvas.fill_rect(x, y, w, h, bg);
}
