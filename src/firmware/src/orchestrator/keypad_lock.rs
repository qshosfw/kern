//! Keypad Lock & Sticky Function Key State Handler
//!
//! Provides industrial-grade keypad lock protection, sticky F-key toggles,
//! authentic inverted key badges ('#' and 'F'), padlock status symbols,
//! live hold-to-unlock/lock progress bar animation, and auto-lock timer support.

use gfx::fonts::tomthumb::FONT as TOMTHUMB;
use gfx::{Canvas, Color, text};
use kernel::event::{Event, KeyCode, KeyState};

const DEFAULT_HOLD_MS: u16 = 1200;
const TOAST_DURATION_MS: u16 = 1800;

/// Lock notification modal state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockToast {
    None,
    /// Just unlocked: displays open padlock symbol & confirmation
    Unlocked,
    /// Just locked: displays closed padlock symbol & confirmation
    Locked,
    /// Intercepted key while locked: prompts user to hold # or F to unlock
    PromptUnlock,
}

pub struct KeypadLockManager {
    pub is_locked: bool,
    pub is_f_active: bool,
    pub f_key_remaining_ms: u16,

    // Hold tracking for # and F keys
    held_code: Option<KeyCode>,
    hold_elapsed_ms: u16,
    hold_target_ms: u16,
    hold_start_threshold_ms: u16,
    is_long_press_triggered: bool,

    // Millisecond timing reference
    last_tick_ms: u32,

    // Inactivity tracking for auto-lock in real milliseconds
    inactivity_ms: u32,

    // Dialog / Toast state
    toast: LockToast,
    toast_remaining_ms: u16,

    // Flag indicating hardware KeypadScanner confirmed KeyState::Held for the active key
    held_confirmed: bool,

    // Sleep request flag (triggered when Exit key is held past target)
    pub sleep_requested: bool,
}

impl KeypadLockManager {
    pub const fn new() -> Self {
        Self {
            is_locked: false,
            is_f_active: false,
            f_key_remaining_ms: 0,
            held_code: None,
            hold_elapsed_ms: 0,
            hold_target_ms: DEFAULT_HOLD_MS,
            hold_start_threshold_ms: 550,
            is_long_press_triggered: false,
            last_tick_ms: 0,
            inactivity_ms: 0,
            toast: LockToast::None,
            toast_remaining_ms: 0,
            held_confirmed: false,
            sleep_requested: false,
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        (self.held_code.is_some() && self.hold_elapsed_ms >= self.hold_start_threshold_ms)
            || self.toast_remaining_ms > 0
    }

    pub fn toggle_lock(&mut self) {
        self.is_locked = !self.is_locked;
        if self.is_locked {
            self.is_f_active = false;
            self.f_key_remaining_ms = 0;
            self.toast = LockToast::Locked;
        } else {
            self.toast = LockToast::Unlocked;
        }
        self.toast_remaining_ms = TOAST_DURATION_MS;
    }

    /// Reset inactivity timer when user input occurs
    pub fn reset_input_tracking(&mut self) {
        self.inactivity_ms = 0;
    }

    /// Process system events, hold-to-lock/unlock gestures, sticky F-key timeouts, and auto-lock.
    /// Returns `true` if the event was consumed/intercepted by the lock manager.
    pub fn handle_event(
        &mut self,
        event: &Event,
        base_hold_delay_ms: u16,
        f_timeout_ms: u32,
        auto_lock_ms: u32,
    ) -> bool {
        // 1. Tick handling (animations, timeouts, hold tracking, auto-lock)
        if let Event::Tick = event {
            let now = py32_hal::delay::millis();
            let dt = if self.last_tick_ms == 0 {
                20
            } else {
                now.wrapping_sub(self.last_tick_ms).min(200) as u16
            };
            self.last_tick_ms = now;

            // Decay toast dialog countdown in real milliseconds
            if self.toast_remaining_ms > 0 {
                self.toast_remaining_ms = self.toast_remaining_ms.saturating_sub(dt);
                if self.toast_remaining_ms == 0 {
                    self.toast = LockToast::None;
                }
            }

            // Sticky F-key timeout in real milliseconds
            if self.is_f_active && self.f_key_remaining_ms > 0 {
                self.f_key_remaining_ms = self.f_key_remaining_ms.saturating_sub(dt);
                if self.f_key_remaining_ms == 0 {
                    self.is_f_active = false;
                }
            }

            // Auto-lock inactivity timer (when enabled and not already locked)
            if auto_lock_ms > 0 && !self.is_locked {
                self.inactivity_ms = self.inactivity_ms.saturating_add(dt as u32);
                if self.inactivity_ms >= auto_lock_ms {
                    self.toggle_lock();
                    self.inactivity_ms = 0;
                    return true;
                }
            }

            // Active hold tracking for #, F, or Exit key in real milliseconds
            if let Some(code) = self.held_code {
                self.hold_elapsed_ms = self.hold_elapsed_ms.saturating_add(dt);

                // Anti-phantom tap protection: if elapsed time exceeds base hold delay (e.g. 550ms)
                // and hardware scanner NEVER confirmed KeyState::Held, this was a short tap
                // whose release was missed or consumed by an app -> abort hold tracking!
                if self.hold_elapsed_ms >= (base_hold_delay_ms + 150) && !self.held_confirmed {
                    self.held_code = None;
                    self.hold_elapsed_ms = 0;
                    self.held_confirmed = false;
                    self.is_long_press_triggered = false;
                } else if self.hold_elapsed_ms >= self.hold_target_ms
                    && !self.is_long_press_triggered
                {
                    self.is_long_press_triggered = true;
                    if code == KeyCode::Exit {
                        self.sleep_requested = true;
                    } else {
                        self.toggle_lock();
                    }
                    self.held_code = None;
                    self.hold_elapsed_ms = 0;
                    self.held_confirmed = false;
                    return true;
                }

                // Safety watchdog: auto-clear if hold orphaned (>4s)
                if self.hold_elapsed_ms > 4000 {
                    self.held_code = None;
                    self.hold_elapsed_ms = 0;
                    self.held_confirmed = false;
                }
            }
        }

        // 2. Key events handling
        if let Event::Key { code, state } = event {
            // Reset auto-lock inactivity on any physical key interaction
            self.inactivity_ms = 0;

            // If a different key event arrives while tracking a hold, immediately abort the hold
            if let Some(held) = self.held_code {
                if *code != held {
                    self.held_code = None;
                    self.hold_elapsed_ms = 0;
                    self.held_confirmed = false;
                    self.is_long_press_triggered = false;
                }
            }

            // Handle Exit key (Hold-to-Sleep gesture)
            if *code == KeyCode::Exit {
                match state {
                    KeyState::Pressed => {
                        self.held_code = Some(KeyCode::Exit);
                        self.hold_elapsed_ms = 0;
                        self.held_confirmed = false;
                        self.is_long_press_triggered = false;
                        self.last_tick_ms = py32_hal::delay::millis();

                        // Responsive hold threshold before showing "HOLD FOR SLEEP" dialog (~480ms).
                        // Normal key taps (< 400ms) release before this threshold with strictly zero dialog popup.
                        let start_threshold =
                            (base_hold_delay_ms as u32 + 80).clamp(450, 550) as u16;
                        self.hold_start_threshold_ms = start_threshold;

                        // Total hold target to sleep: ~1000ms progress fill (~1500ms total hold time).
                        self.hold_target_ms =
                            (start_threshold as u32 + 1000).clamp(1400, 1600) as u16;
                        // Do NOT consume Pressed event immediately: apps/navigation still see Pressed if short tap
                    }
                    KeyState::Held => {
                        self.held_confirmed = true;
                        // Suppress key repeat while hold-to-sleep is tracking
                        if self.held_code == Some(KeyCode::Exit) {
                            return true;
                        }
                    }
                    KeyState::Released => {
                        let was_long_press = self.is_long_press_triggered;
                        let had_started_hold = self.hold_elapsed_ms >= self.hold_start_threshold_ms;
                        self.held_code = None;
                        self.hold_elapsed_ms = 0;
                        self.held_confirmed = false;
                        self.is_long_press_triggered = false;

                        // If user held past threshold or triggered sleep, consume release so app doesn't act
                        if was_long_press || had_started_hold {
                            return true;
                        }
                    }
                }
            } else if *code == KeyCode::Hash || *code == KeyCode::KeyF {
                match state {
                    KeyState::Pressed => {
                        self.held_code = Some(*code);
                        self.hold_elapsed_ms = 0;
                        self.held_confirmed = false;
                        self.is_long_press_triggered = false;
                        self.last_tick_ms = py32_hal::delay::millis();

                        // Deliberate hold threshold before showing hold dialog & progress bar (~550ms).
                        // Normal key taps (< threshold) toggle functions with strictly ZERO UI popups or progress bars.
                        let start_threshold =
                            (base_hold_delay_ms as u32 + 150).clamp(550, 750) as u16;
                        self.hold_start_threshold_ms = start_threshold;

                        // Asymmetric hold targets:
                        // - Hold-to-Lock: deliberate (~1200ms) before pocketing
                        // - Hold-to-Unlock: deliberate (~1500ms) against pocket bumps
                        let lock_target = (start_threshold as u32 + 650).clamp(1200, 1600) as u16;
                        let unlock_target = (start_threshold as u32 + 850).clamp(1400, 2000) as u16;
                        self.hold_target_ms = if self.is_locked {
                            unlock_target
                        } else {
                            lock_target
                        };

                        if !self.is_locked {
                            self.toast = LockToast::None;
                            self.toast_remaining_ms = 0;
                        }

                        return true;
                    }
                    KeyState::Held => {
                        self.held_confirmed = true;
                        // Do not instantly toggle! Progressively tracked via hold_elapsed_ms on Tick.
                        return true;
                    }
                    KeyState::Released => {
                        let was_long_press = self.is_long_press_triggered;
                        let had_started_hold = self.hold_elapsed_ms >= self.hold_start_threshold_ms;
                        self.held_code = None;
                        self.hold_elapsed_ms = 0;
                        self.held_confirmed = false;
                        self.is_long_press_triggered = false;

                        if self.is_locked {
                            // If released before long press triggered while locked, show unlock hint
                            if !was_long_press {
                                self.toast = LockToast::PromptUnlock;
                                self.toast_remaining_ms = TOAST_DURATION_MS;
                            }
                        } else {
                            // Short press of F (released before hold threshold) toggles sticky F
                            if !was_long_press && !had_started_hold && *code == KeyCode::KeyF {
                                self.is_f_active = !self.is_f_active;
                                self.f_key_remaining_ms = if self.is_f_active {
                                    f_timeout_ms.min(10000) as u16
                                } else {
                                    0
                                };
                            }
                        }
                        return true;
                    }
                }
            } else if *state == KeyState::Pressed {
                // Pressing any other physical key cancels hold-to-lock tracking
                self.held_code = None;
                self.hold_elapsed_ms = 0;
                self.held_confirmed = false;
            }

            // 3. When locked, intercept all non-permitted keys
            if self.is_locked {
                let is_allowed = matches!(code, KeyCode::Ptt | KeyCode::Side1 | KeyCode::Side2);
                if !is_allowed {
                    if *state == KeyState::Pressed {
                        self.toast = LockToast::PromptUnlock;
                        self.toast_remaining_ms = TOAST_DURATION_MS;
                    }
                    return true; // Strictly intercept key
                }
            }
        }

        false
    }

    /// Renders an authentic centered notification modal dialog with lock/unlock padlock symbols,
    /// authentic inverted key badges for '#' and 'F', reusing standard system modal widgets.
    pub fn render_overlay(&self, canvas: &mut Canvas) {
        // 1. Live hold-to-unlock / hold-to-lock progress bar takes precedence ONLY during deliberate long-press hold
        if self.held_code.is_some() && self.hold_elapsed_ms >= self.hold_start_threshold_ms {
            let box_w = 108u8;
            let box_h = 30u8;
            let (bx, by) = gfx::widgets::dialog::draw_modal_box(canvas, box_w, box_h);
            let center_x = (bx as i16) + (box_w as i16 / 2);

            let (icon, text_str) = if self.held_code == Some(KeyCode::Exit) {
                (gfx::icons::Icon::Moon, "HOLD FOR SLEEP")
            } else if self.is_locked {
                (gfx::icons::Icon::Lock, "HOLD TO UNLOCK")
            } else {
                (gfx::icons::Icon::Unlock, "HOLD TO LOCK")
            };

            let text_w = text::str_width(text_str, &TOMTHUMB);
            let total_w = 8 + 4 + text_w;
            let start_x = center_x - (total_w as i16 / 2);
            gfx::icons::draw_icon(canvas, start_x, (by + 5) as i16, icon, Color::Fill);
            text::draw_str(
                canvas,
                start_x + 12,
                (by + 11) as i16,
                text_str,
                &TOMTHUMB,
                Color::Fill,
            );

            let bar_w = 92u8;
            let bar_x = (center_x - (bar_w as i16) / 2) as i16;
            let visible_elapsed = self
                .hold_elapsed_ms
                .saturating_sub(self.hold_start_threshold_ms);
            let visible_target = self
                .hold_target_ms
                .saturating_sub(self.hold_start_threshold_ms)
                .max(100) as u32;
            let pct = ((visible_elapsed as u32 * 100) / visible_target).min(100) as u8;
            gfx::widgets::draw_progress_bar(canvas, bar_x, (by + 18) as i16, bar_w, 6, pct, true);
            return;
        }

        if self.toast == LockToast::None {
            return;
        }

        let box_w = 108u8;
        let box_h = 30u8;
        let (bx, by) = gfx::widgets::dialog::draw_modal_box(canvas, box_w, box_h);
        let center_x = (bx as i16) + (box_w as i16 / 2);

        let (icon, text_str, y_off) = match self.toast {
            LockToast::Unlocked => (gfx::icons::Icon::Unlock, "KEYPAD UNLOCKED", 11i16),
            LockToast::Locked => (gfx::icons::Icon::Lock, "KEYPAD LOCKED", 11i16),
            _ => (gfx::icons::Icon::Lock, "KEYPAD LOCKED", 5i16),
        };

        let total_w = 12 + text::str_width(text_str, &TOMTHUMB);
        let start_x = center_x - (total_w as i16 / 2);
        let icon_y = (by as i16) + y_off;
        gfx::icons::draw_icon(canvas, start_x, icon_y, icon, Color::Fill);
        text::draw_str(
            canvas,
            start_x + 12,
            icon_y + 6,
            text_str,
            &TOMTHUMB,
            Color::Fill,
        );

        if self.toast != LockToast::Unlocked && self.toast != LockToast::Locked {
            draw_unlock_hint(canvas, center_x, (by + 18) as i16);
        }
    }
}

/// Render the bottom hint line: "HOLD [#] / [F] TO UNLOCK" with authentic inverted badges centered at center_x
fn draw_unlock_hint(canvas: &mut Canvas, center_x: i16, base_y: i16) {
    // Total width: HOLD (15) + 3 + [#] (7) + 2 + / (3) + 2 + [F] (7) + 3 + TO UNLOCK (35) = 77px
    let total_w = 77i16;
    let start_x = center_x - (total_w / 2);

    text::draw_str(canvas, start_x, base_y + 5, "HOLD", &TOMTHUMB, Color::Fill);
    gfx::widgets::dialog::draw_key_badge(canvas, start_x + 18, base_y, "#", "");
    text::draw_str(
        canvas,
        start_x + 27,
        base_y + 5,
        "/",
        &TOMTHUMB,
        Color::Fill,
    );
    gfx::widgets::dialog::draw_key_badge(canvas, start_x + 32, base_y, "F", "");
    text::draw_str(
        canvas,
        start_x + 41,
        base_y + 5,
        "TO UNLOCK",
        &TOMTHUMB,
        Color::Fill,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_press_f_does_not_show_lock_dialog() {
        let mut mgr = KeypadLockManager::new();
        assert!(!mgr.is_locked);
        assert!(!mgr.is_f_active);
        assert!(!mgr.is_animating());

        // Press F key
        let pressed = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Pressed,
        };
        assert!(mgr.handle_event(&pressed, 400, 3000, 0));
        assert!(!mgr.is_animating()); // Crucial: must NOT animate immediately

        // Simulate 200ms normal keypress duration
        mgr.hold_elapsed_ms = 200;
        assert!(!mgr.is_animating()); // Still below 550ms threshold

        // Release F key
        let released = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Released,
        };
        assert!(mgr.handle_event(&released, 400, 3000, 0));
        assert!(!mgr.is_locked);
        assert!(mgr.is_f_active); // Cleanly activated sticky F
        assert!(!mgr.is_animating());
    }

    #[test]
    fn test_long_press_f_locks_keypad_after_threshold() {
        let mut mgr = KeypadLockManager::new();
        let pressed = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Pressed,
        };
        mgr.handle_event(&pressed, 400, 3000, 0);

        let held = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Held,
        };
        mgr.handle_event(&held, 400, 3000, 0);

        // Before threshold
        mgr.hold_elapsed_ms = 400;
        assert!(!mgr.is_animating());

        // Past threshold, progress bar appears
        mgr.hold_elapsed_ms = 600;
        assert!(mgr.is_animating());

        // Hold completed past target
        mgr.hold_elapsed_ms = 1300;
        let tick = Event::Tick;
        mgr.handle_event(&tick, 400, 3000, 0);
        assert!(mgr.is_locked);
    }

    #[test]
    fn test_long_press_exit_requests_sleep() {
        let mut mgr = KeypadLockManager::new();
        let pressed = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        mgr.handle_event(&pressed, 400, 3000, 0);

        let held = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Held,
        };
        mgr.handle_event(&held, 400, 3000, 0);

        // Before threshold
        mgr.hold_elapsed_ms = 350;
        assert!(!mgr.is_animating());

        // Past threshold, progress bar appears
        mgr.hold_elapsed_ms = 600;
        assert!(mgr.is_animating());
        assert!(!mgr.sleep_requested);

        // Hold completed past target (~1500ms)
        mgr.hold_elapsed_ms = 1550;
        let tick = Event::Tick;
        mgr.handle_event(&tick, 400, 3000, 0);
        assert!(mgr.sleep_requested);
    }

    #[test]
    fn test_short_press_exit_aborts_without_held_confirmation() {
        let mut mgr = KeypadLockManager::new();
        let pressed = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        mgr.handle_event(&pressed, 400, 3000, 0);

        // Advance to 600ms without Held event (simulating tap without Held confirmation)
        mgr.hold_elapsed_ms = 600;
        let tick = Event::Tick;
        mgr.handle_event(&tick, 400, 3000, 0);

        // Must have auto-aborted!
        assert_eq!(mgr.held_code, None);
        assert_eq!(mgr.hold_elapsed_ms, 0);
        assert!(!mgr.sleep_requested);
    }
}
