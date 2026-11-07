//! Declarative Toast / Notification Banner GUI Module
//! Non-blocking temporary overlay badge that auto-dismisses after a timeout.

use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::view::View;
use crate::text;
use crate::{Canvas, Color};
use heapless::String;
use kernel::event::Event;

pub struct ToastModel {
    pub message: String<32>,
    pub duration_ms: u16,
    pub elapsed_ms: u16,
    pub is_active: bool,
}

impl ToastModel {
    pub const fn new() -> Self {
        Self {
            message: String::new(),
            duration_ms: 1500,
            elapsed_ms: 0,
            is_active: false,
        }
    }

    pub fn show(&mut self, msg: &str, duration_ms: u16) {
        self.message.clear();
        let _ = self.message.push_str(msg);
        self.duration_ms = duration_ms;
        self.elapsed_ms = 0;
        self.is_active = true;
    }

    pub fn dismiss(&mut self) {
        self.is_active = false;
        self.elapsed_ms = 0;
    }

    pub fn tick(&mut self, dt_ms: u16) {
        if self.is_active {
            self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
            if self.elapsed_ms >= self.duration_ms {
                self.is_active = false;
            }
        }
    }
}

impl Default for ToastModel {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ToastView;

impl ToastView {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for ToastView {
    fn default() -> Self {
        Self::new()
    }
}

impl View<ToastModel> for ToastView {
    fn render(&self, canvas: &mut Canvas, model: &ToastModel) {
        if !model.is_active || model.message.is_empty() {
            return;
        }

        // Calculate width from message length (~4px per character in TomThumb)
        let text_w = (model.message.len() as i16 * 4).min(112);
        let badge_w = (text_w + 12).min(124) as u8;
        let badge_x = (128 - badge_w) / 2;
        let badge_y = 26u8; // Centered vertically
        let badge_h = 13u8;

        // Draw pill/capsule badge with clear backdrop and fill border
        canvas.fill_rect(badge_x, badge_y, badge_w, badge_h, Color::Fill);
        canvas.draw_rect(badge_x, badge_y, badge_w, badge_h, Color::Clear);

        // Center text in badge
        let text_x = badge_x as i16 + ((badge_w as i16 - text_w) / 2);
        text::draw_str(
            canvas,
            text_x,
            badge_y as i16 + 8,
            model.message.as_str(),
            &TOMTHUMB,
            Color::Clear,
        );
    }

    fn on_event(&mut self, model: &mut ToastModel, event: &Event) -> bool {
        if let Event::Tick = *event {
            model.tick(50);
            return model.is_active;
        }
        false
    }
}
