//! T9 Text Input GUI Module (DeltaFW AG TextInput style with Authentic Inverted Key Badges)

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::view::View;
use crate::text;
use crate::widgets;
use crate::{Canvas, Color};
use heapless::String;
use kernel::event::{Event, KeyCode, KeyState};

/// Standard filesystem dictionary pack format identifier (`DICT`, 16 B header + 32 B records).
/// Dictionaries live in KernFS (e.g. `/user/dict_en.dct`) so predictive T9 suggestions cost
/// zero Flash overhead when no pack is installed.
pub const DICT_FILE_ID: [u8; 4] = *b"DICT";
pub const DICT_HEADER_SIZE: usize = 16;
pub const DICT_RECORD_SIZE: usize = 32;

/// Declarative keypad charset descriptor supporting custom encodings and symbol sets.
#[derive(Debug, Clone, Copy)]
pub struct CharsetSpec {
    pub label: &'static str,
    pub star_hint: &'static str,
    pub case_hint: &'static str,
    pub show_grid_on_k1: bool,
    pub keys: [&'static str; 10],
}

/// Optional predictive suggestion source (implemented by filesystem dictionary packs).
pub trait TextSuggestionProvider {
    fn suggest(&self, prefix: &str, out: &mut [String<12>; 3]) -> usize;
}

pub static BUILTIN_CHARSETS: [CharsetSpec; 4] = [
    CharsetSpec {
        label: "ABC",
        star_hint: "123",
        case_hint: "abc",
        show_grid_on_k1: false,
        keys: [
            " ", ".,?!'-", "ABC", "DEF", "GHI", "JKL", "MNO", "PQRS", "TUV", "WXYZ",
        ],
    },
    CharsetSpec {
        label: "abc",
        star_hint: "123",
        case_hint: "ABC",
        show_grid_on_k1: false,
        keys: [
            " ", ".,?!'-", "abc", "def", "ghi", "jkl", "mno", "pqrs", "tuv", "wxyz",
        ],
    },
    CharsetSpec {
        label: "123",
        star_hint: "SYM",
        case_hint: "Case",
        show_grid_on_k1: false,
        keys: ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"],
    },
    CharsetSpec {
        label: "#@$",
        star_hint: "ABC",
        case_hint: "Case",
        show_grid_on_k1: true,
        keys: [
            " ", ".,?!'-", "@#$", "%&*", "()[]", "<>{}/", "\\|+", "-=\"", "'` :", ";_",
        ],
    },
];

/// Optional localized 0..=9 keypad layout loaded dynamically from the active `.lng` pack
/// (e.g. Russian Cyrillic, Japanese Keitai Kana, or Spanish accented characters).
/// Automatically activates the on-screen 3x3 keypad guide since it differs from the factory Latin silkscreen.
#[derive(Debug, Clone)]
pub struct LocaleKeymap {
    pub active: bool,
    label_len: u8,
    label_buf: [u8; 8],
    offsets: [u8; 11],
    buf: [u8; 72],
}

impl LocaleKeymap {
    pub const fn empty() -> Self {
        Self {
            active: false,
            label_len: 0,
            label_buf: [0; 8],
            offsets: [0; 11],
            buf: [0; 72],
        }
    }

    pub fn set_label(&mut self, s: &str) {
        let b = s.as_bytes();
        let mut n = b.len().min(self.label_buf.len());
        while n > 0 && n < b.len() && (b[n] & 0xC0) == 0x80 {
            n -= 1;
        }
        self.label_buf[..n].copy_from_slice(&b[..n]);
        self.label_len = n as u8;
    }

    #[inline(always)]
    pub fn label_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.label_buf[..self.label_len as usize]) }
    }

    /// Populate key `key` (`0..10`) in ascending order inside the packed UTF-8 buffer.
    pub fn set_key(&mut self, key: usize, s: &str) {
        if key >= 10 {
            return;
        }
        let used = self.offsets[key] as usize;
        let b = s.as_bytes();
        let avail = self.buf.len().saturating_sub(used);
        let mut n = b.len().min(avail);
        while n > 0 && n < b.len() && (b[n] & 0xC0) == 0x80 {
            n -= 1;
        }
        self.buf[used..used + n].copy_from_slice(&b[..n]);
        let end = (used + n) as u8;
        for o in &mut self.offsets[key + 1..] {
            *o = end;
        }
    }

    #[inline(always)]
    pub fn key_str(&self, key: usize) -> &str {
        if key >= 10 {
            return "";
        }
        let s = self.offsets[key] as usize;
        let e = self.offsets[key + 1] as usize;
        if e <= s || e > self.buf.len() {
            return "";
        }
        unsafe { core::str::from_utf8_unchecked(&self.buf[s..e]) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Charset {
    Upper = 0,
    Lower = 1,
    Numbers = 2,
    Symbols = 3,
    Locale = 4,
}

impl Charset {
    #[inline(always)]
    pub fn spec(self) -> &'static CharsetSpec {
        let idx = (self as usize).min(BUILTIN_CHARSETS.len() - 1);
        &BUILTIN_CHARSETS[idx]
    }
}

#[repr(C)]
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
    pub locale_keymap: LocaleKeymap,
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
            locale_keymap: LocaleKeymap::empty(),
        }
    }

    #[inline(always)]
    pub fn title_str(&self) -> &str {
        self.custom_title.as_str()
    }

    #[inline(always)]
    pub fn charset_label(&self) -> &str {
        let lbl = self.locale_keymap.label_str();
        if self.charset == Charset::Locale && !lbl.is_empty() {
            lbl
        } else {
            self.charset.spec().label
        }
    }

    /// True whenever the active input layout differs from the factory Latin keypad silkscreen.
    #[inline(always)]
    pub fn needs_keypad_guide(&self) -> bool {
        !self.digits_only
            && (self.show_grid
                || self.charset == Charset::Locale
                || self.charset.spec().show_grid_on_k1
                || (!self.is_k1 && self.charset != Charset::Numbers))
    }

    #[inline(always)]
    pub fn key_chars(&self, key: u8) -> &str {
        if key > 9 {
            return "";
        }
        if self.charset == Charset::Locale && self.locale_keymap.active {
            self.locale_keymap.key_str(key as usize)
        } else {
            self.charset.spec().keys[key as usize]
        }
    }
}

pub struct TextInputView;

/// Count UTF-8 codepoints in `s` without pulling in `core::str::Chars`.
#[inline(never)]
fn utf8_char_count(s: &str) -> usize {
    let mut n = 0usize;
    for &b in s.as_bytes() {
        if (b & 0xC0) != 0x80 {
            n += 1;
        }
    }
    n
}

/// Return byte range `(start, end)` of the `nth` UTF-8 codepoint in `s`.
#[inline(never)]
fn utf8_nth_slice(s: &str, nth: usize) -> Option<&str> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut c = 0usize;
    while i < b.len() {
        let mut next = i + 1;
        while next < b.len() && (b[next] & 0xC0) == 0x80 {
            next += 1;
        }
        if c == nth {
            return Some(unsafe { core::str::from_utf8_unchecked(&b[i..next]) });
        }
        c += 1;
        i = next;
    }
    None
}

/// Return the last UTF-8 codepoint slice in `s`, if non-empty.
#[inline(never)]
fn utf8_last_slice(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut i = b.len() - 1;
    while i > 0 && (b[i] & 0xC0) == 0x80 {
        i -= 1;
    }
    Some(unsafe { core::str::from_utf8_unchecked(&b[i..]) })
}

/// Pop the last UTF-8 codepoint from a `heapless::String` without `char` decoding overhead.
#[inline(never)]
fn pop_utf8_char<const N: usize>(s: &mut String<N>) {
    let b = s.as_bytes();
    if b.is_empty() {
        return;
    }
    let mut i = b.len() - 1;
    while i > 0 && (b[i] & 0xC0) == 0x80 {
        i -= 1;
    }
    unsafe { s.as_mut_vec().truncate(i) };
}

impl TextInputView {
    pub const fn new() -> Self {
        Self
    }

    #[inline(never)]
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

    #[inline(never)]
    fn draw_t9_grid(canvas: &mut Canvas, model: &TextInputModel, is_cycling: bool) {
        let mut x = 2i16;
        let mut y = 31i16;

        for key in 1..10u8 {
            let active_key = is_cycling && model.last_key == Some(key);
            let (bg_y, bg_w, bg_h, txt_col, max_x) = if active_key {
                ((y - 1) as u8, 40u8, 8u8, Color::Clear, x + 39)
            } else {
                (y as u8, 7u8, 6u8, Color::Fill, x + 40)
            };
            canvas.fill_rect(x as u8, bg_y, bg_w, bg_h, Color::Fill);
            text::draw_char(
                canvas,
                x + 2,
                y + 5,
                (b'0' + key) as char,
                &TOMTHUMB,
                Color::Clear,
            );
            let chars = model.key_chars(key);
            text::draw_str_max_x(canvas, x + 9, y + 5, chars, &TOMTHUMB, txt_col, max_x);

            if key == 3 || key == 6 {
                x = 2;
                y += 8;
            } else {
                x += 42;
            }
        }
    }

    /// Render a live tap-cycle preview strip in the mode bar (e.g. `2: A [B] C`) while multi-tapping.
    #[inline(never)]
    fn draw_cycle_strip(canvas: &mut Canvas, model: &TextInputModel, y: i16) {
        let Some(key) = model.last_key else {
            return;
        };
        let chars = model.key_chars(key);
        let b = chars.as_bytes();
        let mut cx = 28i16;
        let max_x = 104i16;
        let mut i = 0usize;
        let mut idx = 0usize;
        while i < b.len() {
            let mut next = i + 1;
            while next < b.len() && (b[next] & 0xC0) == 0x80 {
                next += 1;
            }
            let s = unsafe { core::str::from_utf8_unchecked(&b[i..next]) };
            let w = text::str_width(s, &TOMTHUMB).max(4);
            if cx + w + 2 > max_x {
                break;
            }
            let col = if idx == model.press_count {
                canvas.fill_rect((cx - 1) as u8, (y - 6) as u8, (w + 2) as u8, 8, Color::Fill);
                Color::Clear
            } else {
                Color::Fill
            };
            text::draw_str(canvas, cx, y, s, &TOMTHUMB, col);
            cx += w + 2;
            idx += 1;
            i = next;
        }
    }

    #[inline(never)]
    fn draw_bottom_badges(canvas: &mut Canvas, model: &TextInputModel, by: i16, show_grid: bool) {
        let menu_key = if model.is_k1 { "M" } else { "Menu" };
        let exit_label = if model.buffer.is_empty() {
            "Back"
        } else {
            "Del"
        };

        let mut draw_exit = true;
        if model.digits_only {
            widgets::draw_key_badge(canvas, 1, by, menu_key, "Enter");
        } else {
            let spec = model.charset.spec();
            let hash_key = if model.is_k1 { "F" } else { "#" };
            let (x0, x1, x2) = if show_grid { (2, 44, 86) } else { (1, 28, 60) };
            let enter_label = if show_grid || model.is_k1 {
                "Enter"
            } else {
                "OK"
            };
            widgets::draw_key_badge(canvas, x0, by, "*", spec.star_hint);
            widgets::draw_key_badge(canvas, x1, by, menu_key, enter_label);
            widgets::draw_key_badge(canvas, x2, by, hash_key, spec.case_hint);
            draw_exit = !show_grid;
        }
        if draw_exit {
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
        let clear_y: u8 = if model.show_header && !title_s.is_empty() {
            crate::widgets::draw_negated_header(canvas, 0, title_s);
            9
        } else {
            8
        };
        canvas.fill_rect(0, clear_y, 128, 64 - clear_y, Color::Clear);

        // 2. Mode & Length Indicator Bar (DeltaFW layout below header)
        let header_y = 16i16;
        let is_cycling =
            model.last_key.is_some() && (model.ticks.wrapping_sub(model.last_activity_tick) < 15);
        if !model.digits_only {
            text::draw_str(
                canvas,
                3,
                header_y,
                model.charset_label(),
                &TOMTHUMB,
                Color::Fill,
            );
            if is_cycling {
                Self::draw_cycle_strip(canvas, model, header_y);
            }
        }

        #[cfg(feature = "passcode")]
        let is_passcode = model.is_passcode;
        #[cfg(not(feature = "passcode"))]
        let is_passcode = false;

        if !is_passcode {
            let mut num_buf = core::mem::MaybeUninit::<[u8; 12]>::uninit();
            let count_s = text::format_fraction_u32(
                utf8_char_count(&model.buffer) as u32,
                model.max_len as u32,
                unsafe { &mut *num_buf.as_mut_ptr() },
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

        let show_grid = model.needs_keypad_guide();
        let text_y = if show_grid { 26i16 } else { 33i16 };

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

            text::draw_str_clipped(
                canvas,
                text_x,
                text_x + max_w,
                scroll_x,
                text_y,
                &model.buffer,
                &MUMATRIX,
                Color::Fill,
            );

            let cursor_x = (text_x - scroll_x) + str_w;
            if is_cycling && let Some(last_char_str) = utf8_last_slice(&model.buffer) {
                let last_w = text::str_width(last_char_str, &MUMATRIX).max(4);
                let char_x = (cursor_x - last_w).max(text_x);
                canvas.fill_rect(
                    char_x as u8,
                    (text_y - 7) as u8,
                    (last_w + 1) as u8,
                    9,
                    Color::Fill,
                );
                text::draw_str(
                    canvas,
                    char_x + 1,
                    text_y,
                    last_char_str,
                    &MUMATRIX,
                    Color::Clear,
                );
            } else if (model.ticks & 8) != 0 {
                canvas.draw_vertical_line(cursor_x as u8, (text_y - 7) as u8, 10, Color::Fill);
            }
        }

        let by = if show_grid {
            Self::draw_t9_grid(canvas, model, is_cycling);
            56
        } else {
            if !is_passcode {
                canvas.draw_horizontal_line(0, 52, 128, Color::Fill);
            }
            54 + is_passcode as i16
        };
        Self::draw_bottom_badges(canvas, model, by, show_grid);
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

        #[cfg(not(target_arch = "arm"))]
        if let Event::Char(ch) = *event {
            match ch {
                '\x08' | '\x7f' => {
                    Self::confirm_current_char(model);
                    pop_utf8_char(&mut model.buffer);
                }
                '\r' | '\n' => {
                    Self::confirm_current_char(model);
                    model.is_done = true;
                }
                '\x1b' => {
                    cancel_input(model);
                }
                c if (c as u32) >= 0x20 => {
                    if !model.digits_only || c.is_ascii_digit() {
                        Self::confirm_current_char(model);
                        if model.buffer.len() + c.len_utf8() <= model.max_len {
                            let _ = model.buffer.push(c);
                        }
                        #[cfg(feature = "passcode")]
                        if model.is_passcode
                            && model.expose_len
                            && model.buffer.len() >= model.max_len
                        {
                            model.is_done = true;
                        }
                    }
                }
                _ => {}
            }
            model.last_activity_tick = model.ticks;
            return true;
        }

        if let Event::Key { code, state } = event {
            let is_pressed = *state == KeyState::Pressed;
            let is_held = *state == KeyState::Held;
            let is_released = *state == KeyState::Released;

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
                            confirm_and_push(model, b'0' + digit);
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
                            pop_utf8_char(&mut model.buffer);
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
                        confirm_and_push(model, b'0' + key_num);
                        model.last_activity_tick = model.ticks;
                        return true;
                    }

                    let chars_count = utf8_char_count(model.key_chars(key_num));
                    if chars_count == 0 {
                        return true;
                    }

                    let is_same_key = model.last_key == Some(key_num);
                    let is_recent = (model.ticks.wrapping_sub(model.last_activity_tick)) < 15;

                    let mut glyph = [0u8; 4];
                    if is_same_key && is_recent && !model.buffer.is_empty() {
                        // Multi-tap cycle through UTF-8 characters safely
                        model.press_count += 1;
                        if model.press_count >= chars_count {
                            model.press_count = 0;
                        }
                        if let Some(s) = utf8_nth_slice(model.key_chars(key_num), model.press_count) {
                            let n = s.len().min(4);
                            glyph[..n].copy_from_slice(&s.as_bytes()[..n]);
                            pop_utf8_char(&mut model.buffer);
                            let _ = model.buffer.push_str(unsafe {
                                core::str::from_utf8_unchecked(&glyph[..n])
                            });
                        }
                    } else if let Some(s) = utf8_nth_slice(model.key_chars(key_num), 0) {
                        // First tap on a new key
                        let n = s.len().min(4);
                        glyph[..n].copy_from_slice(&s.as_bytes()[..n]);
                        TextInputView::confirm_current_char(model);
                        if model.buffer.len() + n <= model.max_len {
                            let _ = model.buffer.push_str(unsafe {
                                core::str::from_utf8_unchecked(&glyph[..n])
                            });
                        }
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
                    }

                    KeyCode::Side2 if !model.digits_only => {
                        model.show_grid = !model.show_grid;
                    }

                    KeyCode::Star | KeyCode::KeyF | KeyCode::Hash | KeyCode::Side1
                        if !model.digits_only =>
                    {
                        Self::confirm_current_char(model);
                        let has_loc = model.locale_keymap.active;
                        model.charset = match (*code == KeyCode::Star, model.charset) {
                            (true, Charset::Upper | Charset::Lower) if has_loc => Charset::Locale,
                            (true, Charset::Upper | Charset::Lower | Charset::Locale) => {
                                Charset::Numbers
                            }
                            (true, Charset::Numbers) => Charset::Symbols,
                            (false, Charset::Upper) => Charset::Lower,
                            (false, Charset::Lower) if has_loc => Charset::Locale,
                            _ => Charset::Upper,
                        };
                    }

                    KeyCode::Menu => {
                        // Single tap MENU submits/finishes
                        Self::confirm_current_char(model);
                        model.is_done = true;
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

#[inline(never)]
fn confirm_and_push(model: &mut TextInputModel, ch: u8) {
    TextInputView::confirm_current_char(model);
    if utf8_char_count(&model.buffer) < model.max_len && model.buffer.len() < model.buffer.capacity()
    {
        let buf = [ch];
        let _ = model
            .buffer
            .push_str(unsafe { core::str::from_utf8_unchecked(&buf) });
    }
    #[cfg(feature = "passcode")]
    if model.is_passcode && model.expose_len && utf8_char_count(&model.buffer) >= model.max_len {
        model.is_done = true;
    }
}

#[inline(never)]
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

    #[test]
    fn test_direct_char_input_and_controls() {
        let mut model = TextInputModel::new(16);
        let mut view = TextInputView;

        for ch in "Hello".chars() {
            assert!(view.on_event(&mut model, &Event::Char(ch)));
        }
        assert_eq!(model.buffer.as_str(), "Hello");

        // Backspace pops cleanly
        assert!(view.on_event(&mut model, &Event::Char('\x08')));
        assert_eq!(model.buffer.as_str(), "Hell");

        // Enter submits
        assert!(view.on_event(&mut model, &Event::Char('\n')));
        assert!(model.is_done);

        // digits_only filters non-digits
        let mut num_model = TextInputModel::new(8);
        num_model.digits_only = true;
        for ch in "1a2b3".chars() {
            assert!(view.on_event(&mut num_model, &Event::Char(ch)));
        }
        assert_eq!(num_model.buffer.as_str(), "123");
    }

    #[test]
    fn test_locale_utf8_multitap_and_automatic_keypad_guide() {
        let mut model = TextInputModel::new(16);
        let mut view = TextInputView;
        assert!(!model.needs_keypad_guide());

        // Load a Russian Cyrillic keymap on key 2 ("АБВГ")
        model.locale_keymap.active = true;
        model.locale_keymap.set_label("АБВ");
        model.locale_keymap.set_key(2, "АБВГ");
        model.charset = Charset::Locale;

        // Non-factory layout automatically activates the on-screen 3x3 keypad guide
        assert!(model.needs_keypad_guide());
        assert_eq!(model.charset_label(), "АБВ");

        // Multi-tap key 2 twice -> 'А' then cleanly replaces UTF-8 multi-byte char with 'Б'
        let press2 = Event::Key {
            code: KeyCode::Key2,
            state: KeyState::Pressed,
        };
        assert!(view.on_event(&mut model, &press2));
        assert_eq!(model.buffer.as_str(), "А");
        assert!(view.on_event(&mut model, &press2));
        assert_eq!(model.buffer.as_str(), "Б");

        // Render with active key highlight + live cycle strip without panicking
        let mut canvas = Canvas::new();
        view.render(&mut canvas, &model);
        assert!(canvas.buffer.iter().flatten().any(|&b| b != 0));

        // Side2 toggles the guide manually even in factory Latin mode
        model.charset = Charset::Upper;
        assert!(!model.needs_keypad_guide());
        let side2 = Event::Key {
            code: KeyCode::Side2,
            state: KeyState::Pressed,
        };
        assert!(view.on_event(&mut model, &side2));
        assert!(model.needs_keypad_guide());
    }
}
