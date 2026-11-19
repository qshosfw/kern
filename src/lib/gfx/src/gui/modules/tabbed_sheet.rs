//! Tabbed Sheet Container Module.
//!
//! Provides an animated, reusable multi-tab container with rounded tab headers,
//! filled/inverted active tab selection, bordered container body, and smooth kinetic transitions.

use crate::fonts::tomthumb::FONT;
use crate::gui::animation::TransitionProgress;
use crate::text;
use crate::{Canvas, Color};
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone, Copy)]
pub struct TabbedSheetConfig {
    pub x: u8,
    pub y: u8,
    pub w: u8,
    pub h: u8,
    pub tab_height: u8,
    pub max_tab_x: u8,
    pub status_bar_integrated: bool,
    pub bottom_border: bool,
}

impl TabbedSheetConfig {
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

impl Default for TabbedSheetConfig {
    fn default() -> Self {
        Self::new(0, 8, 128, 56)
    }
}

pub struct TabbedSheetState {
    pub selected_idx: usize,
    pub prev_selected_idx: usize,
    pub tab_count: usize,
    pub anim: TransitionProgress,
    pub slide_dir: i8, // -1 for sliding left, +1 for sliding right, 0 for idle
}

impl TabbedSheetState {
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
        if let Event::Key { code, state } = *event
            && state == KeyState::Pressed
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

#[inline(never)]
fn draw_tab_pill(
    canvas: &mut Canvas,
    tx: u8,
    ty: u8,
    tw: u8,
    pill_h: u8,
    text_y: i16,
    title: &str,
) {
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

pub struct TabbedSheet;

impl TabbedSheet {
    pub fn render<F>(
        canvas: &mut Canvas,
        config: &TabbedSheetConfig,
        state: &TabbedSheetState,
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

    /// Renders only the tab headers, borders, baseline, and overflow arrows without
    /// clearing the container interior.
    pub fn render_chrome(
        canvas: &mut Canvas,
        config: &TabbedSheetConfig,
        state: &TabbedSheetState,
        tab_titles: &[&str],
    ) {
        Self::render_chrome_impl(canvas, config, state, TabTitles::Slice(tab_titles));
    }

    /// Zero-alloc variant for NUL-delimited packed tab titles (e.g. C-ABI KRX modules).
    #[inline(never)]
    pub fn render_chrome_packed(
        canvas: &mut Canvas,
        config: &TabbedSheetConfig,
        selected_idx: usize,
        tab_count: usize,
        packed_titles: &[u8],
    ) {
        let state = TabbedSheetState {
            selected_idx: selected_idx.min(tab_count.saturating_sub(1)),
            prev_selected_idx: 0,
            tab_count,
            anim: crate::gui::animation::TransitionProgress::completed(4),
            slide_dir: 0,
        };
        Self::render_chrome_impl(canvas, config, &state, TabTitles::Packed(packed_titles));
    }

    #[inline(always)]
    fn render_chrome_impl(
        canvas: &mut Canvas,
        config: &TabbedSheetConfig,
        state: &TabbedSheetState,
        titles: TabTitles<'_>,
    ) {
        let x = config.x;
        let y = config.y;
        let w = config.w;
        let h = config.h;
        let th = config.tab_height;
        let max_x = config.max_tab_x.min(x.saturating_add(w));
        let count = state.tab_count.max(1).min(8);
        let sel = state.selected_idx.min(count - 1);

        let mut tab_w = [0u8; 8];
        for (i, slot) in tab_w[..count].iter_mut().enumerate() {
            let tw = text::str_width(titles.get(i), &FONT) as u8;
            *slot = (tw + 8).max(12);
        }

        // Compute visible tab window keeping `sel` in view within `max_x`
        let mut start_tab = 0usize;
        let mut end_tab;
        loop {
            let mut cur_x = x + if start_tab > 0 { 7 } else { 0 };
            let mut fit_end = start_tab;
            while fit_end < count {
                let arrow_pad = if fit_end + 1 < count { 7 } else { 0 };
                let need = tab_w[fit_end] + arrow_pad;
                if cur_x + need > max_x {
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

        let mut tab_x = [0u8; 8];
        let mut cur_tx = x + if start_tab > 0 { 7 } else { 0 };
        for i in start_tab..end_tab {
            tab_x[i] = cur_tx;
            cur_tx += tab_w[i] + 2;
        }

        let (active_tx, active_tw) = if state.is_animating()
            && state.prev_selected_idx >= start_tab
            && state.prev_selected_idx < end_tab
        {
            let prev = state.prev_selected_idx;
            let p = state.anim.progress() as i16;
            let itx =
                (tab_x[prev] as i16 + (((tab_x[sel] as i16 - tab_x[prev] as i16) * p) >> 4)) as u8;
            let itw =
                (tab_w[prev] as i16 + (((tab_w[sel] as i16 - tab_w[prev] as i16) * p) >> 4)) as u8;
            (itx, itw)
        } else {
            (tab_x[sel], tab_w[sel])
        };

        // Container borders & merged baseline
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

        // Tab headers
        for i in start_tab..end_tab {
            let is_active = i == sel;
            let (tx, tw, ty, pill_h, text_y) = if is_active {
                (active_tx, active_tw, y, th, (y + th - 3) as i16)
            } else {
                (
                    tab_x[i],
                    tab_w[i],
                    y + 1,
                    th.saturating_sub(2),
                    (y + th - 2) as i16,
                )
            };
            draw_tab_pill(canvas, tx, ty, tw, pill_h, text_y, titles.get(i));
        }

        // Overflow arrows if clipped
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

    #[test]
    fn test_tabbed_sheet_navigation() {
        let mut state = TabbedSheetState::new(3);
        assert_eq!(state.selected_idx, 0);

        state.next_tab();
        assert_eq!(state.selected_idx, 1);
        assert_eq!(state.slide_dir, 1);
        assert!(state.is_animating());

        state.next_tab();
        assert_eq!(state.selected_idx, 2);

        // Wrap around
        state.next_tab();
        assert_eq!(state.selected_idx, 0);

        // Previous tab wrap around
        state.prev_tab();
        assert_eq!(state.selected_idx, 2);
        assert_eq!(state.slide_dir, -1);
    }

    #[test]
    fn test_tabbed_sheet_key_events() {
        let mut state = TabbedSheetState::new(3);

        let evt_key6 = Event::Key {
            code: KeyCode::Key6,
            state: KeyState::Pressed,
        };
        assert!(state.handle_key_event(&evt_key6));
        assert_eq!(state.selected_idx, 1);

        let evt_key4 = Event::Key {
            code: KeyCode::Key4,
            state: KeyState::Pressed,
        };
        assert!(state.handle_key_event(&evt_key4));
        assert_eq!(state.selected_idx, 0);

        // Verify Hash, Star, KeyF, Key1 do not switch tabs
        for code in [KeyCode::Hash, KeyCode::Star, KeyCode::KeyF, KeyCode::Key1] {
            let evt = Event::Key {
                code,
                state: KeyState::Pressed,
            };
            assert!(!state.handle_key_event(&evt));
            assert_eq!(state.selected_idx, 0);
        }
    }

    #[test]
    fn test_tabbed_sheet_animation_ticking() {
        let mut state = TabbedSheetState::new(3);
        assert!(!state.is_animating());

        state.next_tab();
        assert!(state.is_animating());

        for _ in 0..20 {
            state.tick();
        }
        assert!(!state.is_animating());
    }

    #[test]
    fn test_tabbed_sheet_render() {
        let mut cv = Canvas::new();
        let config = TabbedSheetConfig::default();
        let state = TabbedSheetState::new(3);
        let titles = ["T1", "T2", "T3"];

        let mut content_called = false;
        TabbedSheet::render(
            &mut cv,
            &config,
            &state,
            &titles,
            |_cv, tab, ix, iy, iw, ih, _off| {
                content_called = true;
                assert_eq!(tab, 0);
                assert!(iw > 0);
                assert!(ih > 0);
                assert_eq!(ix, config.x + 2);
                assert_eq!(iy, config.y + config.tab_height + 2);
            },
        );

        assert!(content_called);
    }

    fn is_pixel_set(cv: &Canvas, x: u8, y: u8) -> bool {
        let buf = cv.buffer_flat();
        let page = (y >> 3) as usize;
        let bit = y & 7;
        let col = x as usize;
        (buf[page * 128 + col] & (1 << bit)) != 0
    }

    #[test]
    fn test_tabbed_sheet_merged_baseline_and_overflow() {
        let mut cv = Canvas::new();
        let config = TabbedSheetConfig::new(0, 0, 128, 54).with_status_bar_integrated(86);
        let mut state = TabbedSheetState::new(3);
        let titles = ["Tasks", "Memory", "Services"];

        // Render Tab 0
        TabbedSheet::render(
            &mut cv,
            &config,
            &state,
            &titles,
            |_cv, _, _, _, _, _, _| {},
        );

        // Baseline is at y + th = 0 + 9 = 9
        // Under active tab 0 (x = 0..~27), baseline at x=2, y=9 must be clear (merged into frame body!)
        assert!(!is_pixel_set(&cv, 2, 9));

        // After active tab (e.g. at x=65), baseline should be filled
        assert!(is_pixel_set(&cv, 65, 9));

        // Bottom border should be filled by default at row 53 (54-1)
        assert!(is_pixel_set(&cv, 64, 53));

        // When bottom_border is disabled, row 53 should remain clear
        let cfg_no_bot = TabbedSheetConfig::new(0, 0, 128, 54)
            .with_status_bar_integrated(86)
            .with_bottom_border(false);
        let mut cv_nb = Canvas::new();
        TabbedSheet::render(
            &mut cv_nb,
            &cfg_no_bot,
            &state,
            &titles,
            |_cv, _, _, _, _, _, _| {},
        );
        assert!(!is_pixel_set(&cv_nb, 64, 53));

        // Switch to Tab 2 (which exceeds 86 and requires windowing/overflow)
        state.select_tab(2);
        while state.is_animating() {
            state.tick();
        }

        let mut cv2 = Canvas::new();
        TabbedSheet::render(
            &mut cv2,
            &config,
            &state,
            &titles,
            |_cv, _, _, _, _, _, _| {},
        );

        // Left arrow should be drawn on left edge (x=0..6, y=1..8)
        // Check that at least some arrow pixels are filled
        let mut has_arrow_pixel = false;
        for py in 1..8 {
            for px in 0..6 {
                if is_pixel_set(&cv2, px, py) {
                    has_arrow_pixel = true;
                }
            }
        }
        assert!(
            has_arrow_pixel,
            "Left overflow arrow should be drawn when windowed to Tab 2"
        );
    }

    #[test]
    fn test_tabbed_sheet_render_chrome_packed() {
        let mut cv_slice = Canvas::new();
        let mut cv_packed = Canvas::new();
        let config = TabbedSheetConfig::new(0, 0, 128, 64).with_status_bar_integrated(86);
        let state = TabbedSheetState::new(4);
        let titles = ["System", "Device", "Status", "Legal"];
        TabbedSheet::render_chrome(&mut cv_slice, &config, &state, &titles);
        TabbedSheet::render_chrome_packed(
            &mut cv_packed,
            &config,
            0,
            4,
            b"System\0Device\0Status\0Legal\0",
        );
        assert_eq!(cv_slice.buffer_flat(), cv_packed.buffer_flat());

        // Also verify 4th tab renders identically without trailing NUL.
        let mut cv_slice_3 = Canvas::new();
        let mut cv_packed_3 = Canvas::new();
        let mut state_3 = TabbedSheetState::new(4);
        state_3.selected_idx = 3;
        TabbedSheet::render_chrome(&mut cv_slice_3, &config, &state_3, &titles);
        TabbedSheet::render_chrome_packed(
            &mut cv_packed_3,
            &config,
            3,
            4,
            b"System\0Device\0Status\0Legal",
        );
        assert_eq!(cv_slice_3.buffer_flat(), cv_packed_3.buffer_flat());
    }

    #[test]
    fn test_tabbed_sheet_render_chrome_preserves_content() {
        let mut cv = Canvas::new();
        // Pre-draw a pixel inside inner body (x=20, y=20)
        cv.draw_pixel(20, 20, Color::Fill);
        let config = TabbedSheetConfig::new(0, 0, 128, 64).with_status_bar_integrated(86);
        let state = TabbedSheetState::new(4);
        let titles = ["Info", "Hw", "Live", "Reg"];
        TabbedSheet::render_chrome(&mut cv, &config, &state, &titles);

        assert!(
            is_pixel_set(&cv, 20, 20),
            "Pre-drawn body pixel must remain"
        );
        assert!(is_pixel_set(&cv, 0, 9), "Left body border must be drawn");
        assert!(is_pixel_set(&cv, 127, 9), "Right body border must be drawn");
        assert!(
            is_pixel_set(&cv, 64, 63),
            "Bottom body border must be drawn"
        );
    }
}
