//! UI Navigation & Input Key Repeater
//!
//! Provides tick-driven auto-repeat for list navigation, menu scrolling, and
//! value adjustment without requiring the low-level keyboard driver to emit synthetic repeats.

use kernel::event::{KeyCode, KeyState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyRepeater {
    active_key: u8,
    ticks: u16,
    initial_delay_ticks: u16,
    repeat_rate_ticks: u16,
}

impl KeyRepeater {
    /// Create a new repeater with specified tick thresholds.
    /// E.g. at 50ms per tick: initial_delay_ticks = 8 (400ms), repeat_rate_ticks = 2 (100ms).
    pub const fn new(initial_delay_ticks: u16, repeat_rate_ticks: u16) -> Self {
        Self {
            active_key: 0,
            ticks: 0,
            initial_delay_ticks,
            repeat_rate_ticks,
        }
    }

    /// Determines if a key is a navigation key permitted to auto-repeat.
    /// Action keys (Menu, Exit, Key5, PTT, etc.) are strictly non-repeatable.
    #[inline(always)]
    pub fn is_repeatable_key(code: KeyCode) -> bool {
        matches!(
            code,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::Key2
                | KeyCode::Key8
                | KeyCode::Key4
                | KeyCode::Key6
        )
    }

    /// Process a key transition event. Call on Key events.
    pub fn handle_key(&mut self, code: KeyCode, state: KeyState) {
        match state {
            KeyState::Pressed => {
                if Self::is_repeatable_key(code) {
                    self.active_key = (code as u8) + 1;
                    self.ticks = 0;
                } else {
                    self.active_key = 0;
                    self.ticks = 0;
                }
            }
            KeyState::Released => {
                if self.active_key == (code as u8) + 1 {
                    self.active_key = 0;
                    self.ticks = 0;
                }
            }
            _ => {}
        }
    }

    /// Advance repeater state by 1 tick (typically 50ms in firmware main loop).
    /// Returns `Some(KeyCode)` if the held key should repeat on this tick.
    pub fn tick(&mut self) -> Option<KeyCode> {
        if let Some(key) = self.active_key() {
            self.ticks = self.ticks.saturating_add(1);
            if self.ticks > self.initial_delay_ticks {
                let repeat_ticks = self.ticks - self.initial_delay_ticks;
                if self.repeat_rate_ticks > 0 && (repeat_ticks % self.repeat_rate_ticks == 0) {
                    return Some(key);
                }
            }
        }
        None
    }

    /// Retrieve the currently held key, if any
    #[inline(always)]
    pub fn active_key(&self) -> Option<KeyCode> {
        if self.active_key == 0 {
            None
        } else {
            KeyCode::from_u8(self.active_key - 1)
        }
    }

    /// Manually reset repeater state
    pub fn reset(&mut self) {
        self.active_key = 0;
        self.ticks = 0;
    }
}

impl Default for KeyRepeater {
    fn default() -> Self {
        // Default: 400ms initial delay (8 ticks @ 50ms), 100ms repeat rate (2 ticks @ 50ms)
        Self::new(8, 2)
    }
}
