//! Declarative Progress Modal GUI Module
//! Provides a clean status/progress bar modal dialog for long-running operations.

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::view::View;
use crate::text;
use crate::widgets;
use crate::{Canvas, Color};
use heapless::String;
use kernel::event::{Event, KeyCode, KeyState};

pub struct ProgressModalModel {
    pub title: &'static str,
    pub subtitle: String<48>,
    pub percent: u8,
    pub cancelable: bool,
    pub is_canceled: bool,
}

impl ProgressModalModel {
    pub const fn new(title: &'static str) -> Self {
        Self {
            title,
            subtitle: String::new(),
            percent: 0,
            cancelable: false,
            is_canceled: false,
        }
    }

    pub fn with_cancelable(mut self, cancelable: bool) -> Self {
        self.cancelable = cancelable;
        self
    }

    pub fn set_progress(&mut self, percent: u8, subtitle: &str) {
        self.percent = percent.min(100);
        self.subtitle.clear();
        let _ = self.subtitle.push_str(subtitle);
    }
}

pub struct ProgressModalView;

impl ProgressModalView {
    pub const fn new() -> Self {
        Self
    }
}

impl View<ProgressModalModel> for ProgressModalView {
    fn render(&self, canvas: &mut Canvas, model: &ProgressModalModel) {
        let sheet_y = 18i16;
        let sheet_h = 46u8;

        // 1. Draw outer frame, rounded border & clear backdrop with inverted header
        crate::widgets::draw_sheet_frame(canvas, model.title, sheet_y, sheet_h, None);

        // 3. Subtitle / Status message
        if !model.subtitle.is_empty() {
            text::draw_str(
                canvas,
                6,
                sheet_y + 16,
                model.subtitle.as_str(),
                &TOMTHUMB,
                Color::Fill,
            );
        }

        // 4. Progress bar (Y=38..46, width 96, X=8)
        let bar_x = 8u8;
        let bar_y = (sheet_y + 20) as u8;
        let bar_w = 88u8;
        let bar_h = 8u8;

        canvas.draw_rect(bar_x, bar_y, bar_w, bar_h, Color::Fill);
        let fill_w = ((model.percent as u32 * (bar_w as u32 - 4)) / 100) as u8;
        if fill_w > 0 {
            canvas.fill_rect(bar_x + 2, bar_y + 2, fill_w, bar_h - 4, Color::Fill);
        }

        // Percentage text display (X=100)
        let mut pct_buf = [0u8; 12];
        let pct_str = text::format_suffix_u32(model.percent as u32, b'%', &mut pct_buf);
        text::draw_str(canvas, 102, sheet_y + 26, pct_str, &MUMATRIX, Color::Fill);

        // 5. Cancel badge if cancelable
        if model.cancelable {
            widgets::draw_key_badge_right(canvas, 126, 56, "EXIT", "Cancel");
        }
    }

    fn on_event(&mut self, model: &mut ProgressModalModel, event: &Event) -> bool {
        if model.cancelable {
            if let Event::Key {
                code: KeyCode::Exit,
                state: KeyState::Pressed,
            } = *event
            {
                model.is_canceled = true;
                return true;
            }
        }
        false
    }
}
