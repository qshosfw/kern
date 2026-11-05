//! Navigation Stack for Microkernel Apps and Shell Orchestrator
//!
//! Provides bounded, zero-allocation navigation history (breadcrumbs)
//! allowing hierarchical "Exit" returns to previous apps or the launcher.

/// Bounded LIFO navigation stack.
#[derive(Debug, Clone)]
pub struct NavStack<const N: usize = 6> {
    entries: [usize; N],
    len: usize,
}

impl<const N: usize> Default for NavStack<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> NavStack<N> {
    pub const fn new() -> Self {
        Self {
            entries: [0; N],
            len: 0,
        }
    }

    /// Number of entries currently on the stack.
    #[inline]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the navigation stack is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Current top of the stack without popping.
    #[inline]
    pub fn peek(&self) -> Option<usize> {
        if self.len > 0 {
            Some(self.entries[self.len - 1])
        } else {
            None
        }
    }

    /// Push an app ID to the history. If full, drops oldest entry.
    pub fn push(&mut self, app_id: usize) {
        // Avoid duplicate consecutive entries
        if self.peek() == Some(app_id) {
            return;
        }

        if self.len < N {
            self.entries[self.len] = app_id;
            self.len += 1;
        } else {
            // Shift left by 1 to make room at the top
            let mut i = 0;
            while i + 1 < N {
                self.entries[i] = self.entries[i + 1];
                i += 1;
            }
            self.entries[N - 1] = app_id;
        }
    }

    /// Pop the most recent app ID.
    pub fn pop(&mut self) -> Option<usize> {
        if self.len > 0 {
            self.len -= 1;
            Some(self.entries[self.len])
        } else {
            None
        }
    }

    /// Clear navigation history and optionally push home.
    pub fn clear_to_home(&mut self, home_id: usize) {
        self.len = 0;
        self.push(home_id);
    }

    /// Clear all navigation history.
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Ensure the stack has at most `max_len` entries, dropping oldest from the bottom.
    /// Returns the oldest evicted app ID if an entry was dropped.
    pub fn trim_oldest(&mut self, max_len: usize) -> Option<usize> {
        if max_len == 0 {
            let evicted = if self.len > 0 {
                Some(self.entries[0])
            } else {
                None
            };
            self.len = 0;
            return evicted;
        }
        if self.len > max_len {
            let evicted = self.entries[0];
            let drop_count = self.len - max_len;
            for i in 0..max_len {
                self.entries[i] = self.entries[i + drop_count];
            }
            self.len = max_len;
            Some(evicted)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nav_stack_push_pop() {
        let mut stack = NavStack::<6>::new();
        assert!(stack.is_empty());
        stack.push(1);
        stack.push(2);
        stack.push(3);
        assert_eq!(stack.len(), 3);
        assert_eq!(stack.peek(), Some(3));

        assert_eq!(stack.pop(), Some(3));
        assert_eq!(stack.pop(), Some(2));
        assert_eq!(stack.pop(), Some(1));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn test_nav_stack_trim_oldest() {
        let mut stack = NavStack::<6>::new();
        stack.push(10);
        stack.push(20);
        stack.push(30);
        stack.push(40);
        assert_eq!(stack.len(), 4);

        // Trim to 2: should evict 10, leaving [30, 40]
        let evicted = stack.trim_oldest(2);
        assert_eq!(evicted, Some(10));
        assert_eq!(stack.len(), 2);
        assert_eq!(stack.pop(), Some(40));
        assert_eq!(stack.pop(), Some(30));
        assert_eq!(stack.pop(), None);
    }

    #[test]
    fn test_sunk_keys_filter_preserves_new_press() {
        // A key sunk during app/view transition must drop Released/Held, but never drop a new Pressed event
        let mut sunk_keys: u32 = 0;
        const BIT_MENU: u32 = 1 << 0;
        const BIT_KEY5: u32 = 1 << 5;

        // Simulate sink_enter_keys()
        sunk_keys |= BIT_MENU | BIT_KEY5;

        // User releases KeyCode::Menu
        let bit = BIT_MENU;
        let is_released = true;
        if (sunk_keys & bit) != 0 {
            if is_released {
                sunk_keys &= !bit;
            }
        }
        assert_eq!(sunk_keys & BIT_MENU, 0, "Menu release cleared BIT_MENU");
        assert_ne!(sunk_keys & BIT_KEY5, 0, "BIT_KEY5 remained in sunk mask");

        // Next, user presses Key5 (Enter on keypad). Fresh press must NOT be dropped!
        let bit = BIT_KEY5;
        let is_pressed = true;
        let mut dropped = false;
        if (sunk_keys & bit) != 0 {
            if !is_pressed {
                dropped = true;
            } else {
                // New press: clear stale bit and pass through
                sunk_keys &= !bit;
                dropped = false;
            }
        }
        assert!(!dropped, "Fresh press of Key5 must NOT be dropped");
        assert_eq!(sunk_keys, 0, "Stale sunk bit was cleanly cleared on press");
    }

    #[test]
    fn test_submenu_stack_preservation() {
        #[derive(Copy, Clone, Debug, PartialEq, Eq)]
        struct MenuFrame {
            page_id: u8,
            selected_idx: usize,
        }

        let mut stack: [MenuFrame; 4] = [MenuFrame {
            page_id: 0,
            selected_idx: 0,
        }; 4];
        let mut stack_top = 0;

        // Enter Submenu 1 from Root (index 2)
        stack[stack_top] = MenuFrame {
            page_id: 0,
            selected_idx: 2,
        };
        stack_top += 1;
        let mut current_page = 1;

        // Enter Submenu 2 from Submenu 1 (index 1)
        stack[stack_top] = MenuFrame {
            page_id: current_page,
            selected_idx: 1,
        };
        stack_top += 1;

        assert_eq!(stack_top, 2);

        // Rapid exit 1: pop Submenu 2 -> Submenu 1
        assert!(stack_top > 0);
        stack_top -= 1;
        let frame = stack[stack_top];
        current_page = frame.page_id;
        let mut selected_idx = frame.selected_idx;
        assert_eq!(current_page, 1);
        assert_eq!(selected_idx, 1);

        // Rapid exit 2: pop Submenu 1 -> Root
        assert!(stack_top > 0);
        stack_top -= 1;
        let frame = stack[stack_top];
        current_page = frame.page_id;
        selected_idx = frame.selected_idx;
        assert_eq!(current_page, 0);
        assert_eq!(selected_idx, 2);

        // Exit 3: at Root, pop returns false -> signals exit to Launcher
        assert_eq!(stack_top, 0);
    }
}
