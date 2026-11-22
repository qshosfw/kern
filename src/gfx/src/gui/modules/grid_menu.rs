//! Grid Menu View Module optimized for 1.7:1 tall pixels on 128x64 display.
//! Features 2D wrap navigation, key hold acceleration, and zero allocation rendering.

use crate::fonts::tomthumb::FONT;
use crate::gui::view::View;
use crate::text;
use crate::{Canvas, Color};
use kernel::event::{Event, KeyCode, KeyState};

/// Abstract trait for any item displayed in the grid menu.
pub trait GridItem {
    fn title(&self) -> &'static str;
    fn icon(&self) -> &'static [u8; 32];
}

#[derive(Debug, Clone, Copy)]
pub struct GridMenuItem {
    pub title: &'static str,
    pub icon_bmp: &'static [u8; 32],
}

impl GridItem for GridMenuItem {
    #[inline(always)]
    fn title(&self) -> &'static str {
        self.title
    }
    #[inline(always)]
    fn icon(&self) -> &'static [u8; 32] {
        self.icon_bmp
    }
}

#[repr(C)]
pub struct GridMenuModel<T: GridItem + 'static = GridMenuItem> {
    pub nav_layout: u8,
    pub tick_counter: u8,
    pub show_numbers: bool,
    pub show_labels: bool,
    pub is_pressed: bool,
    pub selected_idx: usize,
    pub items: &'static [T],
    pub title: &'static str,
}

impl<T: GridItem + 'static> GridMenuModel<T> {
    pub const fn new(items: &'static [T], title: &'static str) -> Self {
        Self {
            items,
            selected_idx: 0,
            title,
            nav_layout: 0,
            tick_counter: 0,
            show_numbers: true,
            show_labels: true,
            is_pressed: false,
        }
    }

    pub fn selected_item(&self) -> Option<&T> {
        if self.selected_idx < self.items.len() {
            Some(&self.items[self.selected_idx])
        } else {
            None
        }
    }
}

pub struct GridMenuView {
    pub repeater: crate::gui::repeater::KeyRepeater,
}

impl GridMenuView {
    pub const fn new() -> Self {
        Self {
            repeater: crate::gui::repeater::KeyRepeater::new(8, 2),
        }
    }

    fn nav_delta(code: KeyCode, nav_layout: u8) -> Option<i8> {
        let dir = if nav_layout == 0 { -1 } else { 1 };
        match code {
            KeyCode::Up => Some(dir),
            KeyCode::Down => Some(-dir),
            KeyCode::Key2 => Some(-3),
            KeyCode::Key8 => Some(3),
            KeyCode::Key4 | KeyCode::Key1 => Some(-1),
            KeyCode::Key6 | KeyCode::Key3 => Some(1),
            _ => None,
        }
    }

    #[inline(never)]
    pub fn step_sel(idx: &mut usize, len: usize, delta: i8) {
        if len == 0 {
            return;
        }
        *idx = match delta {
            -1 => if *idx > 0 { *idx - 1 } else { len - 1 },
            1 => if *idx + 1 < len { *idx + 1 } else { 0 },
            -3 => if *idx >= 3 { *idx - 3 } else { (*idx + 3).min(len - 1) },
            _ => {
                if *idx + 3 < len {
                    *idx + 3
                } else {
                    let mut v = *idx;
                    while v >= 3 {
                        v -= 3;
                    }
                    v
                }
            }
        };
    }

    fn handle_nav_key<T: GridItem>(model: &mut GridMenuModel<T>, code: KeyCode) -> bool {
        if let Some(d) = Self::nav_delta(code, model.nav_layout) {
            Self::step_sel(&mut model.selected_idx, model.items.len(), d);
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn is_nav_key(code: KeyCode) -> bool {
        Self::nav_delta(code, 0).is_some()
    }

    pub fn on_event<T: GridItem + 'static>(
        &mut self,
        model: &mut GridMenuModel<T>,
        event: &Event,
    ) -> bool {
        <Self as View<GridMenuModel<T>>>::on_event(self, model, event)
    }
}

impl Default for GridMenuView {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: GridItem + 'static> View<GridMenuModel<T>> for GridMenuView {
    #[inline(never)]
    fn on_event(&mut self, model: &mut GridMenuModel<T>, event: &Event) -> bool {
        if model.items.is_empty() {
            return false;
        }

        if let Event::Tick = event {
            model.tick_counter = model.tick_counter.wrapping_add(1);
            return self
                .repeater
                .tick()
                .is_some_and(|code| Self::handle_nav_key(model, code));
        }

        if let Event::Encoder(delta) = event {
            if *delta == 0 {
                return false;
            }
            let step = if *delta > 0 { 1i8 } else { -1i8 };
            for _ in 0..delta.unsigned_abs().min(8) {
                Self::step_sel(&mut model.selected_idx, model.items.len(), step);
            }
            return true;
        }

        if let Event::Key { code, state } = event {
            self.repeater.handle_key(*code, *state);
            if *state == KeyState::Pressed {
                return Self::handle_nav_key(model, *code);
            } else if *state == KeyState::Released && Self::is_nav_key(*code) {
                return true; // Consume release of navigation keys to prevent leaking into global shortcuts
            }
        }
        false
    }

    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, model: &GridMenuModel<T>) {
        let total_items = model.items.len();
        if total_items == 0 {
            text::draw_str_align(
                canvas,
                64,
                32,
                "No Applications",
                &FONT,
                Color::Fill,
                text::Align::Center,
            );
            return;
        }

        // 3x2 Grid (Tiles are 40x26 pixels, physically square on 1.7:1 tall pixels)
        let items_per_page = 6;
        let page = ((model.selected_idx as u32 * 0xAAAB) >> 18) as usize;
        let start_idx = page * items_per_page;
        let end_idx = (start_idx + items_per_page).min(total_items);

        // Micro-motion animation state: gentle 1px hop at ~3-4 Hz
        let anim_hop = if (model.tick_counter >> 3) & 1 == 1 {
            1i16
        } else {
            0i16
        };

        for idx in start_idx..end_idx {
            let page_rel = idx - start_idx;
            let (col_idx, row_idx) = if page_rel >= 3 {
                (page_rel - 3, 1u8)
            } else {
                (page_rel, 0u8)
            };
            // 3 columns (40px wide, 2px margin) x 2 rows (26px tall, 2px margin)
            let tx = 2u8 + (col_idx as u8) * 42;
            let ty = 8u8 + row_idx * 28;
            let is_selected = idx == model.selected_idx;
            let item = &model.items[idx];

            let fg_color = if is_selected {
                crate::widgets::draw_selection_capsule(
                    canvas,
                    [tx, ty, 40, 26],
                    if model.is_pressed {
                        crate::widgets::CAPSULE_ROUND
                    } else {
                        crate::widgets::CAPSULE_FILL_ROUND
                    },
                )
            } else {
                Color::Fill
            };

            // Minimalist 1-touch numeric keypad index in top-left
            if model.show_numbers && page_rel < 9 {
                let badge_char = (b'1' + page_rel as u8) as char;
                text::draw_char(
                    canvas,
                    (tx + 2) as i16,
                    (ty + 6) as i16,
                    badge_char,
                    &FONT,
                    fg_color,
                );
            }

            // Icon position: Centered vertically if labels hidden (ty+5), else at ty+2 with micro-hop
            let base_icon_y = if model.show_labels { ty + 2 } else { ty + 5 };
            let icon_y = if is_selected {
                base_icon_y as i16 - anim_hop
            } else {
                base_icon_y as i16
            };

            crate::icons::draw_icon_16(canvas, (tx + 12) as i16, icon_y, item.icon(), fg_color);

            // Centered title below icon (if enabled), localized & clipped safely inside 40px tile
            if model.show_labels {
                let mut lbuf = [0u8; 24];
                let lbl = text::localize(item.title(), true, &mut lbuf);
                if text::str_width(lbl, &FONT) > 38 {
                    text::draw_str_ellipsis(
                        canvas,
                        (tx + 1) as i16,
                        (ty + 24) as i16,
                        lbl,
                        &FONT,
                        fg_color,
                        (tx + 39) as i16,
                    );
                } else {
                    text::draw_str_align(
                        canvas,
                        (tx + 20) as i16,
                        (ty + 24) as i16,
                        lbl,
                        &FONT,
                        fg_color,
                        text::Align::Center,
                    );
                }
            }
        }

        // Page Indicator Dots (only shown when multiple pages exist)
        let total_pages = (((total_items as u32 + 5) * 0xAAAB) >> 18) as usize;
        if total_pages > 1 {
            let dot_spacing = 6u8;
            let dots_total_width = (total_pages as u8 - 1) * dot_spacing;
            let dots_start_x = 64u8 - (dots_total_width >> 1);
            for p in 0..total_pages {
                let dx = dots_start_x + (p as u8 * dot_spacing);
                if p == page {
                    canvas.fill_rect(dx, 62, 3, 2, Color::Fill);
                } else {
                    canvas.draw_pixel(dx + 1, 62, Color::Fill);
                }
            }
        }
    }
}
