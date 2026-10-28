//! Declarative NumberPicker Modal GUI Module
//! Provides an interactive numeric selector with min, max, step, units, and fast repeat support.

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::gui::view::View;
use crate::text;
use crate::widgets;
use crate::{Canvas, Color};
use heapless::String;
use kernel::event::{Event, KeyCode, KeyState};

pub struct NumberPickerModel {
    pub title: &'static str,
    pub val: i32,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub unit: &'static str,
    pub is_done: bool,
    pub is_canceled: bool,
    pub confirm_label: &'static str,
    pub cancel_label: &'static str,
}

impl NumberPickerModel {
    pub const fn new(
        title: &'static str,
        val: i32,
        min: i32,
        max: i32,
        step: i32,
        unit: &'static str,
    ) -> Self {
        Self {
            title,
            val,
            min,
            max,
            step,
            unit,
            is_done: false,
            is_canceled: false,
            confirm_label: "OK",
            cancel_label: "Cancel",
        }
    }

    pub fn with_labels(mut self, confirm: &'static str, cancel: &'static str) -> Self {
        self.confirm_label = confirm;
        self.cancel_label = cancel;
        self
    }

    pub fn increment(&mut self) {
        self.val = (self.val + self.step).min(self.max);
    }

    pub fn decrement(&mut self) {
        self.val = (self.val - self.step).max(self.min);
    }

    pub fn increment_fast(&mut self) {
        let fast_step = self.step * 5;
        self.val = (self.val + fast_step).min(self.max);
    }

    pub fn decrement_fast(&mut self) {
        let fast_step = self.step * 5;
        self.val = (self.val - fast_step).max(self.min);
    }
}

pub struct NumberPickerView {
    pub repeater: crate::gui::repeater::KeyRepeater,
}

impl NumberPickerView {
    pub const fn new() -> Self {
        Self {
            repeater: crate::gui::repeater::KeyRepeater::new(8, 2),
        }
    }
}

impl View<NumberPickerModel> for NumberPickerView {
    fn render(&self, canvas: &mut Canvas, model: &NumberPickerModel) {
        let sheet_y = 16i16;
        let sheet_h = 48u8;

        // 1. Draw frame, rounded border, background and inverted header
        crate::widgets::draw_sheet_frame(canvas, model.title, sheet_y, sheet_h, None);

        // 3. Central Value Display Box (Y=28..46, centered)
        let box_y = sheet_y + 12;
        canvas.draw_rect(24, box_y as u8, 80, 18, Color::Fill);

        // Left and Right indicator arrows
        if model.val > model.min {
            text::draw_str(canvas, 28, box_y + 12, "<", &MUMATRIX, Color::Fill);
        }
        if model.val < model.max {
            text::draw_str(canvas, 96, box_y + 12, ">", &MUMATRIX, Color::Fill);
        }

        // Format value + unit string
        let mut buf: String<24> = String::new();
        let mut nbuf = [0u8; 12];
        let _ = buf.push_str(text::format_i32(model.val, &mut nbuf));
        if !model.unit.is_empty() {
            let _ = buf.push(' ');
            let _ = buf.push_str(model.unit);
        }

        // Center formatted value string inside the box
        let text_w = buf.len() as i16 * 6; // ~6px average char width in MuMatrix
        let text_x = 24 + ((80 - text_w) / 2).max(10);
        text::draw_str(
            canvas,
            text_x,
            box_y + 12,
            buf.as_str(),
            &MUMATRIX,
            Color::Fill,
        );

        // 4. Progress indicator track (Y=48..51)
        let track_y = (sheet_y + 32) as u8;
        let track_w = 80u8;
        let track_x = 24u8;
        canvas.draw_rect(track_x, track_y, track_w, 3, Color::Fill);
        let range = (model.max - model.min).max(1);
        let progress = ((model.val - model.min) as u32 * (track_w as u32 - 2)) / range as u32;
        if progress > 0 {
            canvas.fill_rect(track_x + 1, track_y + 1, progress as u8, 1, Color::Fill);
        }

        // 5. Bottom action badges
        widgets::draw_key_badge(canvas, 2, 56, "M", model.confirm_label);
        widgets::draw_key_badge_right(canvas, 126, 56, "EXIT", model.cancel_label);
    }

    fn on_event(&mut self, model: &mut NumberPickerModel, event: &Event) -> bool {
        if let Event::Tick = event {
            if let Some(code) = self.repeater.tick() {
                match code {
                    KeyCode::Up => {
                        model.increment();
                        return true;
                    }
                    KeyCode::Down => {
                        model.decrement();
                        return true;
                    }
                    KeyCode::Key2 => {
                        model.increment_fast();
                        return true;
                    }
                    KeyCode::Key8 => {
                        model.decrement_fast();
                        return true;
                    }
                    _ => {}
                }
            }
            return false;
        }

        if let Event::Key { code, state } = *event {
            self.repeater.handle_key(code, state);
            if state == KeyState::Pressed {
                match code {
                    KeyCode::Up => {
                        model.increment();
                        return true;
                    }
                    KeyCode::Down => {
                        model.decrement();
                        return true;
                    }
                    KeyCode::Key2 => {
                        model.increment_fast();
                        return true;
                    }
                    KeyCode::Key8 => {
                        model.decrement_fast();
                        return true;
                    }
                    KeyCode::Menu => {
                        model.is_done = true;
                        return true;
                    }
                    KeyCode::Exit => {
                        model.is_canceled = true;
                        return true;
                    }
                    _ => {}
                }
            }
        }
        false
    }
}
