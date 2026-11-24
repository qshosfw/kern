//! Numeric & Frequency Bottom-Sheet Input Module (`NumberInputModel`, `NumberInputView`).
//!
//! Renders as a native 46px slide-up bottom sheet with negated header and floating `[M] OK` /
//! `[EXIT] BACK` key badges — matching `PopupView` and `OptionSheet` with zero heap allocation.

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::animation::TransitionProgress;
use crate::gui::view::View;
use crate::text::{self, Align};
use crate::widgets;
use crate::{Canvas, Color};
use kernel::event::{Event, KeyCode, KeyState};

/// Compact fixed-point / integer / frequency input model (`60 B`, zero heap).
/// Supports rotary/Up-Down stepping (`+/- step`), `Key4`/`Key6` coarse `10x` stepping,
/// and direct keypad digit entry with `[min..=max]` clamping.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct NumberInputModel {
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

impl NumberInputModel {
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

    pub fn set_title(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(self.title.len());
        self.title[..n].copy_from_slice(&b[..n]);
        self.title_len = n as u8;
    }

    pub fn set_unit(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(self.unit.len());
        self.unit[..n].copy_from_slice(&b[..n]);
        self.unit_len = n as u8;
    }

    #[inline(always)]
    pub fn title_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.title[..self.title_len as usize]) }
    }

    #[inline(always)]
    pub fn unit_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.unit[..self.unit_len as usize]) }
    }

    fn step_by(&mut self, mult: i32) {
        self.entry_len = 0;
        let delta = self.step.saturating_mul(mult);
        self.value = self.value.saturating_add(delta).clamp(self.min, self.max);
    }

    fn commit_typed(&mut self) -> bool {
        if self.entry_len == 0 {
            return (self.min..=self.max).contains(&self.value);
        }
        let mut acc: i32 = 0;
        let mut overflow = false;
        for i in 0..(self.entry_len as usize) {
            if let Some(next) = acc
                .checked_mul(10)
                .and_then(|v| v.checked_add(self.entry_digits[i] as i32))
            {
                acc = next;
            } else {
                overflow = true;
                break;
            }
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

#[inline(never)]
fn fmt_num_dec<'a>(val: i32, decimals: u8, unit: &str, out: &'a mut [u8; 24]) -> &'a str {
    let mut pos = 0usize;
    let abs = if val < 0 {
        out[0] = b'-';
        pos = 1;
        val.unsigned_abs()
    } else {
        val as u32
    };
    let mut tmp = [0u8; 10];
    let digits = text::format_u32(abs, &mut tmp).as_bytes();
    let dec = decimals as usize;
    if dec > 0 {
        let pad = (dec + 1).saturating_sub(digits.len());
        let total = digits.len() + pad;
        let int_len = total - dec;
        for i in 0..total {
            if i == int_len && pos < out.len() {
                out[pos] = b'.';
                pos += 1;
            }
            if pos < out.len() {
                out[pos] = if i < pad { b'0' } else { digits[i - pad] };
                pos += 1;
            }
        }
    } else {
        let n = digits.len().min(out.len() - pos);
        out[pos..pos + n].copy_from_slice(&digits[..n]);
        pos += n;
    }
    let ub = unit.as_bytes();
    let un = ub.len().min(out.len() - pos);
    out[pos..pos + un].copy_from_slice(&ub[..un]);
    pos += un;
    unsafe { core::str::from_utf8_unchecked(&out[..pos]) }
}

pub struct NumberInputView;

impl NumberInputView {
    pub const fn new() -> Self {
        Self
    }
}

impl Default for NumberInputView {
    fn default() -> Self {
        Self::new()
    }
}

impl View<NumberInputModel> for NumberInputView {
    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, m: &NumberInputModel) {
        let sheet_h = 46u8;
        let sheet_y = widgets::draw_sheet_slide_up(
            canvas,
            m.title_str(),
            sheet_h,
            m.anim.slide_offset(sheet_h),
        );
        if m.anim.progress() < 8 {
            return;
        }

        let cancel_lbl = if m.entry_len > 0 { "DEL" } else { "BACK" };
        widgets::draw_sheet_badges(canvas, sheet_y, sheet_h, "OK", cancel_lbl);

        let mut vbuf = [0u8; 24];
        let val_str = if m.entry_len > 0 {
            let mut pos = 0usize;
            if m.entry_neg {
                vbuf[pos] = b'-';
                pos += 1;
            }
            for i in 0..(m.entry_len as usize) {
                vbuf[pos] = b'0' + m.entry_digits[i];
                pos += 1;
            }
            vbuf[pos] = b'_';
            pos += 1;
            unsafe { core::str::from_utf8_unchecked(&vbuf[..pos]) }
        } else {
            fmt_num_dec(m.value, m.decimals, m.unit_str(), &mut vbuf)
        };

        let box_y = (sheet_y + 12).clamp(0, 48) as u8;
        widgets::draw_rounded_box(canvas, 8, box_y, 112, 14);
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
            let lo_s = fmt_num_dec(m.min, m.decimals, "", &mut vbuf);
            text::draw_str(canvas, 10, range_y, lo_s, &TOMTHUMB, Color::DarkGray);
            let hi_s = fmt_num_dec(m.max, m.decimals, m.unit_str(), &mut vbuf);
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

    #[inline(never)]
    fn on_event(&mut self, m: &mut NumberInputModel, event: &Event) -> bool {
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
                KeyCode::Up => {
                    m.step_by(1);
                    true
                }
                KeyCode::Down => {
                    m.step_by(-1);
                    true
                }
                KeyCode::Side1 => {
                    m.step_by(-10);
                    true
                }
                KeyCode::Side2 => {
                    m.step_by(10);
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
                        let digit = raw - KeyCode::Key0 as u8;
                        if (m.entry_len as usize) < m.entry_digits.len() {
                            m.entry_digits[m.entry_len as usize] = digit;
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
    fn number_input_supports_rotary_step_and_direct_digit_commit() {
        let mut m = NumberInputModel::new(145_500, 18_000, 1_300_000, 25, 3);
        m.set_title("Freq kHz");
        m.set_unit("MHz");
        m.anim.jump_complete();
        let mut view = NumberInputView::new();

        assert!(view.on_event(&mut m, &Event::Encoder(2)));
        assert_eq!(m.value, 145_550);

        for k in [
            KeyCode::Key4,
            KeyCode::Key3,
            KeyCode::Key3,
            KeyCode::Key5,
            KeyCode::Key0,
            KeyCode::Key0,
        ] {
            assert!(view.on_event(
                &mut m,
                &Event::Key {
                    code: k,
                    state: KeyState::Pressed,
                },
            ));
        }
        assert!(view.on_event(
            &mut m,
            &Event::Key {
                code: KeyCode::Menu,
                state: KeyState::Pressed,
            },
        ));
        assert!(m.is_done);
        assert_eq!(m.value, 433_500);

        let mut cv = Canvas::new();
        view.render(&mut cv, &m);
    }
}
