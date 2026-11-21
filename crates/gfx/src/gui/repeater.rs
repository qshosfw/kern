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
    turbo_threshold_ticks: u16,
}

impl KeyRepeater {
    /// Create a new repeater with specified tick thresholds and default turbo acceleration.
    /// E.g. at 50ms per tick: initial_delay_ticks = 8 (400ms), repeat_rate_ticks = 2 (100ms).
    pub const fn new(initial_delay_ticks: u16, repeat_rate_ticks: u16) -> Self {
        Self::new_with_turbo(initial_delay_ticks, repeat_rate_ticks, 8)
    }

    /// Create a repeater with explicit turbo acceleration threshold.
    /// After holding past `turbo_threshold_ticks` beyond initial delay, the repeat rate
    /// accelerates to every single tick (50ms) for high-speed scrolling.
    pub const fn new_with_turbo(
        initial_delay_ticks: u16,
        repeat_rate_ticks: u16,
        turbo_threshold_ticks: u16,
    ) -> Self {
        Self {
            active_key: 0,
            ticks: 0,
            initial_delay_ticks,
            repeat_rate_ticks,
            turbo_threshold_ticks,
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
            KeyState::Released if self.active_key == (code as u8) + 1 => {
                self.active_key = 0;
                self.ticks = 0;
            }
            _ => {}
        }
    }

    /// Advance repeater state by 1 tick (typically 50ms in firmware main loop).
    /// Returns `Some(KeyCode)` if the held key should repeat on this tick.
    ///
    /// Implements adaptive repeat acceleration:
    /// 1. Precision rate (`repeat_rate_ticks`) for the first few items so the user
    ///    can stop accurately on item +1, +2, etc.
    /// 2. Turbo rate (every tick) once held past `turbo_threshold_ticks` for fast
    ///    scrolling through long menus and tables.
    pub fn tick(&mut self) -> Option<KeyCode> {
        if let Some(key) = self.active_key() {
            self.ticks = self.ticks.saturating_add(1);
            if self.ticks > self.initial_delay_ticks {
                let repeat_ticks = self.ticks - self.initial_delay_ticks;
                let is_fire = if self.turbo_threshold_ticks > 0
                    && repeat_ticks > self.turbo_threshold_ticks
                {
                    true
                } else {
                    let rate = self.repeat_rate_ticks.max(1);
                    match rate {
                        1 => true,
                        2 => (repeat_ticks & 1) == 0,
                        3 => utils::math::div3_u16(repeat_ticks).1 == 0,
                        4 => (repeat_ticks & 3) == 0,
                        8 => (repeat_ticks & 7) == 0,
                        _ => {
                            let mut rem = repeat_ticks;
                            while rem >= rate {
                                rem -= rate;
                            }
                            rem == 0
                        }
                    }
                };
                if is_fire {
                    return Some(key);
                }
            }
        }
        None
    }

    /// Returns true if currently in accelerated turbo repeat mode
    #[inline(always)]
    pub fn is_turbo(&self) -> bool {
        self.turbo_threshold_ticks > 0
            && self.ticks > self.initial_delay_ticks + self.turbo_threshold_ticks
    }

    /// Total repeat ticks elapsed since initial delay threshold
    #[inline(always)]
    pub fn repeat_count(&self) -> u16 {
        self.ticks.saturating_sub(self.initial_delay_ticks)
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

    /// Insert a pause/dwell delay into the auto-repeat stream.
    /// Used when wrapping around list boundaries to prevent runaway continuous cycling on key hold.
    pub fn pause(&mut self, pause_ticks: u16) {
        self.ticks = self.initial_delay_ticks.saturating_sub(pause_ticks);
    }

    /// Create a repeater without turbo acceleration (remains at steady repeat rate).
    pub const fn new_no_turbo(initial_delay_ticks: u16, repeat_rate_ticks: u16) -> Self {
        Self::new_with_turbo(initial_delay_ticks, repeat_rate_ticks, 0)
    }

    /// Enable or disable turbo rate acceleration on long hold.
    pub fn set_turbo_enabled(&mut self, enabled: bool) {
        self.turbo_threshold_ticks = if enabled { 8 } else { 0 };
    }
}

impl Default for KeyRepeater {
    fn default() -> Self {
        // Default: 400ms initial delay (8 ticks @ 50ms), 100ms repeat rate (2 ticks @ 50ms)
        Self::new(8, 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_repeater_hold_down_and_turbo() {
        let mut rep = KeyRepeater::new_with_turbo(4, 2, 6);

        // Press down
        rep.handle_key(KeyCode::Down, KeyState::Pressed);
        assert_eq!(rep.active_key(), Some(KeyCode::Down));

        // Ticks 1..4: waiting for initial delay
        for _ in 1..=4 {
            assert_eq!(rep.tick(), None);
            assert!(!rep.is_turbo());
        }

        // Ticks 5..10: Stage 1 precision repeat (every 2 ticks)
        assert_eq!(rep.tick(), None); // tick 5 (repeat_ticks = 1, 1 % 2 != 0)
        assert_eq!(rep.tick(), Some(KeyCode::Down)); // tick 6 (repeat_ticks = 2, 2 % 2 == 0)
        assert_eq!(rep.tick(), None); // tick 7
        assert_eq!(rep.tick(), Some(KeyCode::Down)); // tick 8
        assert_eq!(rep.tick(), None); // tick 9
        assert_eq!(rep.tick(), Some(KeyCode::Down)); // tick 10 (repeat_ticks = 6)
        assert!(!rep.is_turbo());

        // Tick 11+: Stage 2 Turbo repeat (every 1 tick!)
        assert_eq!(rep.tick(), Some(KeyCode::Down)); // tick 11 (repeat_ticks = 7 > 6)
        assert!(rep.is_turbo());
        assert_eq!(rep.tick(), Some(KeyCode::Down)); // tick 12
        assert_eq!(rep.tick(), Some(KeyCode::Down)); // tick 13

        // Release immediately stops repeat
        rep.handle_key(KeyCode::Down, KeyState::Released);
        assert_eq!(rep.active_key(), None);
        assert_eq!(rep.tick(), None);
        assert!(!rep.is_turbo());
    }

    #[test]
    fn test_non_repeatable_keys() {
        let mut rep = KeyRepeater::new(4, 2);
        rep.handle_key(KeyCode::Menu, KeyState::Pressed);
        assert_eq!(rep.active_key(), None);
        assert_eq!(rep.tick(), None);

        rep.handle_key(KeyCode::Exit, KeyState::Pressed);
        assert_eq!(rep.active_key(), None);
        assert_eq!(rep.tick(), None);
    }
}
