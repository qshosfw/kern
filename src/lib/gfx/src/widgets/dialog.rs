use super::progress::draw_progress_bar;
use crate::text;
use crate::{Canvas, Color};

/// Standard modal dialog viewport constants matching text input box
pub const MODAL_BOX_X: u8 = 2;
pub const MODAL_BOX_Y: u8 = 14;
pub const MODAL_BOX_W: u8 = 124;
pub const MODAL_BOX_H: u8 = 35;

/// Center-Screen Modal Outer Box with Clear background and rounded border
pub fn draw_modal_box(canvas: &mut Canvas, w: u8, h: u8) -> (u8, u8) {
    let x = canvas.width().saturating_sub(w) >> 1;
    let y = canvas.height().saturating_sub(h) >> 1;
    canvas.fill_rect(x, y, w, h, Color::Clear);
    canvas.draw_rect(x, y, w, h, Color::Fill);
    canvas.clear_corners(x, y, w, h);
    (x, y)
}

/// Clean, non-cramped centered modal popup with generous padding
pub fn draw_modal_popup(canvas: &mut Canvas, title: Option<&str>, msg: &str) {
    draw_modal_popup_materialize(canvas, title, msg, 16);
}

/// Clean centered modal progress dialog box
pub fn draw_modal_progress(canvas: &mut Canvas, title: &str, subtitle: &str, progress: u8) {
    let w = MODAL_BOX_W;
    let h = MODAL_BOX_H;
    let (bx, by) = draw_modal_box(canvas, w, h);
    let center_x = (bx as i16) + ((w as i16) >> 1);

    let has_title = !title.is_empty() && !text::ascii_eq_ignore_case(title, "passcode");

    if has_title {
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
        // Subtitle only (e.g. "Verifying..."), cleanly centered in MUMATRIX font
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

    let bar_w = 96u8;
    let bar_x = center_x - ((bar_w as i16) >> 1);
    let bar_y = (by + 20) as i16;
    let bar_h = 6u8;

    draw_progress_bar(canvas, bar_x, bar_y, bar_w, bar_h, progress, true);
}

/// Draw an authentic KERN inverted filled key badge (e.g. [M] Format)
#[inline(never)]
fn key_badge_key_width(key_str: &str) -> u8 {
    match key_str.as_bytes() {
        b"^" | b"v" | b"V" => 5,
        _ => text::str_width(key_str, &crate::fonts::tomthumb::FONT) as u8,
    }
}

pub fn draw_key_badge(canvas: &mut Canvas, x: i16, y: i16, key_str: &str, label: &str) {
    let font = &crate::fonts::tomthumb::FONT;
    let box_w = key_badge_key_width(key_str) + 4;

    canvas.fill_rect(x as u8, y as u8, box_w, 7, Color::Fill);

    match key_str {
        "^" | "v" | "V" => {
            let is_up = key_str == "^";
            for dx in 0..5u8 {
                let d = dx.abs_diff(2) as i16;
                let py = if is_up { y + 2 + d } else { y + 4 - d };
                canvas.draw_pixel((x + 2 + dx as i16) as u8, py as u8, Color::Clear);
            }
        }
        _ => text::draw_str(canvas, x + 2, y + 5, key_str, font, Color::Clear),
    }

    if !label.is_empty() {
        text::draw_str(
            canvas,
            x + (box_w as i16) + 2,
            y + 5,
            label,
            font,
            Color::Fill,
        );
    }
}

/// Draw a right-aligned KERN inverted filled key badge
#[inline(never)]
pub fn draw_key_badge_right(canvas: &mut Canvas, right_x: i16, y: i16, key_str: &str, label: &str) {
    let font = &crate::fonts::tomthumb::FONT;
    let key_w = key_badge_key_width(key_str) as i16;
    let label_w = if label.is_empty() {
        0
    } else {
        text::str_width(label, font) + 2
    };
    let total_w = key_w + 4 + label_w;
    draw_key_badge(canvas, right_x - total_w, y, key_str, label);
}

/// Draw standard inverted (negated) header bar across the screen width.
pub fn draw_negated_header(canvas: &mut Canvas, y: i16, title: &str) {
    let yu = y as u8;
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

fn draw_dialog_header(canvas: &mut Canvas, dy: i16, big_title: &str, sub_title: &str) {
    canvas.clear();
    draw_negated_header(canvas, dy, "KERN");
    text::draw_str(
        canvas,
        4,
        22 + dy,
        big_title,
        &crate::fonts::mumatrix_regular::FONT,
        Color::Fill,
    );
    if !sub_title.is_empty() {
        text::draw_str(
            canvas,
            4,
            34 + dy,
            sub_title,
            &crate::fonts::tomthumb::FONT,
            Color::Fill,
        );
    }
}

/// Minimalist full-screen system recovery or modal notification dialog
pub fn draw_dialog(
    canvas: &mut Canvas,
    big_title: &str,
    sub_title: &str,
    hint: &str,
    has_btns: bool,
) {
    draw_dialog_slide_up(canvas, big_title, sub_title, hint, has_btns, 0);
}

/// System dialog with a Hawk5-style progress bar
pub fn draw_dialog_progress(
    canvas: &mut Canvas,
    big_title: &str,
    sub_title: &str,
    progress_percent: u8,
) {
    draw_dialog_header(canvas, 0, big_title, sub_title);
    draw_progress_bar(canvas, 4, 44, 120, 8, progress_percent, true);
}

/// Shared bottom-sheet modal frame (inverted header, outer border, clear background, optional badges)
pub fn draw_sheet_frame(
    canvas: &mut Canvas,
    title: &str,
    sheet_y: i16,
    sheet_h: u8,
    badges: Option<(&str, &str)>,
) {
    let base_y = 64i16.saturating_sub(sheet_h as i16);
    let y_offset = (sheet_y - base_y).max(0) as i8;
    draw_sheet_slide_up(canvas, title, sheet_h, badges, y_offset);
}

/// Shared ultra-compact scrollbar renderer
#[inline(never)]
pub fn draw_scrollbar(
    canvas: &mut Canvas,
    track_x: u8,
    track_y: u8,
    track_h: u8,
    current: usize,
    total: usize,
    visible: usize,
) {
    if total <= visible || total == 0 {
        return;
    }
    canvas.draw_vertical_line(track_x + 1, track_y, track_h, Color::DarkGray);
    let thumb_h = ((visible as u32 * track_h as u32) / total as u32).max(4) as u8;
    let max_scroll = (total - visible) as u32;
    let rem_h = (track_h - thumb_h) as u32;
    let cur = current.min(total - visible) as u32;
    let thumb_y = track_y + ((cur * rem_h) / max_scroll) as u8;
    let thumb_y = thumb_y.min(track_y + track_h - thumb_h);
    canvas.fill_rect(track_x, thumb_y, 3, thumb_h, Color::Fill);
}

/// Dialog rendered with a vertical slide-up offset (y_offset = 0 is docked; y_offset > 0 shifts down)
pub fn draw_dialog_slide_up(
    canvas: &mut Canvas,
    big_title: &str,
    sub_title: &str,
    hint: &str,
    has_btns: bool,
    y_offset: i8,
) {
    let dy = y_offset.max(0) as i16;
    if dy >= 64 {
        return;
    }
    draw_dialog_header(canvas, dy, big_title, sub_title);
    if !hint.is_empty() && (44 + dy) < 64 {
        text::draw_str(
            canvas,
            4,
            44 + dy,
            hint,
            &crate::fonts::tomthumb::FONT,
            Color::Fill,
        );
    }
    if has_btns && (55 + dy) < 64 {
        draw_key_badge(canvas, 4, 55 + dy, "M", "Format");
        draw_key_badge_right(canvas, 124, 55 + dy, "EXIT", "Cancel");
    }
}

/// Shared bottom-sheet modal frame rendered with a vertical slide-up offset
pub fn draw_sheet_slide_up(
    canvas: &mut Canvas,
    title: &str,
    sheet_h: u8,
    badges: Option<(&str, &str)>,
    y_offset: i8,
) {
    let base_y = 64i16.saturating_sub(sheet_h as i16);
    let sheet_y = (base_y + y_offset.max(0) as i16).min(64);
    if sheet_y >= 64 {
        return;
    }
    let visible_h = (64 - sheet_y).min(sheet_h as i16) as u8;
    let sy = sheet_y as u8;
    canvas.fill_rect(0, sy, 128, visible_h, Color::Clear);
    canvas.draw_rect(0, sy, 128, visible_h, Color::Fill);
    canvas.clear_corners(0, sy, 128, visible_h);
    draw_negated_header(canvas, sheet_y, title);

    if let Some((confirm, cancel)) = badges {
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
}

/// Clean centered modal popup with zoom materialization (progress: 0..=16)
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
    let bw = ((MODAL_BOX_W as u16 * p as u16) >> 4) as u8;
    let bh = ((MODAL_BOX_H as u16 * p as u16) >> 4) as u8;
    let bx = (128 - bw) >> 1;
    let by = (64 - bh) >> 1;

    canvas.fill_rect(bx, by, bw, bh, Color::Clear);
    canvas.draw_rect(bx, by, bw, bh, Color::Fill);
    if bw > 4 && bh > 4 {
        canvas.clear_corners(bx, by, bw, bh);
    }

    // High-contrast text reveals as materialization expands (>= 8)
    if p >= 8 {
        let center_x = 64i16;
        let tc = Color::Fill;
        let has_title =
            title.is_some_and(|t| !t.is_empty() && !text::ascii_eq_ignore_case(t, "passcode"));

        let (mut line_y, step_y) = if has_title {
            text::draw_str_align(
                canvas,
                center_x,
                (by + 11) as i16,
                title.unwrap_or(""),
                &crate::fonts::tomthumb::FONT,
                tc,
                text::Align::Center,
            );
            ((by + 21) as i16, 8i16)
        } else {
            let y0 = if crate::text::count_lines(msg) <= 1 {
                (by + 20) as i16
            } else {
                (by + 14) as i16
            };
            (y0, 12i16)
        };

        let mut is_first = true;
        for line in crate::text::lines(msg) {
            let font = if !has_title && is_first {
                &crate::fonts::mumatrix_regular::FONT
            } else {
                &crate::fonts::tomthumb::FONT
            };
            text::draw_str_align(
                canvas,
                center_x,
                line_y,
                line,
                font,
                tc,
                text::Align::Center,
            );
            line_y += step_y;
            is_first = false;
        }
    }
}
