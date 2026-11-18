//! Bottom-Sheet Dialog Box Module (Inverted Left-Aligned Header + Internal Padding + Scrollbar)

use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::view::View;
use crate::text;
use crate::{Canvas, Color};
use kernel::event::{Event, KeyCode, KeyState};

use heapless::String;

use crate::widgets;

#[repr(C)]
pub struct PopupModel {
    pub hold: crate::gui::animation::HoldProgress,
    pub anim: crate::gui::animation::TransitionProgress,
    pub scroll_offset: usize,
    pub repeater: crate::gui::repeater::KeyRepeater,
    pub confirm_label: Option<&'static str>,
    pub cancel_label: Option<&'static str>,
    pub custom_title: String<24>,
    pub custom_msg: String<192>,
}

impl PopupModel {
    pub const fn new() -> Self {
        Self {
            hold: crate::gui::animation::HoldProgress::new(),
            anim: crate::gui::animation::TransitionProgress::new(4),
            scroll_offset: 0,
            confirm_label: None,
            cancel_label: None,
            repeater: crate::gui::repeater::KeyRepeater::new(8, 2),
            custom_title: String::new(),
            custom_msg: String::new(),
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

pub struct PopupView;

impl PopupView {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for PopupView {
    fn default() -> Self {
        Self::new()
    }
}

#[inline(always)]
fn popup_visible_lines(model: &PopupModel) -> usize {
    if model.hold.required_ms > 0 {
        2
    } else if model.confirm_label.is_some() || model.cancel_label.is_some() {
        3
    } else {
        4
    }
}

#[inline(always)]
fn popup_max_scroll(model: &PopupModel) -> usize {
    text::count_lines(model.custom_msg.as_str()).saturating_sub(popup_visible_lines(model))
}

fn scroll_popup(model: &mut PopupModel, code: KeyCode) -> bool {
    match code {
        KeyCode::Up | KeyCode::Key2 => {
            if model.scroll_offset > 0 {
                model.scroll_offset -= 1;
            }
            true
        }
        KeyCode::Down | KeyCode::Key8 => {
            if model.scroll_offset < popup_max_scroll(model) {
                model.scroll_offset += 1;
            }
            true
        }
        _ => false,
    }
}

impl View<PopupModel> for PopupView {
    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, model: &PopupModel) {
        let has_badges = model.confirm_label.is_some() || model.cancel_label.is_some();
        let msg = model.custom_msg.as_str();
        let total_lines = text::count_lines(msg);

        // Short unbadged notifications / alerts (e.g. "Passcode Set", "Wrong Passcode", "Mismatch"):
        let anim_p = model.anim.progress();
        if !has_badges && total_lines <= 2 && msg.len() < 60 {
            let t = model.title_str();
            let title_opt = (!t.is_empty()).then_some(t);
            widgets::draw_modal_popup_materialize(canvas, title_opt, msg, anim_p);
            return;
        }

        // Standard bottom sheet occupies Y=18 to Y=63 (46px high)
        let sheet_h = 46u8;
        let y_offset = model.anim.slide_offset(sheet_h);
        let sheet_y = 18i16 + y_offset as i16;

        let badges = has_badges.then(|| {
            let confirm = model
                .confirm_label
                .unwrap_or(if model.hold.required_ms > 0 {
                    "Hold"
                } else {
                    ""
                });
            (confirm, model.cancel_label.unwrap_or(""))
        });
        widgets::draw_sheet_slide_up(canvas, model.title_str(), sheet_h, badges, y_offset);

        // Body text rendering (TomThumb, multi-line aware with padding)
        let visible_lines = popup_visible_lines(model);
        let line_height = 7i16;
        let start_y = sheet_y + 14;

        let max_scroll = total_lines.saturating_sub(visible_lines);
        let scroll_offset = model.scroll_offset.min(max_scroll);

        if anim_p >= 8 {
            for (idx, line) in text::lines(msg).enumerate() {
                if idx >= scroll_offset && idx < scroll_offset + visible_lines {
                    let line_y = start_y + ((idx - scroll_offset) as i16 * line_height);
                    if line_y < 64 {
                        text::draw_str(canvas, 4, line_y, line, &TOMTHUMB, Color::Fill);
                    }
                }
            }
        }

        // Hold-to-confirm balanced 6px sub-pixel progress bar
        if model.hold.required_ms > 0 && anim_p >= 8 {
            widgets::draw_progress_bar(canvas, 4, sheet_y + 25, 120, 6, model.hold_percent(), true);
        }

        // Integrated Scrollbar (X=124..125) when total lines exceed viewport
        if total_lines > visible_lines && anim_p >= 12 {
            let track_y = (sheet_y + 10) as u8;
            let track_h = if has_badges { 24u8 } else { 31u8 };
            widgets::draw_scrollbar(
                canvas,
                124,
                track_y,
                track_h,
                scroll_offset,
                total_lines,
                visible_lines,
            );
        }
    }

    #[inline(never)]
    fn on_event(&mut self, model: &mut PopupModel, event: &Event) -> bool {
        if let Event::Tick = event {
            let anim_tick = model.anim.tick();
            let rep_tick = model
                .repeater
                .tick()
                .is_some_and(|c| scroll_popup(model, c));
            return anim_tick || rep_tick || model.is_hold_animating();
        }

        if let Event::Key { code, state } = event {
            model.repeater.handle_key(*code, *state);
            if matches!(code, KeyCode::Menu | KeyCode::Key5) && model.hold.required_ms > 0 {
                model.hold.active = *state != KeyState::Released;
                return true;
            }
            if *state == KeyState::Pressed {
                if scroll_popup(model, *code) {
                    return true;
                }
                return !matches!(code, KeyCode::Ptt | KeyCode::Menu | KeyCode::Exit);
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_confirm_header_omits_countdown_text() {
        let mut model = PopupModel::new().with_badges("Hold", "Cancel");
        let _ = model.custom_title.push_str("DFU MODE");
        let _ = model.custom_msg.push_str("Reboot to bootloader?");
        model.hold.required_ms = 1000;
        model.hold.elapsed_ms = 500;
        model.anim.jump_complete();

        let mut canvas = Canvas::new();
        PopupView::new().render(&mut canvas, &model);

        // Negated header bar occupies Y=18..26 (bits 2..=7 in page 2). Top-right X=100..=125
        // must remain unbroken Fill with no cleared countdown glyph pixels.
        for x in 100..=125 {
            assert_eq!(canvas.buffer[2][x] & 0xFC, 0xFC);
        }
        // Hold progress bar outline at Y=43..48 (page 5 bits 3..=7) should still render.
        assert_ne!(canvas.buffer[5][64] & 0xF8, 0);
    }

    #[test]
    fn test_hold_badge_displays_hold_when_active() {
        let mut model = PopupModel::new().with_badges("Hold", "Cancel");
        let _ = model.custom_title.push_str("DELETE");
        let _ = model.custom_msg.push_str("file.krx\nCannot be undone.");
        model.hold.required_ms = 1500;
        model.hold.elapsed_ms = 750;
        model.hold.active = true;
        model.anim.jump_complete();

        let mut canvas_active = Canvas::new();
        PopupView::new().render(&mut canvas_active, &model);

        model.hold.active = false;
        let mut canvas_inactive = Canvas::new();
        PopupView::new().render(&mut canvas_inactive, &model);

        // Badge area on bottom bar (page 6/7, X=3..25) must match exactly
        // whether active or inactive because the badge shows "Hold" in both states.
        for x in 3..25 {
            assert_eq!(canvas_active.buffer[6][x], canvas_inactive.buffer[6][x]);
            assert_eq!(canvas_active.buffer[7][x], canvas_inactive.buffer[7][x]);
        }
    }
}
