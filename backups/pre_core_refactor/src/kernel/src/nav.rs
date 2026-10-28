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
}
