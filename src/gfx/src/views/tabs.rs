//! Animated Multi-Tab Container View (`TabView`, `TabConfig`, `TabState`).

use super::animation::TransitionProgress;
use crate::fonts::tomthumb::FONT;
use crate::text;
use crate::{Canvas, Color};
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone, Copy)]
pub struct TabConfig {
    pub x: u8,
    pub y: u8,
    pub w: u8,
    pub h: u8,
    pub tab_height: u8,
    pub max_tab_x: u8,
    pub status_bar_integrated: bool,
    pub bottom_border: bool,
}

impl TabConfig {
    pub const fn new(x: u8, y: u8, w: u8, h: u8) -> Self {
        Self {
            x,
            y,
            w,
            h,
            tab_height: 9,
            max_tab_x: x.saturating_add(w),
            status_bar_integrated: false,
            bottom_border: true,
        }
    }

    pub const fn with_tab_height(mut self, tab_height: u8) -> Self {
        self.tab_height = tab_height;
        self
    }

    pub const fn with_max_tab_x(mut self, max_x: u8) -> Self {
        self.max_tab_x = max_x;
        self
    }

    pub const fn with_status_bar_integrated(mut self, max_tab_x: u8) -> Self {
        self.status_bar_integrated = true;
        self.max_tab_x = max_tab_x;
        self
    }

    pub const fn with_bottom_border(mut self, enabled: bool) -> Self {
        self.bottom_border = enabled;
        self
    }
}

impl Default for TabConfig {
    fn default() -> Self {
        Self::new(0, 8, 128, 56)
    }
}

pub struct TabState {
    pub selected_idx: usize,
    pub prev_selected_idx: usize,
    pub tab_count: usize,
    pub anim: TransitionProgress,
    pub slide_dir: i8,
}

impl TabState {
    pub const fn new(tab_count: usize) -> Self {
        Self {
            selected_idx: 0,
            prev_selected_idx: 0,
            tab_count,
            anim: TransitionProgress::completed(4),
            slide_dir: 0,
        }
    }

    pub fn select_tab(&mut self, idx: usize) {
        if idx < self.tab_count && idx != self.selected_idx {
            self.slide_dir = if idx > self.selected_idx { 1 } else { -1 };
            self.prev_selected_idx = self.selected_idx;
            self.selected_idx = idx;
            self.anim.reset_in();
        }
    }

    pub fn next_tab(&mut self) {
        if self.tab_count > 1 {
            let next = (self.selected_idx + 1) % self.tab_count;
            self.prev_selected_idx = self.selected_idx;
            self.selected_idx = next;
            self.slide_dir = 1;
            self.anim.reset_in();
        }
    }

    pub fn prev_tab(&mut self) {
        if self.tab_count > 1 {
            let prev = if self.selected_idx == 0 {
                self.tab_count - 1
            } else {
                self.selected_idx - 1
            };
            self.prev_selected_idx = self.selected_idx;
            self.selected_idx = prev;
            self.slide_dir = -1;
            self.anim.reset_in();
        }
    }

    pub fn tick(&mut self) -> bool {
        if !self.anim.is_complete() {
            self.anim.tick();
            if self.anim.is_complete() {
                self.slide_dir = 0;
            }
            true
        } else {
            false
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        !self.anim.is_complete()
    }

    pub fn handle_key_event(&mut self, event: &Event) -> bool {
        if let Event::Encoder(delta) = *event
            && delta != 0
            && self.tab_count > 1
        {
            if delta > 0 {
                self.next_tab();
            } else {
                self.prev_tab();
            }
            return true;
        }
        if let Event::Key {
            code,
            state: KeyState::Pressed,
        } = *event
        {
            match code {
                KeyCode::Key4 => {
                    self.prev_tab();
                    return true;
                }
                KeyCode::Key6 => {
                    self.next_tab();
                    return true;
                }
                _ => {}
            }
        }
        false
    }
}

#[derive(Clone, Copy)]
enum TabTitles<'a> {
    Slice(&'a [&'a str]),
    Packed(&'a [u8]),
}

impl<'a> TabTitles<'a> {
    fn get(&self, idx: usize) -> &'a str {
        match *self {
            TabTitles::Slice(s) => s.get(idx).copied().unwrap_or(""),
            TabTitles::Packed(p) => utils::format::nth_cstr(p, idx as u8),
        }
    }
}

fn draw_tab_pill(canvas: &mut Canvas, rect: [u8; 4], text_y: i16, title: &str) {
    let [tx, ty, tw, pill_h] = rect;
    canvas.draw_horizontal_line(tx + 1, ty, tw.saturating_sub(2), Color::Fill);
    canvas.draw_pixel(tx, ty, Color::Clear);
    canvas.draw_pixel(tx + tw - 1, ty, Color::Clear);
    canvas.draw_vertical_line(tx, ty + 1, pill_h, Color::Fill);
    canvas.draw_vertical_line(tx + tw - 1, ty + 1, pill_h, Color::Fill);
    text::draw_str_max_x(
        canvas,
        (tx + 4) as i16,
        text_y,
        title,
        &FONT,
        Color::Fill,
        (tx + tw).saturating_sub(3) as i16,
    );
}

pub struct TabView;

impl TabView {
    pub fn render<F>(
        canvas: &mut Canvas,
        config: &TabConfig,
        state: &TabState,
        tab_titles: &[&str],
        render_content: F,
    ) where
        F: FnOnce(&mut Canvas, usize, u8, u8, u8, u8, i8),
    {
        let body_y = config.y + config.tab_height;
        let body_h = config.h.saturating_sub(config.tab_height);
        canvas.fill_rect(
            config.x + 1,
            body_y + 1,
            config.w.saturating_sub(2),
            body_h.saturating_sub(2),
            Color::Clear,
        );
        let inner_h = body_h.saturating_sub(if config.bottom_border { 4 } else { 3 });
        let anim_offset = if state.is_animating() {
            (16 - state.anim.progress()) as i8 * state.slide_dir
        } else {
            0
        };
        render_content(
            canvas,
            state.selected_idx,
            config.x + 2,
            body_y + 2,
            config.w.saturating_sub(4),
            inner_h,
            anim_offset,
        );
        Self::render_chrome(canvas, config, state, tab_titles);
    }

    pub fn render_chrome(
        canvas: &mut Canvas,
        config: &TabConfig,
        state: &TabState,
        tab_titles: &[&str],
    ) {
        let anim = state
            .is_animating()
            .then_some((state.prev_selected_idx, state.anim.progress() as i16));
        Self::render_chrome_impl(
            canvas,
            config,
            state.selected_idx,
            state.tab_count,
            anim,
            TabTitles::Slice(tab_titles),
        );
    }

    pub fn render_chrome_packed(
        canvas: &mut Canvas,
        config: &TabConfig,
        selected_idx: usize,
        tab_count: usize,
        packed_titles: &[u8],
    ) {
        Self::render_chrome_impl(
            canvas,
            config,
            selected_idx,
            tab_count,
            None,
            TabTitles::Packed(packed_titles),
        );
    }

    #[inline(always)]
    fn render_chrome_impl(
        canvas: &mut Canvas,
        config: &TabConfig,
        selected_idx: usize,
        tab_count: usize,
        anim: Option<(usize, i16)>,
        titles: TabTitles<'_>,
    ) {
        let x = config.x;
        let y = config.y;
        let w = config.w;
        let h = config.h;
        let th = config.tab_height;
        let max_x = config.max_tab_x.min(x.saturating_add(w));
        let count = tab_count.clamp(1, 8);
        let sel = selected_idx.min(count - 1);

        let mut tab_w = [0u8; 8];
        for (i, slot) in tab_w[..count].iter_mut().enumerate() {
            let tw = text::str_width(titles.get(i), &FONT) as u8;
            *slot = (tw + 8).max(12);
        }

        let mut start_tab = 0usize;
        let mut end_tab;
        loop {
            let mut cur_x = x + if start_tab > 0 { 7 } else { 0 };
            let mut fit_end = start_tab;
            while fit_end < count {
                let arrow_pad = if fit_end + 1 < count { 7 } else { 0 };
                if cur_x + tab_w[fit_end] + arrow_pad > max_x {
                    break;
                }
                cur_x += tab_w[fit_end] + 2;
                fit_end += 1;
            }
            end_tab = fit_end.max(start_tab + 1).min(count);
            if sel >= end_tab && start_tab < sel {
                start_tab += 1;
            } else {
                break;
            }
        }

        let base_tx = x + if start_tab > 0 { 7 } else { 0 };
        let mut active_tx = base_tx;
        let mut active_tw = tab_w[sel];
        let mut prev_tx = base_tx;
        let mut cur_tx = base_tx;
        for i in start_tab..end_tab {
            if i == sel {
                active_tx = cur_tx;
            }
            if let Some((prev, _)) = anim
                && i == prev
            {
                prev_tx = cur_tx;
            }
            cur_tx += tab_w[i] + 2;
        }

        if let Some((prev, p)) = anim
            && prev >= start_tab
            && prev < end_tab
        {
            active_tx = (prev_tx as i16 + (((active_tx as i16 - prev_tx as i16) * p) >> 4)) as u8;
            active_tw =
                (tab_w[prev] as i16 + (((active_tw as i16 - tab_w[prev] as i16) * p) >> 4)) as u8;
        }

        let body_y = y + th;
        let body_h = h.saturating_sub(th);
        canvas.draw_vertical_line(x, body_y, body_h, Color::Fill);
        canvas.draw_vertical_line(x + w - 1, body_y, body_h, Color::Fill);
        if config.bottom_border {
            canvas.draw_horizontal_line(x + 1, y + h - 1, w.saturating_sub(2), Color::Fill);
            canvas.draw_pixel(x, y + h - 1, Color::Clear);
            canvas.draw_pixel(x + w - 1, y + h - 1, Color::Clear);
        }

        if active_tx > x {
            canvas.draw_horizontal_line(x, body_y, active_tx - x, Color::Fill);
        }
        canvas.draw_horizontal_line(active_tx, body_y, active_tw, Color::Clear);
        let right_start = active_tx + active_tw;
        if right_start < x + w {
            canvas.draw_horizontal_line(right_start, body_y, (x + w) - right_start, Color::Fill);
        }

        let mut pill_tx = base_tx;
        for i in start_tab..end_tab {
            let (rect, text_y) = if i == sel {
                ([active_tx, y, active_tw, th], (y + th - 3) as i16)
            } else {
                (
                    [pill_tx, y + 1, tab_w[i], th.saturating_sub(2)],
                    (y + th - 2) as i16,
                )
            };
            draw_tab_pill(canvas, rect, text_y, titles.get(i));
            pill_tx += tab_w[i] + 2;
        }

        if start_tab > 0 {
            crate::icons::draw_icon_8(
                canvas,
                x as i16,
                (y + 1) as i16,
                crate::icons::ARROW_LEFT,
                Color::Fill,
            );
        }
        if end_tab < count {
            crate::icons::draw_icon_8(
                canvas,
                (cur_tx as i16) - 1,
                (y + 1) as i16,
                crate::icons::ARROW_RIGHT,
                Color::Fill,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_pixel_set(cv: &Canvas, x: u8, y: u8) -> bool {
        let buf = cv.buffer_flat();
        (buf[(y >> 3) as usize * 128 + x as usize] & (1 << (y & 7))) != 0
    }

    #[test]
    fn tab_view_navigation_merged_baseline_and_packed_chrome() {
        let mut state = TabState::new(3);
        state.next_tab();
        assert_eq!(state.selected_idx, 1);
        state.prev_tab();
        assert_eq!(state.selected_idx, 0);

        let evt_key6 = Event::Key {
            code: KeyCode::Key6,
            state: KeyState::Pressed,
        };
        assert!(state.handle_key_event(&evt_key6));
        assert_eq!(state.selected_idx, 1);
        state.select_tab(0);
        while state.is_animating() {
            state.tick();
        }

        let mut cv = Canvas::new();
        let config = TabConfig::new(0, 0, 128, 54).with_status_bar_integrated(86);
        let titles = ["Tasks", "Memory", "Services"];
        TabView::render(
            &mut cv,
            &config,
            &state,
            &titles,
            |_cv, _, _, _, _, _, _| {},
        );
        assert!(!is_pixel_set(&cv, 2, 9));
        assert!(is_pixel_set(&cv, 65, 9));
        assert!(is_pixel_set(&cv, 64, 53));

        let mut cv_slice = Canvas::new();
        let mut cv_packed = Canvas::new();
        let cfg4 = TabConfig::new(0, 0, 128, 64).with_status_bar_integrated(86);
        let st4 = TabState::new(4);
        TabView::render_chrome(
            &mut cv_slice,
            &cfg4,
            &st4,
            &["System", "Device", "Status", "Legal"],
        );
        TabView::render_chrome_packed(
            &mut cv_packed,
            &cfg4,
            0,
            4,
            b"System\0Device\0Status\0Legal\0",
        );
        assert_eq!(cv_slice.buffer_flat(), cv_packed.buffer_flat());
    }
}
