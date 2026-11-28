//! Stateless Box, Header, Sheet, and Modal Frame Elements.

use super::badge::{draw_key_badge, draw_key_badge_right, key_badge_key_width};
use super::meter::draw_progress_bar;
use crate::text;
use crate::{Canvas, Color};

pub const MODAL_BOX_X: u8 = 2;
pub const MODAL_BOX_Y: u8 = 14;
pub const MODAL_BOX_W: u8 = 124;
pub const MODAL_BOX_H: u8 = 35;

/// Draw a cleared, 1px bordered box with 1px rounded outer corners.
pub fn draw_rounded_box(canvas: &mut Canvas, x: u8, y: u8, w: u8, h: u8) {
    canvas.fill_rect(x, y, w, h, Color::Clear);
    canvas.draw_rect(x, y, w, h, Color::Fill);
    if w > 4 && h > 4 {
        canvas.clear_corners(x, y, w, h);
    }
}

/// Center-screen modal outer box with 1px clear halo, clear interior, and rounded border.
pub fn draw_modal_box(canvas: &mut Canvas, w: u8, h: u8) -> (u8, u8) {
    let x = canvas.width().saturating_sub(w) >> 1;
    let y = canvas.height().saturating_sub(h) >> 1;
    let hx = x.saturating_sub(1);
    let hy = y.saturating_sub(1);
    let hw = (x.saturating_add(w).saturating_add(1)).min(canvas.width()) - hx;
    let hh = (y.saturating_add(h).saturating_add(1)).min(canvas.height()) - hy;
    canvas.fill_rect(hx, hy, hw, hh, Color::Clear);
    draw_rounded_box(canvas, x, y, w, h);
    (x, y)
}

/// Draw standard inverted (negated) header bar across the screen width.
pub fn draw_negated_header(canvas: &mut Canvas, y: i16, title: &str) {
    let yu = y as u8;
    let title = text::localize_scratch(title, true);
    canvas.fill_rect(0, yu, 128, 9, Color::Fill);
    canvas.draw_pixel(0, yu, Color::Clear);
    canvas.draw_pixel(127, yu, Color::Clear);
    text::draw_str_max_x(
        canvas,
        3,
        y + 6,
        title,
        &crate::fonts::tomthumb::FONT,
        Color::Clear,
        80,
    );
}

/// Shared bottom-sheet modal frame (inverted header, outer border, clear background).
#[inline(always)]
pub fn draw_sheet_frame(canvas: &mut Canvas, title: &str, sheet_y: i16, sheet_h: u8) -> i16 {
    let base_y = 64i16.saturating_sub(sheet_h as i16);
    let y_offset = (sheet_y - base_y).max(0) as i8;
    draw_sheet_slide_up(canvas, title, sheet_h, y_offset)
}

/// Shared bottom-sheet modal frame rendered with a vertical slide-up offset.
pub fn draw_sheet_slide_up(canvas: &mut Canvas, title: &str, sheet_h: u8, y_offset: i8) -> i16 {
    let base_y = 64i16.saturating_sub(sheet_h as i16);
    let sheet_y = (base_y + y_offset.max(0) as i16).min(64);
    if sheet_y >= 64 {
        return 64;
    }
    let visible_h = (64 - sheet_y).min(sheet_h as i16) as u8;
    let sy = sheet_y as u8;
    if sy > 0 {
        canvas.draw_horizontal_line(0, sy - 1, 128, Color::Clear);
    }
    draw_rounded_box(canvas, 0, sy, 128, visible_h);
    draw_negated_header(canvas, sheet_y, title);
    sheet_y
}

/// Render optional bottom-left `[M]` confirm and bottom-right `[EXIT]` cancel badges on a sheet.
pub fn draw_sheet_badges(
    canvas: &mut Canvas,
    sheet_y: i16,
    sheet_h: u8,
    confirm: &str,
    cancel: &str,
) {
    let by = (sheet_y + sheet_h as i16).saturating_sub(10);
    if by < 64 {
        if !confirm.is_empty() {
            draw_key_badge(canvas, 3, by, "M", confirm);
        }
        if !cancel.is_empty() {
            draw_key_badge_right(canvas, 125, by, "EXIT", cancel);
        }
    }
}

/// Render floating bottom softkey badges (`y = 55..63`).
pub fn draw_key_hints(
    canvas: &mut Canvas,
    left: Option<(&str, &str)>,
    center: Option<(&str, &str)>,
    right: Option<(&str, &str)>,
    divider: bool,
) {
    canvas.fill_rect(0, 55, 128, 9, Color::Clear);
    if divider {
        canvas.draw_horizontal_line(0, 55, 128, Color::LightGray);
    }
    if let Some((k, lbl)) = left {
        draw_key_badge(canvas, 3, 56, k, lbl);
    }
    if let Some((k, lbl)) = center {
        let kw = key_badge_key_width(k) as i16 + 4;
        let lw = if lbl.is_empty() {
            0
        } else {
            text::str_width(lbl, &crate::fonts::tomthumb::FONT) + 2
        };
        draw_key_badge(canvas, ((128 - (kw + lw)) >> 1).max(0), 56, k, lbl);
    }
    if let Some((k, lbl)) = right {
        draw_key_badge_right(canvas, 125, 56, k, lbl);
    }
}

/// Centered modal progress dialog box.
pub fn draw_modal_progress(canvas: &mut Canvas, title: &str, subtitle: &str, progress: u8) {
    let (bx, by) = draw_modal_box(canvas, MODAL_BOX_W, MODAL_BOX_H);
    let center_x = (bx as i16) + ((MODAL_BOX_W as i16) >> 1);
    if !title.is_empty() {
        text::draw_str_align(
            canvas,
            center_x,
            (by + 9) as i16,
            title,
            &crate::fonts::tomthumb::FONT,
            Color::Fill,
            text::Align::Center,
        );
        text::draw_str_align(
            canvas,
            center_x,
            (by + 17) as i16,
            subtitle,
            &crate::fonts::tomthumb::FONT,
            Color::Fill,
            text::Align::Center,
        );
    } else {
        text::draw_str_align(
            canvas,
            center_x,
            (by + 13) as i16,
            subtitle,
            &crate::fonts::mumatrix_regular::FONT,
            Color::Fill,
            text::Align::Center,
        );
    }
    draw_progress_bar(
        canvas,
        center_x - 48,
        (by + 20) as i16,
        96,
        6,
        progress,
        true,
    );
}

#[inline(always)]
pub fn draw_modal_popup(canvas: &mut Canvas, title: Option<&str>, msg: &str) {
    draw_modal_popup_materialize(canvas, title, msg, 16);
}

pub fn draw_modal_popup_materialize(
    canvas: &mut Canvas,
    title: Option<&str>,
    msg: &str,
    progress: u8,
) {
    if progress == 0 {
        return;
    }
    let p = progress.min(16);
    let has_title = title.is_some_and(|t| !t.is_empty());
    let target_h: u8 = if has_title { 34 } else { 28 };
    let bw = ((122u16 * p as u16) >> 4) as u8;
    let bh = ((target_h as u16 * p as u16) >> 4) as u8;
    let (bx, by) = draw_modal_box(canvas, bw, bh);
    if p < 8 || bw < 24 || bh < 16 {
        return;
    }
    if has_title {
        canvas.fill_rect(bx, by, bw, 9, Color::Fill);
        canvas.draw_pixel(bx, by, Color::Clear);
        canvas.draw_pixel(bx + bw - 1, by, Color::Clear);
        let t = text::localize_scratch(title.unwrap_or(""), true);
        text::draw_str_max_x(
            canvas,
            (bx + 4) as i16,
            (by + 6) as i16,
            t,
            &crate::fonts::tomthumb::FONT,
            Color::Clear,
            (bx + bw - 4) as i16,
        );
        let mut y = (by + if text::count_lines(msg) <= 1 { 22 } else { 17 }) as i16;
        for line in text::lines(msg) {
            text::draw_str_max_x(
                canvas,
                (bx + 5) as i16,
                y,
                line,
                &crate::fonts::tomthumb::FONT,
                Color::Fill,
                (bx + bw - 4) as i16,
            );
            y += 8;
        }
    } else {
        let mut y = (by + if text::count_lines(msg) <= 1 { 17 } else { 12 }) as i16;
        let mut first = true;
        for line in text::lines(msg) {
            let font = if first {
                &crate::fonts::mumatrix_regular::FONT
            } else {
                &crate::fonts::tomthumb::FONT
            };
            text::draw_str_align(canvas, 64, y, line, font, Color::Fill, text::Align::Center);
            y += 10;
            first = false;
        }
    }
}
