//! Unified Modal, Bottom-Sheet, Lock-Sheet & System Dialog View (`DialogView`, `DialogState`).

use super::View;
use super::animation::{HoldProgress, TransitionProgress};
use super::repeater::KeyRepeater;
use crate::elements::{
    draw_key_badge, draw_key_badge_right, draw_negated_header, draw_progress_bar, draw_scrollbar,
    draw_sheet_badges, draw_sheet_slide_up,
};
use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::icons::Icon;
use crate::text::{self, Align};
use crate::{Canvas, Color};
use kernel::event::{Event, KeyCode, KeyState};
use utils::format::InlineStr;

#[repr(C)]
pub struct DialogState {
    pub hold: HoldProgress,
    pub anim: TransitionProgress,
    pub scroll_offset: u8,
    pub repeater: KeyRepeater,
    pub confirm_label: Option<&'static str>,
    pub cancel_label: Option<&'static str>,
    pub custom_title: InlineStr<24>,
    pub custom_msg: InlineStr<88>,
}

impl DialogState {
    pub const fn new() -> Self {
        Self {
            hold: HoldProgress::new(),
            anim: TransitionProgress::new(4),
            scroll_offset: 0,
            repeater: KeyRepeater::new(8, 2),
            confirm_label: None,
            cancel_label: None,
            custom_title: InlineStr::new(),
            custom_msg: InlineStr::new(),
        }
    }

    pub const fn with_badges(mut self, confirm: &'static str, cancel: &'static str) -> Self {
        self.confirm_label = Some(confirm);
        self.cancel_label = Some(cancel);
        self
    }

    #[inline(always)]
    pub fn is_hold_animating(&self) -> bool {
        self.hold.is_animating()
    }

    #[inline(always)]
    pub fn hold_percent(&self) -> u8 {
        self.hold.percent()
    }

    #[inline(always)]
    pub fn advance_hold(&mut self, dt_ms: u16) -> bool {
        self.hold.advance(dt_ms)
    }

    #[inline(always)]
    pub fn title_str(&self) -> &str {
        self.custom_title.as_str()
    }
}

pub struct DialogView;

impl DialogView {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DialogView {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn visible_lines(model: &DialogState) -> usize {
    if model.hold.required_ms > 0 {
        2
    } else if model.confirm_label.is_some() || model.cancel_label.is_some() {
        3
    } else {
        4
    }
}

#[inline(always)]
fn scroll_dir(model: &mut DialogState, down: bool) {
    let max_scroll =
        text::count_lines(model.custom_msg.as_str()).saturating_sub(visible_lines(model));
    if !down {
        model.scroll_offset = model.scroll_offset.saturating_sub(1);
    } else if (model.scroll_offset as usize) < max_scroll {
        model.scroll_offset += 1;
    }
}

fn scroll_key(model: &mut DialogState, code: KeyCode) -> bool {
    let down = match code {
        KeyCode::Up | KeyCode::Key2 => false,
        KeyCode::Down | KeyCode::Key8 => true,
        _ => return false,
    };
    scroll_dir(model, down);
    true
}

impl View<DialogState> for DialogView {
    fn render(&self, canvas: &mut Canvas, model: &DialogState) {
        let has_badges = model.confirm_label.is_some() || model.cancel_label.is_some();
        let msg = model.custom_msg.as_str();
        let total_lines = text::count_lines(msg);
        let anim_p = model.anim.progress();

        let sheet_h: u8 = if model.hold.required_ms > 0 || total_lines >= 3 {
            46
        } else {
            24 + u8::from(total_lines > 1) * 7 + u8::from(has_badges) * 8
        };
        let sheet_y = draw_sheet_slide_up(
            canvas,
            model.title_str(),
            sheet_h,
            model.anim.slide_offset(sheet_h),
        );
        if has_badges {
            let confirm = model
                .confirm_label
                .unwrap_or(if model.hold.required_ms > 0 {
                    "Hold"
                } else {
                    ""
                });
            draw_sheet_badges(
                canvas,
                sheet_y,
                sheet_h,
                confirm,
                model.cancel_label.unwrap_or(""),
            );
        }

        let vis = visible_lines(model);
        let scroll_offset = (model.scroll_offset as usize).min(total_lines.saturating_sub(vis));

        if anim_p >= 8 {
            for (idx, line) in text::lines(msg).enumerate() {
                if idx >= scroll_offset && idx < scroll_offset + vis {
                    let line_y = sheet_y + 14 + ((idx - scroll_offset) as i16 * 7);
                    if line_y < 64 {
                        text::draw_str(canvas, 4, line_y, line, &TOMTHUMB, Color::Fill);
                    }
                }
            }
            if model.hold.required_ms > 0 {
                draw_progress_bar(canvas, 4, sheet_y + 25, 120, 6, model.hold_percent(), true);
            }
        }

        if total_lines > vis && anim_p >= 12 {
            let track_h = if has_badges { 24u8 } else { 31u8 };
            draw_scrollbar(
                canvas,
                124,
                (sheet_y + 10) as u8,
                track_h,
                scroll_offset,
                total_lines,
                vis,
            );
        }
    }

    fn on_event(&mut self, model: &mut DialogState, event: &Event) -> bool {
        if let Event::Tick = event {
            let anim_tick = model.anim.tick();
            let rep_tick = model.repeater.tick().is_some_and(|c| scroll_key(model, c));
            return anim_tick || rep_tick || model.is_hold_animating();
        }
        if let Event::Encoder(delta) = *event {
            if delta == 0 {
                return false;
            }
            for _ in 0..delta.unsigned_abs().min(8) {
                scroll_dir(model, delta > 0);
            }
            return true;
        }
        if let Event::Key { code, state } = event {
            model.repeater.handle_key(*code, *state);
            if matches!(code, KeyCode::Menu | KeyCode::Key5) && model.hold.required_ms > 0 {
                model.hold.active = *state != KeyState::Released;
                return true;
            }
            if *state == KeyState::Pressed {
                if scroll_key(model, *code) {
                    return true;
                }
                return !matches!(
                    code,
                    KeyCode::Ptt | KeyCode::Menu | KeyCode::Key5 | KeyCode::Exit
                );
            }
        }
        false
    }
}

/// Shared slide-up lock/sleep/force-close sheet renderer used by `KeypadLockManager`.
pub fn draw_lock_sheet(
    canvas: &mut Canvas,
    title: &str,
    msg: &str,
    icon: Icon,
    sheet_h: u8,
    hold_pct: Option<u8>,
    unlock_hint: bool,
    y_offset: i8,
    visible_body: bool,
) {
    let sheet_y = draw_sheet_slide_up(canvas, title, sheet_h, y_offset);
    if sheet_y >= 64 || !visible_body {
        return;
    }
    if let Some(pct) = hold_pct {
        crate::icons::draw_icon(canvas, 6, sheet_y + 11, icon, Color::Fill);
        text::draw_str(canvas, 17, sheet_y + 17, msg, &TOMTHUMB, Color::Fill);
        draw_progress_bar(canvas, 4, sheet_y + 23, 120, 6, pct, true);
    } else if unlock_hint {
        text::draw_str(canvas, 32, sheet_y + 23, "HOLD", &TOMTHUMB, Color::Fill);
        draw_key_badge(canvas, 50, sheet_y + 18, "#", "TO UNLOCK");
    } else {
        let total_w = 11 + text::str_width(msg, &TOMTHUMB);
        let start_x = 64 - (total_w >> 1);
        crate::icons::draw_icon(canvas, start_x, sheet_y + 17, icon, Color::Fill);
        text::draw_str(
            canvas,
            start_x + 11,
            sheet_y + 23,
            msg,
            &TOMTHUMB,
            Color::Fill,
        );
    }
}

fn draw_dialog_header(canvas: &mut Canvas, big_title: &str, sub_title: &str) {
    canvas.clear();
    draw_negated_header(canvas, 0, "KERN");
    text::draw_str(canvas, 4, 22, big_title, &MUMATRIX, Color::Fill);
    if !sub_title.is_empty() {
        text::draw_str(canvas, 4, 34, sub_title, &TOMTHUMB, Color::Fill);
    }
}

pub fn draw_dialog(
    canvas: &mut Canvas,
    big_title: &str,
    sub_title: &str,
    hint: &str,
    _has_btns: bool,
) {
    draw_dialog_header(canvas, big_title, sub_title);
    if !hint.is_empty() {
        text::draw_str(canvas, 4, 44, hint, &TOMTHUMB, Color::Fill);
    }
}

pub fn draw_dialog_progress(
    canvas: &mut Canvas,
    big_title: &str,
    sub_title: &str,
    progress_percent: u8,
) {
    draw_dialog_header(canvas, big_title, sub_title);
    draw_progress_bar(canvas, 4, 43, 120, 6, progress_percent, true);
}

pub fn draw_dialog_hold_prompt(
    canvas: &mut Canvas,
    big_title: &str,
    sub_title: &str,
    confirm_label: &str,
    cancel_label: &str,
    progress_percent: u8,
) {
    draw_dialog_progress(canvas, big_title, sub_title, progress_percent);
    draw_key_badge(canvas, 4, 55, "M", confirm_label);
    draw_key_badge_right(canvas, 124, 55, "EXIT", cancel_label);
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TextLine {
    pub y: u8,
    pub font: u8,
    pub align: u8,
    pub text: &'static str,
}

impl TextLine {
    pub const fn new(y: u8, font: u8, align: u8, text: &'static str) -> Self {
        Self {
            y,
            font,
            align,
            text,
        }
    }
}

pub fn draw_text_screen(
    canvas: &mut Canvas,
    title: &str,
    lines: &[TextLine],
    dyn_fn: Option<fn(&mut Canvas)>,
) {
    canvas.clear();
    if !title.is_empty() {
        canvas.fill_rect(0, 0, 128, 10, Color::Fill);
        text::draw_str_align(canvas, 64, 7, title, &TOMTHUMB, Color::Clear, Align::Center);
    }
    for line in lines {
        let font = match line.font {
            1 => &MUMATRIX,
            _ => &TOMTHUMB,
        };
        let (x, align) = match line.align {
            1 => (64i16, Align::Center),
            2 => (124i16, Align::Right),
            _ => (4i16, Align::Left),
        };
        text::draw_str_align(
            canvas,
            x,
            line.y as i16,
            line.text,
            font,
            Color::Fill,
            align,
        );
    }
    if let Some(hook) = dyn_fn {
        hook(canvas);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::draw_modal_box;

    #[test]
    fn short_confirm_and_hold_dialog_render_cleanly() {
        let mut model = DialogState::new().with_badges("OK", "");
        model.custom_title.push_str("Radio App");
        model
            .custom_msg
            .push_str("Not installed.\nSync via Kern Dock.");
        model.anim.jump_complete();

        let mut canvas = Canvas::new();
        DialogView::new().render(&mut canvas, &model);
        assert!(canvas.buffer[0].iter().all(|&b| b == 0));
        assert!(canvas.buffer[3][20] != 0);
        assert!(canvas.buffer[7][6] != 0);

        let mut hold_canvas = Canvas::new();
        draw_dialog_hold_prompt(
            &mut hold_canvas,
            "Invalid Storage",
            "Calib OK",
            "Hold",
            "Cancel",
            50,
        );
        assert_ne!(hold_canvas.buffer[0][64], 0);
        assert_ne!(hold_canvas.buffer[5][64] & 0xF8, 0);

        let mut backdrop = Canvas::new();
        backdrop.fill_rect(0, 0, 128, 64, Color::Fill);
        backdrop.dim_backdrop(0);
        let (bx, by) = draw_modal_box(&mut backdrop, 60, 30);
        for x in (bx - 1)..=(bx + 60) {
            let page = ((by - 1) >> 3) as usize;
            let bit = 1u8 << ((by - 1) & 7);
            assert_eq!(backdrop.buffer[page][x as usize] & bit, 0);
        }
    }
}
