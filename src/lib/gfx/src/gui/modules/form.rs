//! Declarative Form / Property Sheet GUI Module
//! High-level declarative form layout with inline editing, toggle switches, numeric bounds, and choice pickers.

use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::view::View;
use crate::text;
use crate::widgets;
use crate::{Canvas, Color};
use heapless::String;
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone)]
pub enum FormItemKind<'a> {
    Switch(bool),
    Number {
        val: i32,
        min: i32,
        max: i32,
        step: i32,
        unit: &'static str,
    },
    Choice {
        selected: usize,
        options: &'a [&'a str],
    },
    Action(&'static str),
}

#[derive(Debug, Clone)]
pub struct FormItem<'a> {
    pub id: u8,
    pub label: &'static str,
    pub kind: FormItemKind<'a>,
}

pub struct FormModel<'a, const N: usize> {
    pub title: &'static str,
    pub items: [FormItem<'a>; N],
    pub selected_idx: usize,
    pub top_idx: usize,
    pub is_done: bool,
    pub is_canceled: bool,
    pub changed: bool,
}

impl<'a, const N: usize> FormModel<'a, N> {
    pub const fn new(title: &'static str, items: [FormItem<'a>; N]) -> Self {
        Self {
            title,
            items,
            selected_idx: 0,
            top_idx: 0,
            is_done: false,
            is_canceled: false,
            changed: false,
        }
    }

    pub fn selected_item(&self) -> &FormItem<'a> {
        &self.items[self.selected_idx]
    }

    pub fn selected_item_mut(&mut self) -> &mut FormItem<'a> {
        &mut self.items[self.selected_idx]
    }
}

pub struct FormView {
    pub repeater: crate::gui::repeater::KeyRepeater,
}

impl FormView {
    pub const fn new() -> Self {
        Self {
            repeater: crate::gui::repeater::KeyRepeater::new(8, 2),
        }
    }
}

impl Default for FormView {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, const N: usize> View<FormModel<'a, N>> for FormView {
    fn render(&self, canvas: &mut Canvas, model: &FormModel<'a, N>) {
        let sheet_y = 12i16;
        let sheet_h = 52u8;

        // 1. Draw outer frame, rounded border & clear region with inverted header
        crate::widgets::draw_sheet_frame(canvas, model.title, sheet_y, sheet_h, None);

        // 3. Render 4 visible rows (Y=23, 31, 39, 47)
        let visible_count = 4usize;
        let row_h = 8i16;
        let start_y = sheet_y + 11;

        for i in 0..visible_count {
            let item_idx = model.top_idx + i;
            if item_idx >= N {
                break;
            }

            let item = &model.items[item_idx];
            let row_y = start_y + (i as i16 * row_h);
            let is_sel = item_idx == model.selected_idx;

            // Highlight selected row
            if is_sel {
                canvas.fill_rect(2, (row_y - 1) as u8, 120, 8, Color::Fill);
            }

            let text_color = if is_sel { Color::Clear } else { Color::Fill };

            // Draw label (X=4)
            text::draw_str(canvas, 4, row_y + 5, item.label, &TOMTHUMB, text_color);

            // Format value text on the right side
            let mut val_buf: String<24> = String::new();
            match &item.kind {
                FormItemKind::Switch(on) => {
                    let _ = val_buf.push_str(if *on { "[ON]" } else { "[OFF]" });
                }
                FormItemKind::Number { val, unit, .. } => {
                    let mut nbuf = [0u8; 12];
                    let _ = val_buf.push_str(text::format_i32(*val, &mut nbuf));
                    if !unit.is_empty() {
                        let _ = val_buf.push_str(unit);
                    }
                }
                FormItemKind::Choice { selected, options } => {
                    if *selected < options.len() {
                        let _ = val_buf.push('<');
                        let _ = val_buf.push_str(options[*selected]);
                        let _ = val_buf.push('>');
                    }
                }
                FormItemKind::Action(act) => {
                    let _ = val_buf.push('[');
                    let _ = val_buf.push_str(act);
                    let _ = val_buf.push(']');
                }
            }

            // Right-aligned value text (X=120 - width)
            let val_w = val_buf.len() as i16 * 4;
            let val_x = (120 - val_w).max(56);
            text::draw_str(
                canvas,
                val_x,
                row_y + 5,
                val_buf.as_str(),
                &TOMTHUMB,
                text_color,
            );
        }

        // 4. Scrollbar on the right (X=124..126)
        if N > visible_count {
            let track_y = (sheet_y + 10) as u8;
            widgets::draw_scrollbar(canvas, 124, track_y, 34, model.top_idx, N, visible_count);
        }

        // 5. Bottom action badges
        widgets::draw_key_badge(canvas, 2, 56, "M", "Select");
        widgets::draw_key_badge_right(canvas, 126, 56, "EXIT", "Back");
    }

    fn on_event(&mut self, model: &mut FormModel<'a, N>, event: &Event) -> bool {
        let visible_count = 4usize;

        if let Event::Tick = event {
            if let Some(code) = self.repeater.tick() {
                match code {
                    KeyCode::Up | KeyCode::Key2 => {
                        if model.selected_idx > 0 {
                            model.selected_idx -= 1;
                        } else {
                            model.selected_idx = N.saturating_sub(1);
                        }
                        if model.selected_idx < model.top_idx {
                            model.top_idx = model.selected_idx;
                        } else if model.selected_idx >= model.top_idx + visible_count {
                            model.top_idx = model.selected_idx + 1 - visible_count;
                        }
                        return true;
                    }
                    KeyCode::Down | KeyCode::Key8 => {
                        if N > 0 {
                            model.selected_idx = (model.selected_idx + 1) % N;
                        }
                        if model.selected_idx < model.top_idx {
                            model.top_idx = model.selected_idx;
                        } else if model.selected_idx >= model.top_idx + visible_count {
                            model.top_idx = model.selected_idx + 1 - visible_count;
                        }
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
                    KeyCode::Up | KeyCode::Key2 => {
                        if model.selected_idx > 0 {
                            model.selected_idx -= 1;
                        } else {
                            model.selected_idx = N.saturating_sub(1);
                        }
                        // Adjust scroll top
                        if model.selected_idx < model.top_idx {
                            model.top_idx = model.selected_idx;
                        } else if model.selected_idx >= model.top_idx + visible_count {
                            model.top_idx = model.selected_idx + 1 - visible_count;
                        }
                        return true;
                    }
                    KeyCode::Down | KeyCode::Key8 => {
                        if N > 0 {
                            model.selected_idx = (model.selected_idx + 1) % N;
                        }
                        // Adjust scroll top
                        if model.selected_idx < model.top_idx {
                            model.top_idx = model.selected_idx;
                        } else if model.selected_idx >= model.top_idx + visible_count {
                            model.top_idx = model.selected_idx + 1 - visible_count;
                        }
                        return true;
                    }
                    KeyCode::Star | KeyCode::Key4 => {
                        // Left / Decrement
                        let item = &mut model.items[model.selected_idx];
                        match &mut item.kind {
                            FormItemKind::Switch(on) => {
                                *on = !*on;
                                model.changed = true;
                            }
                            FormItemKind::Number { val, min, step, .. } => {
                                *val = (*val - *step).max(*min);
                                model.changed = true;
                            }
                            FormItemKind::Choice { selected, options } => {
                                if *selected > 0 {
                                    *selected -= 1;
                                } else {
                                    *selected = options.len().saturating_sub(1);
                                }
                                model.changed = true;
                            }
                            FormItemKind::Action(_) => {}
                        }
                        return true;
                    }
                    KeyCode::Hash | KeyCode::Key6 => {
                        // Right / Increment
                        let item = &mut model.items[model.selected_idx];
                        match &mut item.kind {
                            FormItemKind::Switch(on) => {
                                *on = !*on;
                                model.changed = true;
                            }
                            FormItemKind::Number { val, max, step, .. } => {
                                *val = (*val + *step).min(*max);
                                model.changed = true;
                            }
                            FormItemKind::Choice { selected, options } => {
                                if !options.is_empty() {
                                    *selected = (*selected + 1) % options.len();
                                }
                                model.changed = true;
                            }
                            FormItemKind::Action(_) => {}
                        }
                        return true;
                    }
                    _ => {}
                }
            }

            if state == KeyState::Pressed {
                match code {
                    KeyCode::Menu => {
                        // Toggle or activate
                        let item = &mut model.items[model.selected_idx];
                        match &mut item.kind {
                            FormItemKind::Switch(on) => {
                                *on = !*on;
                                model.changed = true;
                            }
                            FormItemKind::Choice { selected, options } => {
                                if !options.is_empty() {
                                    *selected = (*selected + 1) % options.len();
                                }
                                model.changed = true;
                            }
                            FormItemKind::Action(_) => {
                                model.is_done = true;
                            }
                            FormItemKind::Number { .. } => {
                                model.is_done = true;
                            }
                        }
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
