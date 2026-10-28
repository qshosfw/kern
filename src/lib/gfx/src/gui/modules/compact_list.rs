//! Stateful compact scrolling list module for high-density UI screens.
//!
//! Encapsulates cursor position (`selected_idx`), viewport windowing (`scroll_top`),
//! boundary checking, keyboard navigation (`Up`, `Down`, `PageUp`, `PageDown`),
//! and zero-alloc row rendering.

use crate::Canvas;
use crate::gui::animation::{KineticScroller, ListScrollMode};
use crate::widgets::compact_list::{
    CompactListConfig, draw_compact_list, draw_compact_list_smooth,
};
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CompactListState {
    pub selected_idx: usize,
    pub scroll_top: usize,
    pub scroller: KineticScroller,
    pub mode: ListScrollMode,
    pub animated: bool,
}

impl CompactListState {
    pub const fn new() -> Self {
        Self {
            selected_idx: 0,
            scroll_top: 0,
            scroller: KineticScroller::new(2),
            mode: ListScrollMode::Windowed,
            animated: true,
        }
    }

    pub const fn new_paged() -> Self {
        Self {
            selected_idx: 0,
            scroll_top: 0,
            scroller: KineticScroller::new(2),
            mode: ListScrollMode::Paged,
            animated: true,
        }
    }

    pub fn reset(&mut self) {
        self.selected_idx = 0;
        self.scroll_top = 0;
        self.scroller.jump(0);
    }

    pub fn select(&mut self, idx: usize, total_items: usize, visible_rows: usize) {
        if total_items == 0 {
            self.selected_idx = 0;
            self.scroll_top = 0;
            return;
        }
        self.selected_idx = idx.min(total_items - 1);
        self.adjust_scroll(visible_rows);
    }

    pub fn move_up(&mut self, total_items: usize) -> bool {
        if total_items == 0 || self.selected_idx == 0 {
            return false;
        }
        self.selected_idx -= 1;
        if self.selected_idx < self.scroll_top {
            self.scroll_top = self.selected_idx;
        }
        true
    }

    pub fn move_down(&mut self, total_items: usize, visible_rows: usize) -> bool {
        if total_items == 0 || self.selected_idx + 1 >= total_items {
            return false;
        }
        self.selected_idx += 1;
        self.adjust_scroll(visible_rows);
        true
    }

    pub fn page_up(&mut self, visible_rows: usize) -> bool {
        let vis = if visible_rows == 0 { 1 } else { visible_rows };
        if self.selected_idx == 0 {
            return false;
        }
        self.selected_idx = self.selected_idx.saturating_sub(vis);
        self.adjust_scroll(visible_rows);
        true
    }

    pub fn page_down(&mut self, total_items: usize, visible_rows: usize) -> bool {
        if total_items == 0 || self.selected_idx + 1 >= total_items {
            return false;
        }
        let vis = if visible_rows == 0 { 1 } else { visible_rows };
        self.selected_idx = (self.selected_idx + vis).min(total_items - 1);
        self.adjust_scroll(visible_rows);
        true
    }

    pub fn adjust_scroll(&mut self, visible_rows: usize) {
        let vis = if visible_rows == 0 { 1 } else { visible_rows };
        match self.mode {
            ListScrollMode::Windowed => {
                if self.selected_idx < self.scroll_top {
                    self.scroll_top = self.selected_idx;
                } else if self.selected_idx >= self.scroll_top + vis {
                    self.scroll_top = self.selected_idx + 1 - vis;
                }
            }
            ListScrollMode::Paged => {
                self.scroll_top = (self.selected_idx / vis) * vis;
            }
            ListScrollMode::Thirds => {
                let margin = (vis / 3).max(1);
                if self.selected_idx < self.scroll_top + margin {
                    self.scroll_top = self.selected_idx.saturating_sub(margin);
                } else if self.selected_idx + margin >= self.scroll_top + vis {
                    self.scroll_top = (self.selected_idx + margin + 1).saturating_sub(vis);
                }
            }
        }
    }

    /// Handles standard keypad navigation (`Up`, `Down`).
    /// Returns true if the selection changed.
    pub fn handle_key_event(
        &mut self,
        event: &Event,
        total_items: usize,
        visible_rows: usize,
    ) -> bool {
        if let Event::Key {
            code,
            state: KeyState::Pressed,
        } = *event
        {
            match code {
                KeyCode::Up => {
                    let changed = self.move_up(total_items);
                    if changed && self.mode == ListScrollMode::Paged {
                        self.adjust_scroll(visible_rows);
                    }
                    changed
                }
                KeyCode::Down => self.move_down(total_items, visible_rows),
                _ => false,
            }
        } else {
            false
        }
    }

    /// Synchronize kinetic scroller target with current scroll_top and row height.
    ///
    /// If scroll_top did not change (e.g. moving cursor within the visible page,
    /// or when total_items <= visible_rows), the target remains identical to current
    /// and NO animation ticks will occur.
    pub fn sync_scroller(&mut self, row_height: u8) {
        let target = (self.scroll_top as i16) * (row_height as i16);
        if self.animated {
            self.scroller.set_target(target);
        } else {
            self.scroller.jump(target);
        }
    }

    /// Advance kinetic animation frame. Returns true if pixel scroll position changed.
    pub fn tick(&mut self) -> bool {
        self.scroller.tick()
    }

    /// Returns true if currently animating a page or window scroll transition.
    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.scroller.is_animating()
    }

    /// Handles keypad navigation and synchronizes kinetic scroller only when
    /// selection or scroll_top changes.
    pub fn handle_key_event_animated(
        &mut self,
        event: &Event,
        total_items: usize,
        visible_rows: usize,
        row_height: u8,
    ) -> bool {
        if self.handle_key_event(event, total_items, visible_rows) {
            self.sync_scroller(row_height);
            true
        } else {
            false
        }
    }

    /// Render using the compact list widget (static windowing)
    pub fn render<F>(
        &self,
        canvas: &mut Canvas,
        config: &CompactListConfig,
        total_items: usize,
        render_row: F,
    ) where
        F: FnMut(&mut Canvas, usize, i16, i16, u8, u8, bool),
    {
        draw_compact_list(
            canvas,
            config,
            total_items,
            self.selected_idx,
            self.scroll_top,
            render_row,
        );
    }

    /// Render using the smooth compact list widget with pixel-level vertical scrolling.
    /// When static within a page, renders at exact integer row alignment without overhead.
    pub fn render_smooth<F>(
        &self,
        canvas: &mut Canvas,
        config: &CompactListConfig,
        total_items: usize,
        render_row: F,
    ) where
        F: FnMut(&mut Canvas, usize, i16, i16, u8, u8, bool),
    {
        draw_compact_list_smooth(
            canvas,
            config,
            total_items,
            self.selected_idx,
            self.scroller.current(),
            render_row,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compact_list_empty() {
        let mut state = CompactListState::new();
        assert_eq!(state.selected_idx, 0);
        assert_eq!(state.scroll_top, 0);

        assert!(!state.move_up(0));
        assert!(!state.move_down(0, 4));
        assert!(!state.page_up(4));
        assert!(!state.page_down(0, 4));

        state.select(10, 0, 4);
        assert_eq!(state.selected_idx, 0);
        assert_eq!(state.scroll_top, 0);
    }

    #[test]
    fn test_compact_list_navigation() {
        let mut state = CompactListState::new();
        let total = 10;
        let visible = 3;

        // Move down within viewport
        assert!(state.move_down(total, visible));
        assert_eq!(state.selected_idx, 1);
        assert_eq!(state.scroll_top, 0);

        assert!(state.move_down(total, visible));
        assert_eq!(state.selected_idx, 2);
        assert_eq!(state.scroll_top, 0);

        // Move down pushes viewport
        assert!(state.move_down(total, visible));
        assert_eq!(state.selected_idx, 3);
        assert_eq!(state.scroll_top, 1);

        // Move up within viewport
        assert!(state.move_up(total));
        assert_eq!(state.selected_idx, 2);
        assert_eq!(state.scroll_top, 1);

        // Move up pushes viewport back
        assert!(state.move_up(total));
        assert_eq!(state.selected_idx, 1);
        assert_eq!(state.scroll_top, 1);

        assert!(state.move_up(total));
        assert_eq!(state.selected_idx, 0);
        assert_eq!(state.scroll_top, 0);

        // Cannot move above 0
        assert!(!state.move_up(total));
        assert_eq!(state.selected_idx, 0);
    }

    #[test]
    fn test_compact_list_page_up_down() {
        let mut state = CompactListState::new();
        let total = 20;
        let visible = 4;

        assert!(state.page_down(total, visible));
        assert_eq!(state.selected_idx, 4);
        assert_eq!(state.scroll_top, 1);

        assert!(state.page_down(total, visible));
        assert_eq!(state.selected_idx, 8);
        assert_eq!(state.scroll_top, 5);

        assert!(state.page_up(visible));
        assert_eq!(state.selected_idx, 4);
        assert_eq!(state.scroll_top, 4);

        assert!(state.page_up(visible));
        assert_eq!(state.selected_idx, 0);
        assert_eq!(state.scroll_top, 0);
    }

    #[test]
    fn test_compact_list_key_events() {
        let mut state = CompactListState::new();
        let total = 5;
        let visible = 3;

        let ev_down = Event::Key {
            code: KeyCode::Down,
            state: KeyState::Pressed,
        };
        let ev_release = Event::Key {
            code: KeyCode::Down,
            state: KeyState::Released,
        };

        assert!(!state.handle_key_event(&ev_release, total, visible));
        assert!(state.handle_key_event(&ev_down, total, visible));
        assert_eq!(state.selected_idx, 1);

        let ev_up = Event::Key {
            code: KeyCode::Up,
            state: KeyState::Pressed,
        };
        assert!(state.handle_key_event(&ev_up, total, visible));
        assert_eq!(state.selected_idx, 0);
    }

    #[test]
    fn test_compact_list_no_animation_within_page_or_single_page() {
        let mut state = CompactListState::new();
        let row_height = 8u8;

        // 1. Single page scenario: total_items (3) <= visible_rows (4)
        let total = 3;
        let visible = 4;
        let ev_down = Event::Key {
            code: KeyCode::Down,
            state: KeyState::Pressed,
        };

        // Move 0 -> 1
        assert!(state.handle_key_event_animated(&ev_down, total, visible, row_height));
        assert_eq!(state.selected_idx, 1);
        assert_eq!(state.scroll_top, 0);
        assert_eq!(state.scroller.target(), 0);
        assert!(!state.is_animating(), "Must NOT animate when single page!");
        assert!(
            !state.tick(),
            "tick() must return false when no scrolling needed"
        );

        // Move 1 -> 2
        assert!(state.handle_key_event_animated(&ev_down, total, visible, row_height));
        assert_eq!(state.selected_idx, 2);
        assert_eq!(state.scroll_top, 0);
        assert!(!state.is_animating(), "Must NOT animate within same page!");

        // 2. Multi-page scenario: total_items (10) > visible_rows (4)
        let mut multi = CompactListState::new();
        let total = 10;

        // Moves within first page (0 -> 1 -> 2 -> 3)
        for i in 1..=3 {
            assert!(multi.handle_key_event_animated(&ev_down, total, visible, row_height));
            assert_eq!(multi.selected_idx, i);
            assert_eq!(multi.scroll_top, 0);
            assert_eq!(multi.scroller.target(), 0);
            assert!(
                !multi.is_animating(),
                "No animation while cursor moves inside visible page!"
            );
            assert!(!multi.tick());
        }

        // Crossing boundary 3 -> 4 in Windowed mode triggers window scroll
        assert!(multi.handle_key_event_animated(&ev_down, total, visible, row_height));
        assert_eq!(multi.selected_idx, 4);
        assert_eq!(multi.scroll_top, 1);
        assert_eq!(multi.scroller.target(), 8); // 1 row * 8px
        assert!(
            multi.is_animating(),
            "Animation MUST trigger when crossing boundary!"
        );
        assert!(multi.tick()); // Advances 3px towards 8 (with KineticScroller ease-out)
        assert_eq!(multi.scroller.current(), 3);
    }

    #[test]
    fn test_compact_list_paged_mode_boundaries() {
        let mut state = CompactListState::new_paged();
        let total = 12;
        let visible = 4;
        let row_height = 8u8;
        let ev_down = Event::Key {
            code: KeyCode::Down,
            state: KeyState::Pressed,
        };
        let ev_up = Event::Key {
            code: KeyCode::Up,
            state: KeyState::Pressed,
        };

        // Move across slots in Page 0 (0..4)
        for i in 1..=3 {
            assert!(state.handle_key_event_animated(&ev_down, total, visible, row_height));
            assert_eq!(state.selected_idx, i);
            assert_eq!(state.scroll_top, 0, "Page 0 must remain at scroll_top 0");
            assert!(!state.is_animating(), "No animation inside Page 0");
        }

        // Crossing 3 -> 4 flips to Page 1 (scroll_top becomes 4)
        assert!(state.handle_key_event_animated(&ev_down, total, visible, row_height));
        assert_eq!(state.selected_idx, 4);
        assert_eq!(state.scroll_top, 4, "Must flip to page 1 (scroll_top 4)");
        assert_eq!(state.scroller.target(), 32); // 4 * 8px = 32px
        assert!(state.is_animating());

        // Fast-forward animation
        state.scroller.jump(32);
        assert!(!state.is_animating());

        // Move across slots in Page 1 (4..8)
        for i in 5..=7 {
            assert!(state.handle_key_event_animated(&ev_down, total, visible, row_height));
            assert_eq!(state.selected_idx, i);
            assert_eq!(state.scroll_top, 4, "Page 1 must remain at scroll_top 4");
            assert!(!state.is_animating(), "No animation inside Page 1");
        }

        // Moving back 7 -> 6 -> 5 -> 4
        for i in (4..=6).rev() {
            assert!(state.handle_key_event_animated(&ev_up, total, visible, row_height));
            assert_eq!(state.selected_idx, i);
            assert_eq!(state.scroll_top, 4);
            assert!(!state.is_animating());
        }

        // Crossing 4 -> 3 flips back to Page 0
        assert!(state.handle_key_event_animated(&ev_up, total, visible, row_height));
        assert_eq!(state.selected_idx, 3);
        assert_eq!(state.scroll_top, 0);
        assert_eq!(state.scroller.target(), 0);
        assert!(state.is_animating());
    }
}
