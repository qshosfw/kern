//! T9 Text Input GUI Module (DeltaFW AG TextInput style with Authentic Inverted Key Badges)

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::view::View;
use crate::text;
use crate::widgets;
use crate::{Canvas, Color};
use heapless::String;
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Charset {
    Upper = 0,
    Lower = 1,
    Numbers = 2,
    Symbols = 3,
}

#[repr(align(4))]
pub struct TextInputModel {
    pub is_done: bool,
    pub is_canceled: bool,
    pub show_grid: bool,
    pub exit_held: bool,
    pub exit_pressed: bool,
    pub is_passcode: bool,
    pub expose_len: bool,
    pub is_k1: bool,
    pub digits_only: bool,
    pub show_header: bool,
    pub charset: Charset,
    pub last_key: Option<u8>,
    pub max_len: usize,
    pub press_count: usize,
    pub last_activity_tick: u32,
    pub ticks: u32,
    pub exit_press_ticks: u32,
    pub hold_ticks: u32,
    pub custom_title: String<16>,
    pub buffer: String<32>,
}

impl TextInputModel {
    pub const fn new(max_len: usize) -> Self {
        Self {
            is_done: false,
            is_canceled: false,
            show_grid: false,
            exit_held: false,
            exit_pressed: false,
            is_passcode: false,
            expose_len: false,
            is_k1: true,
            digits_only: false,
            show_header: false,
            charset: Charset::Upper,
            last_key: None,
            max_len,
            press_count: 0,
            last_activity_tick: 0,
            ticks: 0,
            exit_press_ticks: 0,
            hold_ticks: 6,
            custom_title: String::new(),
            buffer: String::new(),
        }
    }

    #[inline(always)]
    pub fn title_str(&self) -> &str {
        self.custom_title.as_str()
    }
}

pub struct TextInputView;

impl TextInputView {
    pub const fn new() -> Self {
        Self
    }

    fn get_charset_chars(charset: Charset, key: u8) -> &'static str {
        const ALPHA_OFF: [u8; 11] = [0, 1, 7, 10, 13, 16, 19, 22, 26, 29, 33];
        if key > 9 {
            return "";
        }
        let k = key as usize;
        let (s, off) = match charset {
            Charset::Numbers => return &"0123456789"[k..k + 1],
            Charset::Symbols => (
                " .,?!'-@#$%&*()[]<>{}/\\|+-=\"'` :;_",
                &[0u8, 1, 7, 10, 13, 17, 21, 24, 27, 30, 33][..],
            ),
            Charset::Upper => (" .,?!'-ABCDEFGHIJKLMNOPQRSTUVWXYZ", &ALPHA_OFF[..]),
            Charset::Lower => (" .,?!'-abcdefghijklmnopqrstuvwxyz", &ALPHA_OFF[..]),
        };
        let start = off[k] as usize;
        let end = off[k + 1] as usize;
        &s[start..end]
    }

    fn confirm_current_char(model: &mut TextInputModel) {
        model.last_key = None;
        model.press_count = 0;
    }

    #[cfg(feature = "passcode")]
    fn draw_dot(canvas: &mut Canvas, xu: u8, yu: u8, filled: bool) {
        canvas.draw_horizontal_line(xu + 1, yu, 4, Color::Fill);
        canvas.draw_horizontal_line(xu + 1, yu + 5, 4, Color::Fill);
        if filled {
            canvas.fill_rect(xu, yu + 1, 6, 4, Color::Fill);
        } else {
            canvas.draw_vertical_line(xu, yu + 1, 4, Color::Fill);
            canvas.draw_vertical_line(xu + 5, yu + 1, 4, Color::Fill);
        }
    }

    fn draw_t9_grid(canvas: &mut Canvas, charset: Charset) {
        let mut x = 2i16;
        let mut y = 34i16;

        for key in 1..=9u8 {
            canvas.fill_rect(x as u8, y as u8, 7, 6, Color::Fill);
            text::draw_char(
                canvas,
                x + 2,
                y + 5,
                (b'0' + key) as char,
                &TOMTHUMB,
                Color::Clear,
            );

            let chars = Self::get_charset_chars(charset, key);
            text::draw_str(canvas, x + 9, y + 5, chars, &TOMTHUMB, Color::Fill);

            if key == 3 || key == 6 {
                x = 2;
                y += 7;
            } else {
                x += 42;
            }
        }
    }

    fn draw_bottom_badges(canvas: &mut Canvas, model: &TextInputModel, by: i16, show_grid: bool) {
        let menu_key = if model.is_k1 { "M" } else { "Menu" };
        let exit_label = if model.buffer.is_empty() {
            "Back"
        } else {
            "Del"
        };

        if model.digits_only {
            widgets::draw_key_badge(canvas, 1, by, menu_key, "Enter");
            widgets::draw_key_badge_right(canvas, 126, by, "EXIT", exit_label);
            return;
        }

        let star_label = match model.charset {
            Charset::Upper | Charset::Lower => "123",
            Charset::Numbers => "SYM",
            _ => "ABC",
        };

        let hash_key = if model.is_k1 { "F" } else { "#" };
        let case_label = match model.charset {
            Charset::Upper => "abc",
            Charset::Lower => "ABC",
            _ => "Case",
        };

        let (x0, x1, x2) = if show_grid { (2, 44, 86) } else { (1, 28, 60) };
        let enter_label = if show_grid || model.is_k1 {
            "Enter"
        } else {
            "OK"
        };
        widgets::draw_key_badge(canvas, x0, by, "*", star_label);
        widgets::draw_key_badge(canvas, x1, by, menu_key, enter_label);
        widgets::draw_key_badge(canvas, x2, by, hash_key, case_label);
        if !show_grid {
            widgets::draw_key_badge_right(canvas, 126, by, "EXIT", exit_label);
        }
    }

    #[cfg(feature = "passcode")]
    fn draw_passcode_dots(canvas: &mut Canvas, model: &TextInputModel, cy: i16, is_cycling: bool) {
        let entered_len = model.buffer.len();
        let total_slots = if model.expose_len {
            model.max_len.max(1)
        } else {
            entered_len
        };

        if total_slots == 0 {
            if (model.ticks & 8) != 0 {
                canvas.draw_vertical_line(64, cy as u8, 8, Color::Fill);
            }
        } else {
            let spacing = if total_slots > 8 { 9i16 } else { 12i16 };
            let total_w = if total_slots > 1 {
                (total_slots as i16 - 1) * spacing + 6
            } else {
                6
            };
            let start_x = (128i16 - total_w) / 2;
            let yu = cy as u8;

            for i in 0..total_slots {
                let cx = start_x + (i as i16) * spacing;
                if i + 1 == entered_len && is_cycling {
                    let last_char = &model.buffer[entered_len - 1..];
                    let w = text::str_width(last_char, &MUMATRIX);
                    text::draw_str(
                        canvas,
                        cx + 3 - w / 2,
                        cy + 6,
                        last_char,
                        &MUMATRIX,
                        Color::Fill,
                    );
                } else {
                    Self::draw_dot(canvas, cx as u8, yu, i < entered_len);
                }
            }
        }
    }
}

impl Default for TextInputView {
    fn default() -> Self {
        Self::new()
    }
}

impl View<TextInputModel> for TextInputView {
    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, model: &TextInputModel) {
        let title_s = model.title_str();
        if model.show_header && !title_s.is_empty() {
            crate::widgets::draw_negated_header(canvas, 0, title_s);
            canvas.fill_rect(0, 9, 128, 55, Color::Clear);
        } else {
            canvas.fill_rect(0, 8, 128, 56, Color::Clear);
        }

        // 2. Mode & Length Indicator Bar (DeltaFW layout below header)
        let header_y = 15i16;
        if !model.digits_only {
            let charset_name = match model.charset {
                Charset::Upper => "ABC",
                Charset::Lower => "abc",
                Charset::Numbers => "123",
                Charset::Symbols => "#@$",
            };
            text::draw_str(canvas, 3, header_y, charset_name, &TOMTHUMB, Color::Fill);
        }

        #[cfg(feature = "passcode")]
        let is_passcode = model.is_passcode;
        #[cfg(not(feature = "passcode"))]
        let is_passcode = false;

        if !is_passcode {
            let mut num_buf = [0u8; 12];
            let count_s = text::format_fraction_u32(
                model.buffer.len() as u32,
                model.max_len as u32,
                &mut num_buf,
            );
            text::draw_str_align(
                canvas,
                125,
                header_y,
                count_s,
                &TOMTHUMB,
                Color::Fill,
                text::Align::Right,
            );
        }

        let show_grid = if model.digits_only {
            false
        } else {
            model.show_grid
                || match model.charset {
                    Charset::Numbers => false,
                    Charset::Symbols => true,
                    Charset::Upper | Charset::Lower => !model.is_k1,
                }
        };

        let is_cycling =
            model.last_key.is_some() && (model.ticks.wrapping_sub(model.last_activity_tick) < 15);
        let text_y = if show_grid { 27i16 } else { 33i16 };

        #[cfg(feature = "passcode")]
        if is_passcode {
            let cy = if show_grid { 22i16 } else { 29i16 };
            Self::draw_passcode_dots(canvas, model, cy, is_cycling);
        }
        if !is_passcode {
            let text_x = 5i16;
            let str_w = text::str_width(&model.buffer, &MUMATRIX);
            let max_w = 118i16;
            let scroll_x = if str_w > max_w { str_w - max_w } else { 0 };

            text::draw_str(
                canvas,
                text_x - scroll_x,
                text_y,
                &model.buffer,
                &MUMATRIX,
                Color::Fill,
            );

            let cursor_x = (text_x - scroll_x) + str_w;
            if is_cycling && !model.buffer.is_empty() {
                let last_char_str = &model.buffer[model.buffer.len() - 1..];
                let last_w = text::str_width(last_char_str, &MUMATRIX);
                let char_x = cursor_x - last_w;
                canvas.fill_rect(
                    (char_x - 1) as u8,
                    (text_y - 7) as u8,
                    (last_w + 3) as u8,
                    10,
                    Color::Fill,
                );
                text::draw_str(
                    canvas,
                    char_x,
                    text_y,
                    last_char_str,
                    &MUMATRIX,
                    Color::Clear,
                );
            } else if (model.ticks & 8) != 0 {
                canvas.draw_vertical_line(cursor_x as u8, (text_y - 7) as u8, 10, Color::Fill);
            }
        }

        if show_grid {
            Self::draw_t9_grid(canvas, model.charset);
            Self::draw_bottom_badges(canvas, model, 55, true);
        } else {
            if !is_passcode {
                canvas.draw_horizontal_line(0, 52, 128, Color::Fill);
            }
            let by = if is_passcode { 55 } else { 54 };
            Self::draw_bottom_badges(canvas, model, by, false);
        }
    }

    #[inline(never)]
    fn on_event(&mut self, model: &mut TextInputModel, event: &Event) -> bool {
        if let Event::Tick = event {
            model.ticks = model.ticks.wrapping_add(1);
            if model.exit_pressed && !model.exit_held {
                model.exit_press_ticks = model.exit_press_ticks.saturating_add(1);
                let threshold = if model.hold_ticks > 0 {
                    model.hold_ticks
                } else {
                    6
                };
                if model.exit_press_ticks >= threshold {
                    cancel_input(model);
                }
            }
            return true;
        }

        if let Event::Key { code, state } = event {
            let is_pressed = *state == KeyState::Pressed;
            let is_held = *state == KeyState::Held;
            let is_released = *state == KeyState::Released;

            fn push_char_with_limit(model: &mut TextInputModel, ch: char) {
                if model.buffer.len() < model.max_len {
                    let _ = model.buffer.push(ch);
                }
                #[cfg(feature = "passcode")]
                if model.is_passcode && model.expose_len && model.buffer.len() >= model.max_len {
                    model.last_key = None;
                    model.press_count = 0;
                    model.is_done = true;
                }
            }

            // 1. Explicit KeyState::Held fallback (for drivers/simulators sending Held)
            if is_held {
                match code {
                    KeyCode::Exit => {
                        cancel_input(model);
                        return true;
                    }
                    KeyCode::Menu => {
                        Self::confirm_current_char(model);
                        model.is_done = true;
                        return true;
                    }
                    _ => {
                        if let Some(digit) = code.to_digit() {
                            Self::confirm_current_char(model);
                            push_char_with_limit(model, (b'0' + digit) as char);
                            return true;
                        }
                    }
                }
            }

            // 2. Key Release for Exit (Backspace ONLY on short tap release, or Back if empty)
            if is_released && *code == KeyCode::Exit {
                if model.exit_pressed {
                    model.exit_pressed = false;
                    if !model.exit_held {
                        if model.last_key.is_some() {
                            Self::confirm_current_char(model);
                        } else if !model.buffer.is_empty() {
                            model.buffer.pop();
                        } else {
                            // Buffer is empty: short tap on Exit cancels immediately ("Back")!
                            cancel_input(model);
                        }
                    }
                }
                model.exit_held = false;
                model.exit_press_ticks = 0;
                return true;
            }

            // 3. Short Press Down (KeyState::Pressed)
            if is_pressed {
                if let Some(key_num) = code.to_digit() {
                    if model.digits_only || model.charset == Charset::Numbers {
                        Self::confirm_current_char(model);
                        push_char_with_limit(model, (b'0' + key_num) as char);
                        model.last_activity_tick = model.ticks;
                        return true;
                    }

                    let chars = Self::get_charset_chars(model.charset, key_num).as_bytes();
                    if chars.is_empty() {
                        return true;
                    }

                    let is_same_key = model.last_key == Some(key_num);
                    let is_recent = (model.ticks.wrapping_sub(model.last_activity_tick)) < 15;

                    if is_same_key && is_recent && !model.buffer.is_empty() {
                        // Multi-tap cycle through characters
                        model.press_count += 1;
                        if model.press_count >= chars.len() {
                            model.press_count = 0;
                        }
                        let char_idx = model.press_count;
                        let new_ch = chars[char_idx] as char;
                        model.buffer.pop();
                        let _ = model.buffer.push(new_ch);
                    } else {
                        // First tap on a new key
                        Self::confirm_current_char(model);
                        push_char_with_limit(model, chars[0] as char);
                        model.last_key = Some(key_num);
                        model.press_count = 0;
                    }
                    model.last_activity_tick = model.ticks;
                    return true;
                }

                match code {
                    KeyCode::Exit => {
                        model.exit_pressed = true;
                        model.exit_press_ticks = 0;
                        model.exit_held = false;
                        return true;
                    }

                    KeyCode::Star => {
                        if model.digits_only {
                            return true;
                        }
                        // Cycle Charset Category (*): Letters (Upper/Lower) -> Numbers -> Symbols -> Letters
                        Self::confirm_current_char(model);
                        model.charset = match model.charset {
                            Charset::Upper | Charset::Lower => Charset::Numbers,
                            Charset::Numbers => Charset::Symbols,
                            Charset::Symbols => Charset::Upper,
                        };
                        return true;
                    }

                    KeyCode::KeyF | KeyCode::Hash | KeyCode::Side1 => {
                        if model.digits_only {
                            return true;
                        }
                        // Quick Case Toggle (# / F / Side1): Upper <-> Lower
                        // If in Numbers or Symbols, toggle switches to Upper letters
                        Self::confirm_current_char(model);
                        model.charset = match model.charset {
                            Charset::Upper => Charset::Lower,
                            Charset::Lower => Charset::Upper,
                            Charset::Numbers | Charset::Symbols => Charset::Upper,
                        };
                        return true;
                    }

                    KeyCode::Menu => {
                        // Single tap MENU submits/finishes
                        Self::confirm_current_char(model);
                        model.is_done = true;
                        return true;
                    }

                    _ => {}
                }
            }

            // Always sink key events in TextInput mode
            return true;
        }

        false
    }
}

fn cancel_input(model: &mut TextInputModel) {
    model.exit_held = true;
    model.exit_pressed = false;
    model.exit_press_ticks = 0;
    TextInputView::confirm_current_char(model);
    model.buffer.clear();
    model.is_canceled = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_input_empty_exit_cancels_immediately() {
        let mut model = TextInputModel::new(16);
        let mut view = TextInputView;

        // Press Exit
        let ev_press = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        assert!(view.on_event(&mut model, &ev_press));
        assert!(model.exit_pressed);
        assert!(!model.is_canceled);

        // Release Exit when buffer is empty -> cancels immediately
        let ev_release = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Released,
        };
        assert!(view.on_event(&mut model, &ev_release));
        assert!(!model.exit_pressed);
        assert!(model.is_canceled);
    }

    #[test]
    fn test_text_input_nonempty_exit_pops_character() {
        let mut model = TextInputModel::new(16);
        let mut view = TextInputView;
        let _ = model.buffer.push('A');

        // Press Exit
        let ev_press = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        assert!(view.on_event(&mut model, &ev_press));

        // Release Exit when buffer has 'A' -> pops 'A', does NOT cancel
        let ev_release = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Released,
        };
        assert!(view.on_event(&mut model, &ev_release));
        assert!(!model.is_canceled);
        assert!(model.buffer.is_empty());
    }
}
