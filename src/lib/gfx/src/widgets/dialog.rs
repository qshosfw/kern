use super::progress::draw_progress_bar;
use crate::text;
use crate::{Canvas, Color};

/// Standard modal dialog viewport constants matching text input box
pub const MODAL_BOX_X: u8 = 2;
pub const MODAL_BOX_Y: u8 = 18;
pub const MODAL_BOX_W: u8 = 124;
pub const MODAL_BOX_H: u8 = 35;

/// Center-Screen Modal Outer Box with Clear background and rounded border
pub fn draw_modal_box(canvas: &mut Canvas, w: u8, h: u8) -> (u8, u8) {
    let (x, y) = if w == MODAL_BOX_W && h == MODAL_BOX_H {
        (MODAL_BOX_X, MODAL_BOX_Y)
    } else {
        (
            canvas.width().saturating_sub(w) / 2,
            canvas.height().saturating_sub(h) / 2,
        )
    };
    canvas.fill_rect(x, y, w, h, Color::Clear);
    canvas.draw_rect(x, y, w, h, Color::Fill);
    canvas.draw_pixel(x, y, Color::Clear);
    canvas.draw_pixel(x + w - 1, y, Color::Clear);
    canvas.draw_pixel(x, y + h - 1, Color::Clear);
    canvas.draw_pixel(x + w - 1, y + h - 1, Color::Clear);
    (x, y)
}

/// Clean, non-cramped centered modal popup with generous padding
pub fn draw_modal_popup(canvas: &mut Canvas, title: Option<&str>, msg: &str) {
    let (bx, by) = draw_modal_box(canvas, MODAL_BOX_W, MODAL_BOX_H);
    let center_x = (bx as i16) + (MODAL_BOX_W as i16 / 2);

    if let Some(t) = title {
        if !t.is_empty() && !text::ascii_eq_ignore_case(t, "passcode") {
            text::draw_str_align(
                canvas,
                center_x,
                (by + 11) as i16,
                t,
                &crate::fonts::tomthumb::FONT,
                Color::Fill,
                text::Align::Center,
            );
            let mut line_y = (by + 21) as i16;
            crate::text::for_each_line(msg, |line| {
                text::draw_str_align(
                    canvas,
                    center_x,
                    line_y,
                    line,
                    &crate::fonts::tomthumb::FONT,
                    Color::Fill,
                    text::Align::Center,
                );
                line_y += 8;
            });
            return;
        }
    }

    // No title: render 1 or 2 lines centered
    let mut line_y = if crate::text::count_lines(msg) <= 1 {
        (by + 20) as i16
    } else {
        (by + 14) as i16
    };
    let mut is_first = true;
    crate::text::for_each_line(msg, |line| {
        let font = if is_first {
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
            Color::Fill,
            text::Align::Center,
        );
        line_y += 12;
        is_first = false;
    });
}

/// Clean centered modal progress dialog box
pub fn draw_modal_progress(canvas: &mut Canvas, title: &str, subtitle: &str, progress: u8) {
    let w = MODAL_BOX_W;
    let h = MODAL_BOX_H;
    let (bx, by) = draw_modal_box(canvas, w, h);
    let center_x = (bx as i16) + (w as i16 / 2);

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
    let bar_x = (center_x - (bar_w as i16) / 2) as i16;
    let bar_y = (by + 20) as i16;
    let bar_h = 6u8;

    draw_progress_bar(canvas, bar_x, bar_y, bar_w, bar_h, progress, true);
}

/// Animated 4-frame 5x5 rotating pixel loading spinner
pub fn draw_loading_spinner(canvas: &mut Canvas, x: i16, y: i16, frame: u8, color: Color) {
    let f = frame % 4;
    let (dx1, dy1, dx2, dy2) = match f {
        0 => (2i16, 0i16, 4i16, 2i16),
        1 => (4i16, 2i16, 2i16, 4i16),
        2 => (2i16, 4i16, 0i16, 2i16),
        _ => (0i16, 2i16, 2i16, 0i16),
    };
    canvas.draw_pixel((x + 2) as u8, (y + 2) as u8, color); // Center axis dot
    canvas.draw_pixel((x + dx1) as u8, (y + dy1) as u8, color);
    canvas.draw_pixel((x + dx2) as u8, (y + dy2) as u8, color);
}

/// Draw an authentic KERN inverted filled key badge (e.g. [M] Format)
pub fn draw_key_badge(canvas: &mut Canvas, x: i16, y: i16, key_str: &str, label: &str) {
    let font = &crate::fonts::tomthumb::FONT;
    let key_w = text::str_width(key_str, font) as u8;
    let box_w = key_w + 4; // 2px padding on each side
    canvas.fill_rect(x as u8, y as u8, box_w, 7, Color::Fill);
    text::draw_str(canvas, x + 2, y + 5, key_str, font, Color::Clear);
    text::draw_str(
        canvas,
        x + (box_w as i16) + 2,
        y + 5,
        label,
        font,
        Color::Fill,
    );
}

/// Draw a right-aligned KERN inverted filled key badge
pub fn draw_key_badge_right(canvas: &mut Canvas, right_x: i16, y: i16, key_str: &str, label: &str) {
    let font = &crate::fonts::tomthumb::FONT;
    let total_w = text::str_width(key_str, font) + 6 + text::str_width(label, font);
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

fn draw_dialog_header(canvas: &mut Canvas, big_title: &str, sub_title: &str) {
    canvas.clear();
    draw_negated_header(canvas, 0, "KERN");
    text::draw_str(
        canvas,
        4,
        22,
        big_title,
        &crate::fonts::mumatrix_regular::FONT,
        Color::Fill,
    );
    if !sub_title.is_empty() {
        text::draw_str(
            canvas,
            4,
            34,
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
    draw_dialog_header(canvas, big_title, sub_title);

    if !hint.is_empty() {
        text::draw_str(
            canvas,
            4,
            44,
            hint,
            &crate::fonts::tomthumb::FONT,
            Color::Fill,
        );
    }

    if has_btns {
        draw_key_badge(canvas, 4, 55, "M", "Format");
        draw_key_badge_right(canvas, 124, 55, "EXIT", "Cancel");
    }
}

/// System dialog with a Hawk5-style progress bar
pub fn draw_dialog_progress(
    canvas: &mut Canvas,
    big_title: &str,
    sub_title: &str,
    progress_percent: u8,
) {
    draw_dialog_header(canvas, big_title, sub_title);
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
    let sy = sheet_y as u8;
    canvas.fill_rect(0, sy, 128, sheet_h, Color::Clear);
    canvas.draw_rect(0, sy, 128, sheet_h, Color::Fill);
    canvas.draw_pixel(0, sy, Color::Clear);
    canvas.draw_pixel(127, sy, Color::Clear);
    let bottom_y = sy + sheet_h - 1;
    canvas.draw_pixel(0, bottom_y, Color::Clear);
    canvas.draw_pixel(127, bottom_y, Color::Clear);
    draw_negated_header(canvas, sheet_y, title);

    if let Some((confirm, cancel)) = badges {
        let by = (sheet_y as u8 + sheet_h).saturating_sub(10) as i16;
        draw_key_badge(canvas, 3, by, "M", confirm);
        draw_key_badge_right(canvas, 125, by, "EXIT", cancel);
    }
}

/// Shared ultra-compact scrollbar renderer
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
    canvas.draw_vertical_line(track_x + 1, track_y, track_h, Color::Fill);
    let thumb_h = ((visible as u32 * track_h as u32) / total as u32).max(4) as u8;
    let max_scroll = total.saturating_sub(visible);
    let thumb_y = track_y
        + if max_scroll == 0 {
            0
        } else {
            ((current.min(max_scroll) as u32 * (track_h - thumb_h) as u32) / max_scroll as u32)
                as u8
        };
    let thumb_y = thumb_y.min(track_y + track_h - thumb_h);
    canvas.fill_rect(track_x, thumb_y, 3, thumb_h, Color::Fill);
}
