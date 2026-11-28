//! Slide-Up Option Sheet View (`SheetView`, `OptionSheet`, `SheetOptions`, `SheetResult`).

#[cfg(test)]
use super::list::ShortString;
use super::list::{KeyNavStep, classify_nav_key};
use super::repeater::KeyRepeater;
use crate::Canvas;
use crate::elements::{
    ListRowSpec, ROW_SELECTED, draw_list_row, draw_scrollbar, draw_sheet_slide_up,
};
use crate::icons;
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SheetOptions {
    #[cfg(test)]
    Dynamic(&'static [ShortString]),
    Packed(&'static str),
}

impl SheetOptions {
    pub fn len(&self) -> usize {
        match self {
            #[cfg(test)]
            SheetOptions::Dynamic(d) => d.len(),
            SheetOptions::Packed(p) => {
                let mut count = 0;
                for &b in p.as_bytes() {
                    if b == 0 {
                        count += 1;
                    }
                }
                count
            }
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, idx: usize) -> &str {
        match self {
            #[cfg(test)]
            SheetOptions::Dynamic(d) => d.get(idx).map_or("", ShortString::as_str),
            SheetOptions::Packed(p) => {
                if idx <= u8::MAX as usize {
                    utils::format::nth_cstr(p.as_bytes(), idx as u8)
                } else {
                    ""
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct OptionSheet {
    pub show_radio: bool,
    pub is_pressed: bool,
    pub hover: usize,
    pub selected: usize,
    pub title: &'static str,
    pub options: SheetOptions,
    #[cfg(test)]
    pub icons: Option<&'static [&'static [u8; 8]]>,
}

impl OptionSheet {
    pub fn new(title: &'static str, options: SheetOptions, selected: usize) -> Self {
        let hover = if selected < options.len() {
            selected
        } else {
            0
        };
        Self {
            title,
            options,
            #[cfg(test)]
            icons: None,
            selected,
            hover,
            show_radio: false,
            is_pressed: false,
        }
    }

    #[cfg(test)]
    pub fn new_dynamic(
        title: &'static str,
        labels: &'static [ShortString],
        selected: usize,
    ) -> Self {
        let hover = if selected < labels.len() { selected } else { 0 };
        Self {
            title,
            options: SheetOptions::Dynamic(labels),
            icons: None,
            selected,
            hover,
            show_radio: false,
            is_pressed: false,
        }
    }

    pub const fn with_radio(mut self, show_radio: bool) -> Self {
        self.show_radio = show_radio;
        self
    }

    #[cfg(test)]
    pub const fn with_icons(mut self, icons: &'static [&'static [u8; 8]]) -> Self {
        self.icons = Some(icons);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetResult {
    None,
    Changed,
    Confirmed(usize),
    Canceled,
}

pub struct SheetView;

impl SheetView {
    #[inline(always)]
    pub fn render(canvas: &mut Canvas, sheet: &OptionSheet) {
        let _ = Self::render_slide_up_marquee(canvas, sheet, 0, 0, false);
    }

    #[inline(always)]
    pub fn render_slide_up(canvas: &mut Canvas, sheet: &OptionSheet, y_offset: i8) {
        let _ = Self::render_slide_up_marquee(canvas, sheet, y_offset, 0, false);
    }

    fn render_row(
        canvas: &mut Canvas,
        sheet: &OptionSheet,
        idx: usize,
        row_y_scroll: i16,
        marquee_x: i16,
        marquee_enabled: bool,
    ) -> u8 {
        let has_scroll = row_y_scroll < 0;
        let row_y = if has_scroll {
            !row_y_scroll
        } else {
            row_y_scroll
        };
        let is_hovered = idx == sheet.hover;
        let is_selected = idx == sheet.selected;
        let (fill_x, fill_w) = if has_scroll { (2, 118) } else { (3, 122) };
        #[cfg(test)]
        let row_icon = sheet.icons.and_then(|arr| arr.get(idx).copied());
        #[cfg(not(test))]
        let row_icon: Option<&'static [u8; 8]> = None;
        let prefix_icon = if sheet.show_radio {
            Some(if is_selected {
                icons::RADIO_ON
            } else {
                icons::RADIO_OFF
            })
        } else if is_hovered && row_icon.is_none() {
            Some(icons::ARROW_RIGHT)
        } else {
            None
        };
        let mut spec = ListRowSpec::new(fill_x, row_y, fill_w, 11, sheet.options.get(idx));
        if is_hovered {
            spec.flags = ROW_SELECTED;
        }
        spec.prefix_icon = prefix_icon;
        spec.icon = row_icon;
        spec.marquee_x = marquee_x;
        spec.marquee_enabled = marquee_enabled;
        draw_list_row(canvas, &spec)
    }

    pub fn render_slide_up_marquee(
        canvas: &mut Canvas,
        sheet: &OptionSheet,
        y_offset: i8,
        marquee_x: i16,
        marquee_enabled: bool,
    ) -> u8 {
        let total = sheet.options.len();
        if total == 0 {
            return 0;
        }

        let sheet_h = 48u8;
        let sheet_y = draw_sheet_slide_up(canvas, sheet.title, sheet_h, y_offset);
        if sheet_y >= 64 {
            return 0;
        }

        let visible_count = 3usize;
        let (page_u16, _) = utils::math::div3_u16(sheet.hover as u16);
        let start_idx = page_u16 as usize * visible_count;
        let end_idx = (start_idx + visible_count).min(total);
        let has_scroll = total > visible_count;
        let mut hover_overflow = 0u8;

        let mut row_y = sheet_y + 11;
        for idx in start_idx..end_idx {
            if row_y < 64 && row_y + 11 > sheet_y + 10 {
                let packed = if has_scroll { !row_y } else { row_y };
                let ov = Self::render_row(
                    canvas,
                    sheet,
                    idx,
                    packed,
                    if idx == sheet.hover { marquee_x } else { 0 },
                    marquee_enabled,
                );
                if idx == sheet.hover {
                    hover_overflow = ov;
                }
            }
            row_y += 12;
        }

        if has_scroll && sheet_y + 10 < 60 {
            let track_y = (sheet_y + 10) as u8;
            let track_h = (64 - sheet_y - 12).clamp(8, 36) as u8;
            draw_scrollbar(
                canvas,
                124,
                track_y,
                track_h,
                start_idx,
                total,
                visible_count,
            );
        }
        hover_overflow
    }

    pub fn handle_event(
        repeater: &mut KeyRepeater,
        sheet: &mut OptionSheet,
        event: &Event,
    ) -> SheetResult {
        let total = sheet.options.len();
        if total == 0 {
            return SheetResult::None;
        }

        if let Event::Tick = event {
            if let Some(code) = repeater.tick()
                && let Some(down) = sheet_nav_dir(code)
            {
                let prev = sheet.hover;
                let at_edge = if down { prev + 1 >= total } else { prev == 0 };
                if !at_edge || total > 4 {
                    sheet.hover = step_hover(prev, total, down);
                    if at_edge {
                        repeater.pause(6);
                    }
                    return SheetResult::Changed;
                }
            }
            return SheetResult::None;
        }

        match event {
            Event::Key { code, state } => {
                repeater.handle_key(*code, *state);
                match state {
                    KeyState::Pressed => {
                        if let Some(down) = sheet_nav_dir(*code) {
                            sheet.hover = step_hover(sheet.hover, total, down);
                            return SheetResult::Changed;
                        }
                        match code {
                            KeyCode::Menu | KeyCode::Key5 => {
                                sheet.is_pressed = true;
                                return SheetResult::Changed;
                            }
                            KeyCode::Exit | KeyCode::Key0 => return SheetResult::Canceled,
                            _ => {}
                        }
                    }
                    KeyState::Released
                        if sheet.is_pressed && matches!(code, KeyCode::Menu | KeyCode::Key5) =>
                    {
                        sheet.is_pressed = false;
                        sheet.selected = sheet.hover;
                        return SheetResult::Confirmed(sheet.hover);
                    }
                    _ => {}
                }
            }
            Event::Encoder(delta) if *delta != 0 => {
                let forward = *delta > 0;
                for _ in 0..delta.unsigned_abs().min(8) {
                    sheet.hover = step_hover(sheet.hover, total, forward);
                }
                return SheetResult::Changed;
            }
            _ => {}
        }
        SheetResult::None
    }
}

#[inline(always)]
pub fn sheet_nav_dir(code: KeyCode) -> Option<bool> {
    match code {
        KeyCode::Key6 => Some(true),
        KeyCode::Key4 => Some(false),
        _ => match classify_nav_key(code)? {
            KeyNavStep::Primary(fwd) | KeyNavStep::Keypad(fwd) => Some(fwd),
        },
    }
}

#[inline(always)]
fn step_hover(hover: usize, total: usize, forward: bool) -> usize {
    if forward {
        if hover + 1 < total { hover + 1 } else { 0 }
    } else if hover > 0 {
        hover - 1
    } else if total > 0 {
        total - 1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_sheet_small_repeat_clamps_and_renders_icons() {
        let mut sheet = OptionSheet::new("Beep", SheetOptions::Packed("Off\0Tone\0Voice\0"), 0);
        let mut rep = KeyRepeater::new(4, 2);

        assert_eq!(
            SheetView::handle_event(
                &mut rep,
                &mut sheet,
                &Event::Key {
                    code: KeyCode::Up,
                    state: KeyState::Pressed,
                },
            ),
            SheetResult::Changed
        );
        assert_eq!(sheet.hover, 2);

        SheetView::handle_event(
            &mut rep,
            &mut sheet,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(sheet.hover, 0);

        SheetView::handle_event(
            &mut rep,
            &mut sheet,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        for _ in 0..10 {
            SheetView::handle_event(&mut rep, &mut sheet, &Event::Tick);
        }
        assert_eq!(sheet.hover, 2);

        static TEST_ICONS: [&[u8; 8]; 2] = [icons::ICON_FOLDER, icons::ICON_VFO];
        let icon_sheet = OptionSheet::new("Bind", SheetOptions::Packed("Tasks\0VFO\0"), 0)
            .with_radio(true)
            .with_icons(&TEST_ICONS);
        let mut canvas = Canvas::new();
        SheetView::render(&mut canvas, &icon_sheet);
        assert!(canvas.buffer.iter().flatten().any(|&b| b != 0));
    }
}
