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

#[repr(C)]
pub struct KeypadLockManager {
    // ── Hot 1-byte scalars (offset 0..10: 1-insn ldrb/strb) ─────────
    pub is_locked: bool,
    pub is_f_active: bool,
    held_confirmed: bool,
    pub can_sleep: bool,
    pub sleep_requested: bool,
    pub force_close_requested: bool,
    toast: LockToast,
    is_long_press_triggered: bool,
    held_code: Option<KeyCode>,

    // ── 2-byte scalars (offset 10..24: 1-insn ldrh/strh) ────────────
    pub f_key_remaining_ms: u16,
    hold_elapsed_ms: u16,
    hold_target_ms: u16,
    hold_start_threshold_ms: u16,
    progress_slope_q16: u16,
    toast_remaining_ms: u16,

    // ── 4-byte timing counters ──────────────────────────────────────
    last_tick_ms: u32,
    inactivity_ms: u32,
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
            progress_slope_q16: 0,
            is_long_press_triggered: false,
            last_tick_ms: 0,
            inactivity_ms: 0,
            toast: LockToast::None,
            toast_remaining_ms: 0,
            held_confirmed: false,
            can_sleep: false,
            sleep_requested: false,
            force_close_requested: false,
        }
    }

    #[inline(always)]
    fn is_hold_active(&self) -> bool {
        let is_sleep_hold = self.held_code == Some(KeyCode::Exit);
        self.held_code.is_some()
            && self.held_confirmed
            && (!self.is_long_press_triggered || is_sleep_hold)
            && self.hold_elapsed_ms >= self.hold_start_threshold_ms
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.is_hold_active() || self.toast_remaining_ms > 0
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

    #[inline(always)]
    pub fn reset_hold_tracking(&mut self) {
        self.held_code = None;
        self.hold_elapsed_ms = 0;
        self.held_confirmed = false;
        self.is_long_press_triggered = false;
    }

    #[inline(always)]
    fn start_hold_tracking(&mut self, code: KeyCode) {
        self.held_code = Some(code);
        self.hold_elapsed_ms = 0;
        self.held_confirmed = false;
        self.is_long_press_triggered = false;
        self.last_tick_ms = py32_hal::delay::millis();
    }

    #[inline(never)]
    fn begin_hold(&mut self, code: KeyCode, start_ms: u16, profile: u8) {
        const PROFILES: [(u16, u16); 4] = [(1000, 6553), (500, 13107), (850, 7710), (650, 10082)];
        let (vis_dur, slope_q16) = PROFILES[(profile & 3) as usize];
        self.start_hold_tracking(code);
        self.hold_start_threshold_ms = start_ms;
        self.hold_target_ms = start_ms + vis_dur;
        self.progress_slope_q16 = slope_q16;
    }

    /// Process system events, hold-to-lock/unlock gestures, sticky F-key timeouts, and auto-lock.
    /// Returns `true` if the event was consumed/intercepted by the lock manager.
    #[inline(never)]
    pub fn handle_event(
        &mut self,
        event: &Event,
        base_hold_delay_ms: u16,
        f_timeout_ms: u32,
        auto_lock_ms: u32,
        can_sleep: bool,
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
                if self.hold_elapsed_ms >= base_hold_delay_ms.saturating_add(150)
                    && !self.held_confirmed
                {
                    self.reset_hold_tracking();
                } else if self.held_confirmed
                    && self.hold_elapsed_ms >= self.hold_target_ms
                    && !self.is_long_press_triggered
                {
                    self.is_long_press_triggered = true;
                    if code == KeyCode::Exit {
                        if can_sleep {
                            self.sleep_requested = true;
                        } else {
                            self.force_close_requested = true;
                        }
                    } else {
                        self.toggle_lock();
                    }
                    // Note: do NOT reset hold tracking here. The key is still physically held!
                    // Tracking will be reset on KeyState::Released, ensuring release does not
                    // re-trigger sticky F or other tap actions.
                    return true;
                }

                // Safety watchdog: auto-clear if hold orphaned (>4s)
                if self.hold_elapsed_ms > 4000 {
                    self.reset_hold_tracking();
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
                    self.reset_hold_tracking();
                }
            }

            // Handle Exit key (Hold-to-Sleep in Launcher, Hold-to-Force-Close in apps)
            if *code == KeyCode::Exit {
                self.can_sleep = can_sleep;
                match state {
                    KeyState::Pressed => {
                        let start_threshold = base_hold_delay_ms.saturating_add(80).clamp(450, 550);
                        self.begin_hold(KeyCode::Exit, start_threshold, !can_sleep as u8);
                        // Do NOT consume Pressed event immediately: apps/navigation still see Pressed if short tap
                    }
                    KeyState::Held => {
                        self.held_confirmed = true;
                        // Suppress key repeat while hold-to-sleep/close is tracking
                        if self.held_code == Some(KeyCode::Exit) {
                            return true;
                        }
                    }
                    KeyState::Released => {
                        let was_long_press = self.is_long_press_triggered;
                        let had_started_hold = self.held_confirmed
                            && self.hold_elapsed_ms >= self.hold_start_threshold_ms;
                        self.reset_hold_tracking();

                        // If user held past threshold or triggered action, consume release so app doesn't act
                        if was_long_press || had_started_hold {
                            return true;
                        }
                    }
                }
            } else if *code == KeyCode::Hash || *code == KeyCode::KeyF {
                match state {
                    KeyState::Pressed => {
                        // Deliberate hold threshold before showing hold dialog & progress bar (~550ms).
                        // Normal key taps (< threshold) toggle functions with strictly ZERO UI popups or progress bars.
                        let start_threshold =
                            base_hold_delay_ms.saturating_add(150).clamp(550, 750);
                        self.begin_hold(*code, start_threshold, 2 + (!self.is_locked as u8));

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
                        self.reset_hold_tracking();

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
                self.reset_hold_tracking();
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
        let is_sleep_hold = self.held_code == Some(KeyCode::Exit);
        let show_hold = self.is_hold_active();

        if !show_hold && self.toast == LockToast::None {
            return;
        }

        let box_w = 108u8;
        let box_h = 30u8;
        let (bx, by) = gfx::widgets::dialog::draw_modal_box(canvas, box_w, box_h);
        let center_x = (bx as i16) + 54;

        let (icon, text_str, y_off) = if show_hold {
            if is_sleep_hold {
                let msg = if self.is_long_press_triggered
                    || self.hold_elapsed_ms >= self.hold_target_ms
                {
                    "ENTERING SLEEP"
                } else {
                    "HOLD FOR SLEEP"
                };
                (gfx::icons::Icon::Moon, msg, 5i16)
            } else if self.is_locked {
                (gfx::icons::Icon::Lock, "HOLD TO UNLOCK", 5i16)
            } else {
                (gfx::icons::Icon::Unlock, "HOLD TO LOCK", 5i16)
            }
        } else {
            match self.toast {
                LockToast::Unlocked => (gfx::icons::Icon::Unlock, "KEYPAD UNLOCKED", 11i16),
                LockToast::Locked => (gfx::icons::Icon::Lock, "KEYPAD LOCKED", 11i16),
                _ => (gfx::icons::Icon::Lock, "KEYPAD LOCKED", 5i16),
            }
        };

        let total_w = 12 + text::str_width(text_str, &TOMTHUMB);
        let start_x = center_x - (total_w >> 1);
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

        if show_hold {
            let bar_w = 92u8;
            let bar_x = center_x - ((bar_w as i16) >> 1);
            let pct = if self.is_long_press_triggered || self.hold_elapsed_ms >= self.hold_target_ms
            {
                100
            } else {
                let visible_elapsed = self
                    .hold_elapsed_ms
                    .saturating_sub(self.hold_start_threshold_ms);
                ((visible_elapsed as u32 * self.progress_slope_q16 as u32) >> 16).min(100) as u8
            };
            gfx::widgets::draw_progress_bar(canvas, bar_x, (by + 18) as i16, bar_w, 6, pct, true);
        } else if self.toast == LockToast::PromptUnlock {
            draw_unlock_hint(canvas, center_x, (by + 18) as i16);
        }
    }
}

/// Render the bottom hint line: "HOLD [#] / [F] TO UNLOCK" with authentic inverted badges centered at center_x
fn draw_unlock_hint(canvas: &mut Canvas, center_x: i16, base_y: i16) {
    // Total width: HOLD (15) + 3 + [#] (7) + 2 + / (3) + 2 + [F] (7) + 3 + TO UNLOCK (35) = 77px
    let start_x = center_x - 38;

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
        assert!(mgr.handle_event(&pressed, 400, 3000, 0, false));
        assert!(!mgr.is_animating()); // Crucial: must NOT animate immediately

        // Simulate 200ms normal keypress duration
        mgr.hold_elapsed_ms = 200;
        assert!(!mgr.is_animating()); // Still below 550ms threshold

        // Release F key
        let released = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Released,
        };
        assert!(mgr.handle_event(&released, 400, 3000, 0, false));
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
        mgr.handle_event(&pressed, 400, 3000, 0, false);

        let held = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Held,
        };
        mgr.handle_event(&held, 400, 3000, 0, false);

        // Before threshold
        mgr.hold_elapsed_ms = 400;
        assert!(!mgr.is_animating());

        // Past threshold, progress bar appears
        mgr.hold_elapsed_ms = 600;
        assert!(mgr.is_animating());

        // Hold completed past target
        mgr.hold_elapsed_ms = 1300;
        let tick = Event::Tick;
        mgr.handle_event(&tick, 400, 3000, 0, false);
        assert!(mgr.is_locked);
    }

    #[test]
    fn test_long_press_exit_requests_sleep() {
        let mut mgr = KeypadLockManager::new();
        let pressed = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        mgr.handle_event(&pressed, 400, 3000, 0, true);

        let held = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Held,
        };
        mgr.handle_event(&held, 400, 3000, 0, true);

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
        mgr.handle_event(&tick, 400, 3000, 0, true);
        assert!(mgr.sleep_requested);
        assert!(mgr.is_long_press_triggered);

        // Crucial: overlay must render the completed 100% dialog ("ENTERING SLEEP")
        // and NOT disappear before deep sleep is entered!
        let mut canvas = Canvas::new();
        mgr.render_overlay(&mut canvas);
        let mut non_zero = 0;
        for page in 0..8 {
            for col in 0..128 {
                if canvas.buffer[page][col] != 0 {
                    non_zero += 1;
                }
            }
        }
        assert!(
            non_zero > 0,
            "Sleep overlay dialog must be rendered on long-press Exit completion"
        );

        // Reset hold tracking must clear overlay
        mgr.reset_hold_tracking();
        let mut empty_canvas = Canvas::new();
        mgr.render_overlay(&mut empty_canvas);
        let mut empty_non_zero = 0;
        for page in 0..8 {
            for col in 0..128 {
                if empty_canvas.buffer[page][col] != 0 {
                    empty_non_zero += 1;
                }
            }
        }
        assert_eq!(
            empty_non_zero, 0,
            "Overlay must be cleared after reset_hold_tracking"
        );
    }

    #[test]
    fn test_short_press_exit_aborts_without_held_confirmation() {
        let mut mgr = KeypadLockManager::new();
        let pressed = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        mgr.handle_event(&pressed, 400, 3000, 0, true);

        // Advance to 600ms without Held event (simulating tap without Held confirmation)
        mgr.hold_elapsed_ms = 600;
        let tick = Event::Tick;
        mgr.handle_event(&tick, 400, 3000, 0, true);

        // Must have auto-aborted!
        assert_eq!(mgr.held_code, None);
        assert_eq!(mgr.hold_elapsed_ms, 0);
        assert!(!mgr.sleep_requested);
    }

    #[test]
    fn test_exit_when_cannot_sleep_tracks_force_close() {
        let mut mgr = KeypadLockManager::new();
        let pressed = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        // In app context (can_sleep = false)
        assert!(!mgr.handle_event(&pressed, 400, 3000, 0, false));
        assert_eq!(mgr.held_code, Some(KeyCode::Exit));

        let held = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Held,
        };
        assert!(mgr.handle_event(&held, 400, 3000, 0, false));
        assert!(!mgr.force_close_requested);

        // Advance ticks past hold target (~1000ms)
        let tick = Event::Tick;
        for _ in 0..60 {
            mgr.handle_event(&tick, 400, 3000, 0, false);
        }
        assert!(mgr.force_close_requested);
        assert!(!mgr.sleep_requested);
    }

    #[test]
    fn test_unlock_holding_f_does_not_activate_f_plus_until_pressed_again() {
        let mut mgr = KeypadLockManager::new();
        // 1. Start locked
        mgr.is_locked = true;
        assert!(!mgr.is_f_active);

        // 2. Press and hold F key
        let pressed = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Pressed,
        };
        mgr.handle_event(&pressed, 400, 3000, 0, false);

        let held = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Held,
        };
        mgr.handle_event(&held, 400, 3000, 0, false);

        // Advance to unlock target (~1500ms)
        mgr.hold_elapsed_ms = 1600;
        let tick = Event::Tick;
        mgr.handle_event(&tick, 400, 3000, 0, false);

        // Keypad is now unlocked!
        assert!(!mgr.is_locked);
        // Crucial: while keeping F held down after unlock, F+ mode MUST NOT be activated!
        assert!(!mgr.is_f_active);

        // Continue holding F down (ticks and held events)
        let held_more = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Held,
        };
        mgr.handle_event(&held_more, 400, 3000, 0, false);
        mgr.handle_event(&tick, 400, 3000, 0, false);
        assert!(!mgr.is_f_active);

        // Release F key (from the unlock hold gesture)
        let released = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Released,
        };
        mgr.handle_event(&released, 400, 3000, 0, false);
        // Crucial: releasing the hold must NOT activate F+ mode!
        assert!(!mgr.is_f_active);
        assert!(!mgr.is_locked);

        // 3. Only when pressed AGAIN after unlock action does F+ mode register
        mgr.handle_event(&pressed, 400, 3000, 0, false);
        mgr.hold_elapsed_ms = 150; // Short tap
        mgr.handle_event(&released, 400, 3000, 0, false);
        assert!(mgr.is_f_active);
    }
}
