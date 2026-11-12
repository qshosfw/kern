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

#[inline(always)]
fn format_tab_title<'b>(title: &str, buf: &'b mut [u8; 16]) -> &'b str {
    if title.is_empty() {
        return "";
    }
    let bytes = title.as_bytes();
    let len = bytes.len().min(buf.len());
    let mut capitalize_next = true;
    for i in 0..len {
        let b = bytes[i];
        if b == b' ' || b == b'-' || b == b'_' || b == b'/' {
            buf[i] = b;
            capitalize_next = true;
        } else if capitalize_next {
            buf[i] = b.to_ascii_uppercase();
            capitalize_next = false;
        } else {
            buf[i] = b.to_ascii_lowercase();
        }
    }
    utils::format::ascii_str(&buf[..len]).unwrap_or("")
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
        let x = config.x;
        let y = config.y;
        let w = config.w;
        let h = config.h;
        let th = config.tab_height;
        let max_x = config.max_tab_x.min(x.saturating_add(w));
        let count = state.tab_count.max(1);

        let pad_left = 4u8;
        let pad_right = 4u8;
        let tab_gap = 2u8;

        // 1. Measure tabs and format titles
        let mut tab_widths = [0u8; 8];
        let mut formatted_titles: [[u8; 16]; 8] = [[0u8; 16]; 8];

        for i in 0..count.min(8) {
            let raw = tab_titles.get(i).copied().unwrap_or("");
            let formatted = format_tab_title(raw, &mut formatted_titles[i]);
            let text_w = text::str_width(formatted, &FONT) as u8;
            tab_widths[i] = (text_w + pad_left + pad_right).max(12);
        }

        // 2. Compute windowing so selected_idx is guaranteed visible within max_x
        let mut start_tab = 0usize;
        let end_tab;

        loop {
            let mut cur_x = x;
            if start_tab > 0 {
                cur_x = cur_x.saturating_add(7); // space for left arrow
            }
            let mut fit_end = start_tab;
            while fit_end < count {
                let tw = tab_widths[fit_end.min(7)];
                let need_right_arrow = (fit_end + 1) < count;
                let right_arrow_space = if need_right_arrow { 7 } else { 0 };
                if cur_x + tw + right_arrow_space <= max_x {
                    cur_x += tw + tab_gap;
                    fit_end += 1;
                } else {
                    break;
                }
            }
            if state.selected_idx >= fit_end && start_tab < state.selected_idx {
                start_tab += 1;
            } else {
                end_tab = fit_end.max(start_tab + 1).min(count);
                break;
            }
        }

        // 3. Compute tab positions (aligned on left, starting at x if start_tab == 0)
        let mut tab_x = [0u8; 8];
        let mut tab_w = [0u8; 8];
        let mut cur_tx = x;
        if start_tab > 0 {
            cur_tx += 7;
        }
        for i in start_tab..end_tab {
            let tw = tab_widths[i.min(7)];
            tab_x[i.min(7)] = cur_tx;
            tab_w[i.min(7)] = tw;
            cur_tx += tw + tab_gap;
        }

        // 4. Slide animation offset & active tab indicator position
        let anim_offset = if state.is_animating() {
            let p = state.anim.progress(); // 0..16
            let remaining = (16 - p) as i8;
            remaining * state.slide_dir
        } else {
            0
        };

        let target_tx = tab_x[state.selected_idx.min(7)];
        let target_tw = tab_w[state.selected_idx.min(7)];

        let (active_tx, active_tw) = if state.is_animating()
            && state.prev_selected_idx >= start_tab
            && state.prev_selected_idx < end_tab
        {
            let prev_tx = tab_x[state.prev_selected_idx.min(7)];
            let prev_tw = tab_w[state.prev_selected_idx.min(7)];
            let p = state.anim.progress() as i16;
            let itx = (prev_tx as i16 + ((target_tx as i16 - prev_tx as i16) * p) / 16) as u8;
            let itw = (prev_tw as i16 + ((target_tw as i16 - prev_tw as i16) * p) / 16) as u8;
            (itx, itw)
        } else {
            (target_tx, target_tw)
        };

        // 5. Container Body Geometry
        // Baseline dividing header from body is at body_y = y + th (1px lower so unfocused tabs are 1px taller with proper title padding)
        let body_y = y + th;
        let body_h = h.saturating_sub(th);

        // Clear interior of container body
        canvas.fill_rect(
            x + 1,
            body_y + 1,
            w.saturating_sub(2),
            body_h.saturating_sub(2),
            Color::Clear,
        );

        // 6. Render Child Content
        let inner_x = x + 2;
        let inner_y = body_y + 2;
        let inner_w = w.saturating_sub(4);
        let inner_h = if config.bottom_border {
            body_h.saturating_sub(4)
        } else {
            body_h.saturating_sub(3)
        };

        render_content(
            canvas,
            state.selected_idx,
            inner_x,
            inner_y,
            inner_w,
            inner_h,
            anim_offset,
        );

        // 7. Draw Container Body Borders & Merged Baseline
        // Left & Right borders
        canvas.draw_vertical_line(x, body_y, body_h, Color::Fill);
        canvas.draw_vertical_line(x + w - 1, body_y, body_h, Color::Fill);

        // Bottom border with rounded corners (if enabled)
        if config.bottom_border {
            canvas.draw_horizontal_line(x + 1, y + h - 1, w.saturating_sub(2), Color::Fill);
            canvas.draw_pixel(x, y + h - 1, Color::Clear);
            canvas.draw_pixel(x + w - 1, y + h - 1, Color::Clear);
        }

        // Baseline (top border of body):
        // Segment before active tab:
        if active_tx > x {
            canvas.draw_horizontal_line(x, body_y, active_tx - x, Color::Fill);
        }
        // Under active tab: NO LINE (Clear - merges tab interior directly into frame)
        canvas.draw_horizontal_line(active_tx, body_y, active_tw, Color::Clear);
        // Segment after active tab:
        let right_start = active_tx + active_tw;
        if right_start < x + w {
            canvas.draw_horizontal_line(right_start, body_y, (x + w) - right_start, Color::Fill);
        }

        // 8. Render Tab Headers
        let focused_text_y = (y + th - 3) as i16; // 1px below focused top border (y)
        let unfocused_text_y = (y + th - 2) as i16; // 1px below unfocused top border (y + 1), matching focused padding

        for i in start_tab..end_tab {
            let tx = tab_x[i.min(7)];
            let tw = tab_w[i.min(7)];
            let title_bytes = &formatted_titles[i.min(7)];
            let title = utils::format::ascii_str(title_bytes)
                .unwrap_or("")
                .trim_matches('\0');
            let text_x = (tx + pad_left) as i16;

            if i == state.selected_idx {
                // Active Tab: Open pill merging into body container
                canvas.draw_horizontal_line(
                    active_tx + 1,
                    y,
                    active_tw.saturating_sub(2),
                    Color::Fill,
                );
                canvas.draw_pixel(active_tx, y, Color::Clear);
                canvas.draw_pixel(active_tx + active_tw - 1, y, Color::Clear);
                canvas.draw_vertical_line(active_tx, y + 1, th, Color::Fill);
                canvas.draw_vertical_line(active_tx + active_tw - 1, y + 1, th, Color::Fill);

                text::draw_str(canvas, text_x, focused_text_y, title, &FONT, Color::Fill);
            } else {
                // Inactive Tab: 1px taller (top border at y + 1, resting on body_y = y + th)
                canvas.draw_horizontal_line(tx + 1, y + 1, tw.saturating_sub(2), Color::Fill);
                canvas.draw_pixel(tx, y + 1, Color::Clear);
                canvas.draw_pixel(tx + tw - 1, y + 1, Color::Clear);
                canvas.draw_vertical_line(tx, y + 2, th.saturating_sub(2), Color::Fill);
                canvas.draw_vertical_line(tx + tw - 1, y + 2, th.saturating_sub(2), Color::Fill);

                text::draw_str(canvas, text_x, unfocused_text_y, title, &FONT, Color::Fill);
            }
        }

        // 9. Overflow Arrow Indicators (using submenu arrows from icons.rs)
        if start_tab > 0 {
            crate::icons::draw_icon_8(
                canvas,
                x as i16,
                (y as i16) + 1,
                &crate::icons::ARROW_LEFT,
                Color::Fill,
            );
        }
        if end_tab < count {
            let arrow_x = (cur_tx as i16) - 1;
            crate::icons::draw_icon_8(
                canvas,
                arrow_x,
                (y as i16) + 1,
                &crate::icons::ARROW_RIGHT,
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
    fn test_format_tab_title_cases() {
        let mut buf = [0u8; 16];
        assert_eq!(format_tab_title("TASKS", &mut buf), "Tasks");
        assert_eq!(format_tab_title("MEMORY", &mut buf), "Memory");
        assert_eq!(format_tab_title("SERVICES", &mut buf), "Services");
        assert_eq!(format_tab_title("tasks", &mut buf), "Tasks");
        assert_eq!(format_tab_title("vfo a", &mut buf), "Vfo A");
        assert_eq!(format_tab_title("", &mut buf), "");
    }
}
