//! Zero-Heap Scene Back-Stack & Virtualized List Viewport Controller (`SceneStack`, `ListCursor`).
//!
//! Provides compact `Copy` value types (`6 B` per scene frame, `6 B` per list cursor) for
//! hierarchical screen navigation with automatic selection and scroll restoration.

use kernel::event::{Event, KeyCode, KeyState};

/// Semantic interaction emitted by [`ListCursor::on_event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListAction {
    /// Event was ignored or consumed internally without changing selection/value.
    None,
    /// Selection moved to `index` (via Up/Down, rotary encoder, or `1..=9` numeric jump).
    Moved(u16),
    /// Inline value step requested on `index` (`delta = -1` for `Key4`, `+1` for `Key6`).
    StepValue { index: u16, delta: i8 },
    /// Primary activation (`Menu` pressed) on `index`.
    Activate(u16),
    /// Context options sheet requested (`Menu` held) on `index`.
    ContextOptions(u16),
    /// Navigation back (`Exit` pressed).
    Back,
}

/// Compact 6-byte virtualized list viewport & kinetic scroll state.
/// Never materializes item arrays in SRAM — supports up to `65,535` virtual rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct ListCursor {
    pub sel: u16,
    pub top: u16,
    pub scroll_px: i16,
}

impl Default for ListCursor {
    fn default() -> Self {
        Self::new()
    }
}

impl ListCursor {
    pub const fn new() -> Self {
        Self {
            sel: 0,
            top: 0,
            scroll_px: 0,
        }
    }

    /// Clamp `sel` and `top` to `count` and update target pixel scroll offset.
    #[inline(never)]
    pub fn sync_window(&mut self, count: u16, visible_rows: u8, row_h: u8, animated: bool) {
        if count == 0 {
            self.sel = 0;
            self.top = 0;
            self.scroll_px = 0;
            return;
        }
        if self.sel >= count {
            self.sel = count - 1;
        }
        let vis = (visible_rows.max(1)) as u16;
        let mut top = self.top;
        if self.sel < top {
            top = self.sel;
        } else if self.sel >= top.saturating_add(vis) {
            top = self.sel + 1 - vis;
        }
        let max_top = count.saturating_sub(vis);
        self.top = top.min(max_top);
        let target_px = (self.top as i32 * row_h as i32).min(i16::MAX as i32) as i16;
        if !animated {
            self.scroll_px = target_px;
        }
    }

    /// Advance smooth kinetic pixel scrolling toward `top * row_h`. Returns `true` while moving.
    #[inline(never)]
    pub fn tick(&mut self, row_h: u8) -> bool {
        let target_px = (self.top as i32 * row_h as i32).min(i16::MAX as i32) as i16;
        let diff = target_px - self.scroll_px;
        if diff == 0 {
            return false;
        }
        self.scroll_px += if diff > 0 {
            (diff + 1) >> 1
        } else {
            (diff - 1) >> 1
        };
        true
    }

    #[inline(never)]
    pub fn move_by(
        &mut self,
        delta: i16,
        count: u16,
        visible_rows: u8,
        row_h: u8,
        animated: bool,
    ) -> bool {
        if count == 0 || delta == 0 {
            return false;
        }
        let c = count as i32;
        let next = (self.sel as i32 + delta as i32).rem_euclid(c) as u16;
        let changed = next != self.sel;
        self.sel = next;
        self.sync_window(count, visible_rows, row_h, animated);
        changed
    }

    /// Unified list & inline-parameter event controller.
    #[inline(never)]
    pub fn on_event(
        &mut self,
        count: u16,
        visible_rows: u8,
        row_h: u8,
        nav_layout: u8,
        allow_inline_lr: bool,
        animated: bool,
        event: &Event,
    ) -> ListAction {
        match *event {
            Event::Tick => {
                let _ = self.tick(row_h);
                ListAction::None
            }
            Event::Encoder(delta) if delta != 0 => {
                if self.move_by(delta as i16, count, visible_rows, row_h, animated) {
                    ListAction::Moved(self.sel)
                } else {
                    ListAction::None
                }
            }
            Event::Key {
                code: KeyCode::Menu,
                state: KeyState::Held,
            } => {
                if count > 0 {
                    ListAction::ContextOptions(self.sel.min(count - 1))
                } else {
                    ListAction::None
                }
            }
            Event::Key {
                code,
                state: KeyState::Pressed,
            } => {
                let step_dir: i16 = if nav_layout == 0 { -1 } else { 1 };
                match code {
                    KeyCode::Up => {
                        if self.move_by(step_dir, count, visible_rows, row_h, animated) {
                            ListAction::Moved(self.sel)
                        } else {
                            ListAction::None
                        }
                    }
                    KeyCode::Down => {
                        if self.move_by(-step_dir, count, visible_rows, row_h, animated) {
                            ListAction::Moved(self.sel)
                        } else {
                            ListAction::None
                        }
                    }
                    KeyCode::Key4 if allow_inline_lr && count > 0 => ListAction::StepValue {
                        index: self.sel.min(count - 1),
                        delta: -1,
                    },
                    KeyCode::Key6 if allow_inline_lr && count > 0 => ListAction::StepValue {
                        index: self.sel.min(count - 1),
                        delta: 1,
                    },
                    KeyCode::Menu => {
                        if count > 0 {
                            ListAction::Activate(self.sel.min(count - 1))
                        } else {
                            ListAction::None
                        }
                    }
                    KeyCode::Exit => ListAction::Back,
                    _ => {
                        let raw = code as u8;
                        if (KeyCode::Key1 as u8..=KeyCode::Key9 as u8).contains(&raw) {
                            let idx = (raw - KeyCode::Key1 as u8) as u16;
                            if idx < count {
                                self.sel = idx;
                                self.sync_window(count, visible_rows, row_h, animated);
                                return ListAction::Moved(idx);
                            }
                        }
                        ListAction::None
                    }
                }
            }
            _ => ListAction::None,
        }
    }
}

/// Saved scene navigation frame (`6 B`, `Copy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct SceneFrame {
    pub scene_id: u8,
    pub aux: u8,
    pub sel: u16,
    pub top: u16,
}

impl SceneFrame {
    pub const EMPTY: Self = Self {
        scene_id: 0,
        aux: 0,
        sel: 0,
        top: 0,
    };
}

/// Bounded zero-heap scene back-stack (`2 + 6*N` bytes; default `N = 4` -> `26 B`).
/// Preserves exact selection and scroll viewport across push/pop transitions.
#[derive(Debug, Clone, Copy)]
pub struct SceneStack<const N: usize = 4> {
    frames: [SceneFrame; N],
    len: u8,
}

impl<const N: usize> Default for SceneStack<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> SceneStack<N> {
    pub const fn new() -> Self {
        Self {
            frames: [SceneFrame::EMPTY; N],
            len: 0,
        }
    }

    #[inline(always)]
    pub const fn depth(&self) -> usize {
        self.len as usize
    }

    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Push the outgoing scene's `scene_id`, `aux`, and `cursor` onto the back-stack,
    /// then reset `cursor` cleanly for the incoming child scene.
    pub fn push(&mut self, scene_id: u8, aux: u8, cursor: &mut ListCursor) -> bool {
        let d = self.len as usize;
        if d >= N {
            return false;
        }
        self.frames[d] = SceneFrame {
            scene_id,
            aux,
            sel: cursor.sel,
            top: cursor.top,
        };
        self.len += 1;
        *cursor = ListCursor::new();
        true
    }

    /// Pop the parent scene from the back-stack, restoring its exact `sel`, `top`, and `scroll_px`.
    pub fn pop(&mut self, cursor: &mut ListCursor, row_h: u8) -> Option<(u8, u8)> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        let f = self.frames[self.len as usize];
        cursor.sel = f.sel;
        cursor.top = f.top;
        cursor.scroll_px = (f.top as i32 * row_h as i32).min(i16::MAX as i32) as i16;
        Some((f.scene_id, f.aux))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_stack_preserves_cursor_and_scroll_across_push_pop() {
        let mut stack: SceneStack<4> = SceneStack::new();
        let mut cur = ListCursor::new();
        cur.sel = 7;
        cur.top = 4;
        cur.scroll_px = 52;

        assert!(stack.push(1, 0xAA, &mut cur));
        assert_eq!(cur.sel, 0);
        assert_eq!(cur.top, 0);

        cur.sel = 2;
        cur.top = 1;
        assert_eq!(stack.pop(&mut cur, 13), Some((1, 0xAA)));
        assert_eq!(cur.sel, 7);
        assert_eq!(cur.top, 4);
        assert_eq!(cur.scroll_px, 52);
    }

    #[test]
    fn list_cursor_handles_inline_step_numeric_jump_and_hold_options() {
        let mut cur = ListCursor::new();
        assert_eq!(
            cur.on_event(
                10,
                4,
                13,
                0,
                true,
                false,
                &Event::Key {
                    code: KeyCode::Key5,
                    state: KeyState::Pressed,
                },
            ),
            ListAction::Moved(4)
        );
        assert_eq!(
            cur.on_event(
                10,
                4,
                13,
                0,
                true,
                false,
                &Event::Key {
                    code: KeyCode::Key6,
                    state: KeyState::Pressed,
                },
            ),
            ListAction::StepValue { index: 4, delta: 1 }
        );
        assert_eq!(
            cur.on_event(
                10,
                4,
                13,
                0,
                true,
                false,
                &Event::Key {
                    code: KeyCode::Menu,
                    state: KeyState::Held,
                },
            ),
            ListAction::ContextOptions(4)
        );
    }
}
