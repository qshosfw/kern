//! Text & Numeric Input Prompt Views (`TextPromptView`, `NumberPromptView`).

use super::View;
use super::animation::TransitionProgress;
use crate::elements::{
    draw_key_badge, draw_key_badge_right, draw_negated_header, draw_rounded_box, draw_sheet_badges,
    draw_sheet_slide_up,
};
use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::text::{self, Align};
use crate::{Canvas, Color};
use heapless::String;
use kernel::event::{Event, KeyCode, KeyState};

pub const DICT_FILE_ID: [u8; 4] = *b"DICT";
pub const DICT_HEADER_SIZE: usize = 16;
pub const DICT_RECORD_SIZE: usize = 32;

pub trait TextSuggestionProvider {
    fn suggest(&self, prefix: &str, out: &mut [String<12>; 3]) -> usize;
}

utils::static_bytes!(
    static BUILTIN_CHARSET_PACK = b"\
ABC\0123\0abc\0 \0.,?!'-\0ABC\0DEF\0GHI\0JKL\0MNO\0PQRS\0TUV\0WXYZ\0\
abc\0123\0ABC\0 \0.,?!'-\0abc\0def\0ghi\0jkl\0mno\0pqrs\0tuv\0wxyz\0\
123\0SYM\0Case\00\01\02\03\04\05\06\07\08\09\0\
#@$\0ABC\0Case\0 \0.,?!'-\0@#$\0%&*\0()[]\0<>{}/\0\\|+\0-=\"\0'` :\0;_\0";
);

#[derive(Debug, Clone)]
#[repr(C)]
pub struct LocaleKeymap {
    pub active: bool,
    label_len: u8,
    offsets: [u8; 11],
    label_buf: [u8; 8],
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
        self.label_len = utils::format::copy_trunc_utf8(&mut self.label_buf, 0, s);
    }

    #[inline(always)]
    pub fn label_str(&self) -> &str {
        let len = (self.label_len as usize).min(self.label_buf.len());
        unsafe { core::str::from_utf8_unchecked(self.label_buf.get_unchecked(..len)) }
    }

    pub fn set_key(&mut self, key: usize, s: &str) {
        if key >= 10 {
            return;
        }
        let end = utils::format::copy_trunc_utf8(&mut self.buf, self.offsets[key] as usize, s);
        for o in &mut self.offsets[key + 1..] {
            *o = end;
        }
    }

    pub fn key_str(&self, key: usize) -> &str {
        if key >= 10 {
            return "";
        }
        let s = self.offsets[key] as usize;
        let e = self.offsets[key + 1] as usize;
        if e <= s || e > self.buf.len() {
            return "";
        }
        unsafe { core::str::from_utf8_unchecked(self.buf.get_unchecked(s..e)) }
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
    fn field(self, slot: u8) -> &'static str {
        let idx = (self as u8).min(3);
        utils::format::nth_cstr(&BUILTIN_CHARSET_PACK, idx * 13 + slot)
    }

    #[inline(always)]
    pub fn label(self) -> &'static str {
        self.field(0)
    }

    #[inline(always)]
    pub fn star_hint(self) -> &'static str {
        self.field(1)
    }

    #[inline(always)]
    pub fn case_hint(self) -> &'static str {
        self.field(2)
    }

    #[inline(always)]
    pub fn key_str(self, key: u8) -> &'static str {
        self.field(3 + key)
    }

    #[inline(always)]
    pub fn show_grid_on_k1(self) -> bool {
        self == Charset::Symbols
    }
}

#[repr(C)]
pub struct TextPromptState {
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
    pub cursor_from_end: u8,
    pub max_len: usize,
    pub press_count: usize,
    pub last_activity_tick: u32,
    pub ticks: u32,
    pub exit_press_ticks: u32,
    pub hold_ticks: u32,
    pub buffer: utils::format::InlineStr<32>,
    pub locale_keymap: LocaleKeymap,
    pub custom_title: utils::format::InlineStr<16>,
}

impl TextPromptState {
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
            cursor_from_end: 0,
            max_len,
            press_count: 0,
            last_activity_tick: 0,
            ticks: 0,
            exit_press_ticks: 0,
            hold_ticks: 24,
            custom_title: utils::format::InlineStr::new(),
            buffer: utils::format::InlineStr::new(),
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
            self.charset.label()
        }
    }

    #[inline(always)]
    pub fn needs_keypad_guide(&self) -> bool {
        !self.digits_only
            && (self.show_grid
                || self.charset == Charset::Locale
                || self.charset.show_grid_on_k1()
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
            self.charset.key_str(key)
        }
    }
}

pub struct TextPromptView;

fn utf8_char_count(s: &str) -> usize {
    let mut n = 0usize;
    for &b in s.as_bytes() {
        if (b & 0xC0) != 0x80 {
            n += 1;
        }
    }
    n
}

fn next_utf8_end(b: &[u8], i: usize) -> usize {
    let mut next = i + 1;
    while next < b.len() && (unsafe { *b.get_unchecked(next) } & 0xC0) == 0x80 {
        next += 1;
    }
    next
}

fn utf8_nth_slice(s: &str, nth: usize) -> Option<&str> {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut c = 0usize;
    while i < b.len() {
        let next = next_utf8_end(b, i);
        if c == nth {
            return Some(unsafe { core::str::from_utf8_unchecked(b.get_unchecked(i..next)) });
        }
        c += 1;
        i = next;
    }
    None
}

fn cursor_byte_pos(s: &str, from_end: u8) -> usize {
    let b = s.as_bytes();
    let mut pos = b.len();
    let mut rem = from_end;
    while rem > 0 && pos > 0 {
        pos = utils::format::last_utf8_start(unsafe { b.get_unchecked(..pos) });
        rem -= 1;
    }
    pos
}

#[inline(always)]
fn utf8_slice_before(s: &str, pos: usize) -> Option<&str> {
    let b = unsafe { s.as_bytes().get_unchecked(..pos.min(s.len())) };
    if b.is_empty() {
        return None;
    }
    let start = utils::format::last_utf8_start(b);
    Some(unsafe { core::str::from_utf8_unchecked(b.get_unchecked(start..)) })
}

impl TextPromptView {
    pub const fn new() -> Self {
        Self
    }

    fn confirm_current_char(model: &mut TextPromptState) {
        model.last_key = None;
        model.press_count = 0;
    }

    fn draw_t9_grid(canvas: &mut Canvas, model: &TextPromptState, is_cycling: bool) {
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
            text::draw_str_max_x(
                canvas,
                x + 9,
                y + 5,
                model.key_chars(key),
                &TOMTHUMB,
                txt_col,
                max_x,
            );
            if key == 3 || key == 6 {
                x = 2;
                y += 8;
            } else {
                x += 42;
            }
        }
    }

    fn draw_cycle_strip(canvas: &mut Canvas, model: &TextPromptState, y: i16) {
        let Some(key) = model.last_key else {
            return;
        };
        let b = model.key_chars(key).as_bytes();
        let mut cx = 28i16;
        let mut i = 0usize;
        let mut idx = 0usize;
        while i < b.len() {
            let next = next_utf8_end(b, i);
            let s = unsafe { core::str::from_utf8_unchecked(b.get_unchecked(i..next)) };
            let w = text::str_width(s, &TOMTHUMB).max(4);
            if cx + w + 2 > 104 {
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

    fn draw_bottom_badges(canvas: &mut Canvas, model: &TextPromptState, by: i16) {
        let exit_label = if model.buffer.is_empty() {
            "Back"
        } else {
            "Del"
        };
        if !model.digits_only {
            draw_key_badge(canvas, 2, by, "*", model.charset.star_hint());
            draw_key_badge(
                canvas,
                46,
                by,
                if model.is_k1 { "F" } else { "#" },
                model.charset.case_hint(),
            );
        }
        draw_key_badge_right(canvas, 126, by, "EXIT", exit_label);
    }

    #[cfg(feature = "passcode")]
    fn draw_passcode_dots(canvas: &mut Canvas, model: &TextPromptState, cy: i16, is_cycling: bool) {
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
            return;
        }
        let spacing = if total_slots > 8 { 9i16 } else { 12i16 };
        let start_x = 61 - ((total_slots as i16 - 1) * spacing) / 2;
        let yu = cy as u8;
        for i in 0..total_slots {
            let cx = start_x + (i as i16) * spacing;
            if i + 1 == entered_len && is_cycling {
                text::draw_str(
                    canvas,
                    cx + 1,
                    cy + 6,
                    &model.buffer[entered_len - 1..],
                    &MUMATRIX,
                    Color::Fill,
                );
            } else {
                draw_rounded_box(canvas, cx as u8, yu, 6, 6);
                if i < entered_len {
                    canvas.fill_rect((cx + 1) as u8, yu + 1, 4, 4, Color::Fill);
                }
            }
        }
    }
}

impl Default for TextPromptView {
    fn default() -> Self {
        Self::new()
    }
}

impl View<TextPromptState> for TextPromptView {
    fn render(&self, canvas: &mut Canvas, model: &TextPromptState) {
        let title_s = model.title_str();
        let clear_y: u8 = if model.show_header && !title_s.is_empty() {
            draw_negated_header(canvas, 0, title_s);
            9
        } else {
            8
        };
        canvas.fill_rect(0, clear_y, 128, 64 - clear_y, Color::Clear);

        let header_y = 16i16;
        let is_cycling =
            model.last_key.is_some() && (model.ticks.wrapping_sub(model.last_activity_tick) < 24);
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
                Align::Right,
            );
        }

        let show_grid = model.needs_keypad_guide();
        let text_y = if show_grid { 26i16 } else { 33i16 };

        #[cfg(feature = "passcode")]
        if is_passcode {
            Self::draw_passcode_dots(canvas, model, if show_grid { 22 } else { 29 }, is_cycling);
        }
        if !is_passcode {
            let text_x = 5i16;
            let cur_pos = cursor_byte_pos(&model.buffer, model.cursor_from_end);
            let prefix = unsafe {
                core::str::from_utf8_unchecked(model.buffer.as_bytes().get_unchecked(..cur_pos))
            };
            let prefix_w = text::str_width(prefix, &MUMATRIX);
            let max_w = 118i16;
            let scroll_x = (prefix_w - max_w).max(0);

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

            let cursor_x = (text_x - scroll_x) + prefix_w;
            if is_cycling && let Some(last_char_str) = utf8_slice_before(&model.buffer, cur_pos) {
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
            55
        };
        Self::draw_bottom_badges(canvas, model, by);
    }

    fn on_event(&mut self, model: &mut TextPromptState, event: &Event) -> bool {
        if let Event::Tick = event {
            model.ticks = model.ticks.wrapping_add(1);
            if model.exit_pressed && !model.exit_held {
                model.exit_press_ticks = model.exit_press_ticks.saturating_add(1);
                if model.exit_press_ticks >= hold_threshold(model) {
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
                    delete_at_cursor(model);
                }
                '\r' | '\n' => {
                    Self::confirm_current_char(model);
                    model.is_done = true;
                }
                '\x1b' => cancel_input(model),
                c if (c as u32) >= 0x20 => {
                    if !model.digits_only || c.is_ascii_digit() {
                        Self::confirm_current_char(model);
                        if model.buffer.len() + c.len_utf8() <= model.max_len {
                            let mut b = [0u8; 4];
                            insert_at_cursor(model, c.encode_utf8(&mut b));
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

        if let Event::Encoder(delta) = *event {
            if delta != 0 {
                move_cursor(model, if delta < 0 { 1 } else { -1 });
            }
            return true;
        }

        if let Event::Key { code, state } = event {
            if *code == KeyCode::Menu && *state != KeyState::Released {
                Self::confirm_current_char(model);
                model.is_done = true;
                return true;
            }
            match *state {
                KeyState::Held => match code {
                    KeyCode::Exit => {
                        if !model.exit_pressed || model.exit_press_ticks >= hold_threshold(model) {
                            cancel_input(model);
                        }
                    }
                    _ => {
                        if let Some(digit) = code.to_digit()
                            && !model.digits_only
                            && model.charset != Charset::Numbers
                        {
                            if model.last_key == Some(digit) && !model.buffer.is_empty() {
                                delete_at_cursor(model);
                            }
                            confirm_and_push(model, b'0' + digit);
                        }
                    }
                },
                KeyState::Released if *code == KeyCode::Exit => {
                    if model.exit_pressed {
                        model.exit_pressed = false;
                        if !model.exit_held {
                            if model.last_key.is_some() {
                                Self::confirm_current_char(model);
                            } else if !model.buffer.is_empty() {
                                delete_at_cursor(model);
                            } else {
                                cancel_input(model);
                            }
                        }
                    }
                    model.exit_held = false;
                    model.exit_press_ticks = 0;
                }
                KeyState::Pressed => {
                    if let Some(key_num) = code.to_digit() {
                        handle_digit_press(model, key_num);
                    } else {
                        match code {
                            KeyCode::Up => move_cursor(model, 1),
                            KeyCode::Down => move_cursor(model, -1),
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
                                    (true, Charset::Upper | Charset::Lower) if has_loc => {
                                        Charset::Locale
                                    }
                                    (true, Charset::Upper | Charset::Lower | Charset::Locale) => {
                                        Charset::Numbers
                                    }
                                    (true, Charset::Numbers) => Charset::Symbols,
                                    (false, Charset::Upper) => Charset::Lower,
                                    (false, Charset::Lower) if has_loc => Charset::Locale,
                                    _ => Charset::Upper,
                                };
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            return true;
        }
        false
    }
}

#[inline(always)]
fn hold_threshold(model: &TextPromptState) -> u32 {
    if model.hold_ticks > 0 {
        model.hold_ticks
    } else {
        24
    }
}

fn delete_at_cursor(model: &mut TextPromptState) {
    let pos = cursor_byte_pos(&model.buffer, model.cursor_from_end);
    utils::format::delete_utf8_before(&mut model.buffer.buf, &mut model.buffer.len, pos);
}

fn insert_at_cursor(model: &mut TextPromptState, s: &str) {
    let pos = cursor_byte_pos(&model.buffer, model.cursor_from_end);
    utils::format::insert_utf8_at(&mut model.buffer.buf, &mut model.buffer.len, pos, s);
}

fn move_cursor(model: &mut TextPromptState, step: i8) {
    if model.is_passcode {
        return;
    }
    TextPromptView::confirm_current_char(model);
    if step > 0 {
        let max_c = utf8_char_count(&model.buffer).min(255) as u8;
        model.cursor_from_end = model.cursor_from_end.saturating_add(1).min(max_c);
    } else {
        model.cursor_from_end = model.cursor_from_end.saturating_sub(1);
    }
}

fn handle_digit_press(model: &mut TextPromptState, key_num: u8) {
    if model.digits_only || model.charset == Charset::Numbers {
        confirm_and_push(model, b'0' + key_num);
        model.last_activity_tick = model.ticks;
        return;
    }
    let chars_count = utf8_char_count(model.key_chars(key_num));
    if chars_count == 0 {
        return;
    }
    let is_cycle = model.last_key == Some(key_num)
        && (model.ticks.wrapping_sub(model.last_activity_tick)) < 24
        && !model.buffer.is_empty();
    let next_idx = if is_cycle {
        let p = model.press_count + 1;
        if p >= chars_count { 0 } else { p }
    } else {
        0
    };
    if let Some(s) = utf8_nth_slice(model.key_chars(key_num), next_idx) {
        let mut glyph = [0u8; 4];
        let sb = s.as_bytes();
        let n = sb.len().min(4);
        glyph[..n].copy_from_slice(&sb[..n]);
        let g = unsafe { core::str::from_utf8_unchecked(glyph.get_unchecked(..n)) };
        if is_cycle {
            model.press_count = next_idx;
            delete_at_cursor(model);
            insert_at_cursor(model, g);
        } else {
            TextPromptView::confirm_current_char(model);
            if model.buffer.len() + n <= model.max_len {
                insert_at_cursor(model, g);
            }
            model.last_key = Some(key_num);
            model.press_count = 0;
        }
    }
    model.last_activity_tick = model.ticks;
}

fn confirm_and_push(model: &mut TextPromptState, ch: u8) {
    TextPromptView::confirm_current_char(model);
    if utf8_char_count(&model.buffer) < model.max_len
        && model.buffer.len() < model.buffer.capacity()
    {
        let buf = [ch];
        insert_at_cursor(model, unsafe { core::str::from_utf8_unchecked(&buf) });
    }
    #[cfg(feature = "passcode")]
    if model.is_passcode && model.expose_len && utf8_char_count(&model.buffer) >= model.max_len {
        model.is_done = true;
    }
}

fn cancel_input(model: &mut TextPromptState) {
    model.exit_held = true;
    model.exit_pressed = false;
    model.exit_press_ticks = 0;
    model.cursor_from_end = 0;
    TextPromptView::confirm_current_char(model);
    model.buffer.clear();
    model.is_canceled = true;
}

// ── Numeric & Frequency Bottom-Sheet Prompt (`NumberPromptState`, `NumberPromptView`) ──

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct NumberPromptState {
    pub is_done: bool,
    pub is_canceled: bool,
    pub decimals: u8,
    pub entry_len: u8,
    pub entry_neg: bool,
    pub title_len: u8,
    pub unit_len: u8,
    pub anim: TransitionProgress,
    pub value: i32,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub entry_digits: [u8; 10],
    pub title: [u8; 16],
    pub unit: [u8; 6],
}

impl NumberPromptState {
    pub const fn new(value: i32, min: i32, max: i32, step: i32, decimals: u8) -> Self {
        let lo = if min <= max { min } else { max };
        let hi = if min <= max { max } else { min };
        let clamped = if value < lo {
            lo
        } else if value > hi {
            hi
        } else {
            value
        };
        Self {
            is_done: false,
            is_canceled: false,
            decimals: if decimals > 5 { 5 } else { decimals },
            entry_len: 0,
            entry_neg: false,
            title_len: 0,
            unit_len: 0,
            anim: TransitionProgress::new(4),
            value: clamped,
            min: lo,
            max: hi,
            step: if step > 0 { step } else { 1 },
            entry_digits: [0; 10],
            title: [0; 16],
            unit: [0; 6],
        }
    }

    fn copy_field(dst: &mut [u8], len_out: &mut u8, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(dst.len());
        unsafe { core::ptr::copy_nonoverlapping(b.as_ptr(), dst.as_mut_ptr(), n) };
        *len_out = n as u8;
    }

    pub fn set_title(&mut self, s: &str) {
        Self::copy_field(&mut self.title, &mut self.title_len, s);
    }

    pub fn set_unit(&mut self, s: &str) {
        Self::copy_field(&mut self.unit, &mut self.unit_len, s);
    }

    #[inline(always)]
    pub fn title_str(&self) -> &str {
        let n = (self.title_len as usize).min(self.title.len());
        unsafe { core::str::from_utf8_unchecked(self.title.get_unchecked(..n)) }
    }

    #[inline(always)]
    pub fn unit_str(&self) -> &str {
        let n = (self.unit_len as usize).min(self.unit.len());
        unsafe { core::str::from_utf8_unchecked(self.unit.get_unchecked(..n)) }
    }

    fn step_by(&mut self, mult: i32) {
        self.entry_len = 0;
        let delta = self.step.wrapping_mul(mult);
        self.value = self.value.saturating_add(delta).clamp(self.min, self.max);
    }

    fn commit_typed(&mut self) -> bool {
        if self.entry_len == 0 {
            return (self.min..=self.max).contains(&self.value);
        }
        let mut acc: i32 = 0;
        let mut overflow = false;
        for &d in self.entry_digits.iter().take(self.entry_len as usize) {
            if acc > 214_748_364 {
                overflow = true;
                break;
            }
            let next = acc * 10 + (d as i32);
            if next < acc {
                overflow = true;
                break;
            }
            acc = next;
        }
        if self.entry_neg {
            acc = acc.saturating_neg();
        }
        self.entry_len = 0;
        self.entry_neg = false;
        if !overflow && acc >= self.min && acc <= self.max {
            self.value = acc;
            true
        } else {
            false
        }
    }
}

pub struct NumberPromptView;

impl NumberPromptView {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for NumberPromptView {
    fn default() -> Self {
        Self::new()
    }
}

impl View<NumberPromptState> for NumberPromptView {
    fn render(&self, canvas: &mut Canvas, m: &NumberPromptState) {
        let sheet_h = 46u8;
        let sheet_y =
            draw_sheet_slide_up(canvas, m.title_str(), sheet_h, m.anim.slide_offset(sheet_h));
        if m.anim.progress() < 8 {
            return;
        }

        draw_sheet_badges(
            canvas,
            sheet_y,
            sheet_h,
            "OK",
            if m.entry_len > 0 { "DEL" } else { "BACK" },
        );

        let mut vbuf = [0u8; 24];
        let val_str = if m.entry_len > 0 {
            let p = vbuf.as_mut_ptr();
            let mut pos = 0usize;
            if m.entry_neg {
                unsafe { *p = b'-' };
                pos = 1;
            }
            let elen = (m.entry_len as usize).min(m.entry_digits.len());
            for i in 0..elen {
                unsafe { *p.add(pos) = b'0' + *m.entry_digits.get_unchecked(i) };
                pos += 1;
            }
            unsafe { *p.add(pos) = b'_' };
            pos += 1;
            unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(p, pos)) }
        } else {
            utils::format::format_int_fixed(m.value, m.decimals, "", m.unit_str(), &mut vbuf)
        };

        let box_y = (sheet_y + 12).clamp(0, 48) as u8;
        draw_rounded_box(canvas, 8, box_y, 112, 14);
        text::draw_str_align(
            canvas,
            64,
            box_y as i16 + 10,
            val_str,
            &MUMATRIX,
            Color::Fill,
            Align::Center,
        );

        let range_y = box_y as i16 + 20;
        if range_y < 54 {
            let lo_s = utils::format::format_int_fixed(m.min, m.decimals, "", "", &mut vbuf);
            text::draw_str(canvas, 10, range_y, lo_s, &TOMTHUMB, Color::DarkGray);
            let hi_s =
                utils::format::format_int_fixed(m.max, m.decimals, "", m.unit_str(), &mut vbuf);
            text::draw_str_align(
                canvas,
                118,
                range_y,
                hi_s,
                &TOMTHUMB,
                Color::DarkGray,
                Align::Right,
            );
        }
    }

    fn on_event(&mut self, m: &mut NumberPromptState, event: &Event) -> bool {
        match *event {
            Event::Tick => m.anim.tick(),
            Event::Encoder(delta) if delta != 0 => {
                m.step_by(delta as i32);
                true
            }
            Event::Key {
                code,
                state: KeyState::Pressed,
            } => match code {
                KeyCode::Up | KeyCode::Down | KeyCode::Side1 | KeyCode::Side2 => {
                    let mult = match code {
                        KeyCode::Up => 1,
                        KeyCode::Down => -1,
                        KeyCode::Side1 => -10,
                        _ => 10,
                    };
                    m.step_by(mult);
                    true
                }
                KeyCode::Star => {
                    if m.min < 0 {
                        if m.entry_len > 0 {
                            m.entry_neg = !m.entry_neg;
                        } else {
                            m.value = m.value.saturating_neg().clamp(m.min, m.max);
                        }
                    } else {
                        m.entry_len = 0;
                    }
                    true
                }
                KeyCode::Hash => {
                    m.entry_len = 0;
                    m.entry_neg = false;
                    true
                }
                KeyCode::Exit => {
                    if m.entry_len > 0 {
                        m.entry_len -= 1;
                        if m.entry_len == 0 {
                            m.entry_neg = false;
                        }
                    } else {
                        m.is_canceled = true;
                    }
                    true
                }
                KeyCode::Menu => {
                    if m.commit_typed() {
                        m.is_done = true;
                    }
                    true
                }
                _ => {
                    let raw = code as u8;
                    if (KeyCode::Key0 as u8..=KeyCode::Key9 as u8).contains(&raw) {
                        if (m.entry_len as usize) < m.entry_digits.len() {
                            m.entry_digits[m.entry_len as usize] = raw - KeyCode::Key0 as u8;
                            m.entry_len += 1;
                        }
                        return true;
                    }
                    false
                }
            },
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_prompt_multitap_cursor_and_exit_semantics() {
        let mut model = TextPromptState::new(16);
        let mut view = TextPromptView;
        model.locale_keymap.active = true;
        model.locale_keymap.set_label("АБВ");
        model.locale_keymap.set_key(2, "АБВГ");
        model.charset = Charset::Locale;
        assert!(model.needs_keypad_guide());

        let p2 = Event::Key {
            code: KeyCode::Key2,
            state: KeyState::Pressed,
        };
        assert!(view.on_event(&mut model, &p2));
        assert_eq!(model.buffer.as_str(), "А");
        assert!(view.on_event(&mut model, &p2));
        assert_eq!(model.buffer.as_str(), "Б");

        let mut num = NumberPromptState::new(145_500, 18_000, 1_300_000, 25, 3);
        let mut nview = NumberPromptView::new();
        assert!(nview.on_event(&mut num, &Event::Encoder(2)));
        assert_eq!(num.value, 145_550);
    }
}
