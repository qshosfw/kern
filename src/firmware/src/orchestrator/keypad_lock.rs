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
            hold_start_threshold_ms: 300,
            is_long_press_triggered: false,
            last_tick_ms: 0,
            inactivity_ms: 0,
            toast: LockToast::None,
            toast_remaining_ms: 0,
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

            // Active hold tracking for # or F key in real milliseconds
            if let Some(_code) = self.held_code {
                self.hold_elapsed_ms = self.hold_elapsed_ms.saturating_add(dt);

                // Check if hold duration reached threshold
                if self.hold_elapsed_ms >= self.hold_target_ms && !self.is_long_press_triggered {
                    self.is_long_press_triggered = true;
                    self.toggle_lock();
                    self.held_code = None;
                    self.hold_elapsed_ms = 0;
                    return true;
                }

                // Safety watchdog: auto-clear if hold orphaned (>3s)
                if self.hold_elapsed_ms > 3000 {
                    self.held_code = None;
                    self.hold_elapsed_ms = 0;
                }
            }
        }

        // 2. Key events handling
        if let Event::Key { code, state } = event {
            // Reset auto-lock inactivity on any physical key interaction
            self.inactivity_ms = 0;

            // Handle # and F keys (Lock / Unlock trigger & Sticky F)
            if *code == KeyCode::Hash || *code == KeyCode::KeyF {
                match state {
                    KeyState::Pressed => {
                        self.held_code = Some(*code);
                        self.hold_elapsed_ms = 0;
                        self.is_long_press_triggered = false;
                        self.last_tick_ms = py32_hal::delay::millis();

                        // Deliberate hold threshold before showing hold dialog & progress bar (~250-350ms).
                        // Normal key taps (< threshold) toggle functions with strictly ZERO UI popups or progress bars.
                        let start_threshold =
                            (base_hold_delay_ms as u32 * 3 / 4).clamp(250, 400) as u16;
                        self.hold_start_threshold_ms = start_threshold;

                        // Asymmetric hold targets:
                        // - Hold-to-Lock: faster (~800ms) before pocketing
                        // - Hold-to-Unlock: deliberate (~1300ms) against pocket bumps
                        let lock_target = (base_hold_delay_ms as u32 * 2).clamp(600, 1000) as u16;
                        let unlock_target =
                            (base_hold_delay_ms as u32 * 3).clamp(1000, 2000) as u16;
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
                        // Do not instantly toggle! Progressively tracked via hold_elapsed_ms on Tick.
                        return true;
                    }
                    KeyState::Released => {
                        let was_long_press = self.is_long_press_triggered;
                        let had_started_hold = self.hold_elapsed_ms >= self.hold_start_threshold_ms;
                        self.held_code = None;
                        self.hold_elapsed_ms = 0;
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

            let icon = if self.is_locked {
                gfx::icons::Icon::Lock
            } else {
                gfx::icons::Icon::Unlock
            };
            let text_str = if self.is_locked {
                "HOLD TO UNLOCK"
            } else {
                "HOLD TO LOCK"
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

        if self.toast == LockToast::Unlocked {
            // Unlocked confirmation: 8x8 open padlock + "KEYPAD UNLOCKED"
            let total_w = 8 + 4 + text::str_width("KEYPAD UNLOCKED", &TOMTHUMB);
            let start_x = center_x - (total_w as i16 / 2);
            let icon_y = (by + 11) as i16;
            gfx::icons::draw_icon(
                canvas,
                start_x,
                icon_y,
                gfx::icons::Icon::Unlock,
                Color::Fill,
            );
            text::draw_str(
                canvas,
                start_x + 12,
                (by + 17) as i16,
                "KEYPAD UNLOCKED",
                &TOMTHUMB,
                Color::Fill,
            );
        } else if self.toast == LockToast::Locked {
            // Locked confirmation: 8x8 closed padlock + "KEYPAD LOCKED"
            let total_w = 8 + 4 + text::str_width("KEYPAD LOCKED", &TOMTHUMB);
            let start_x = center_x - (total_w as i16 / 2);
            let icon_y = (by + 11) as i16;
            gfx::icons::draw_icon(canvas, start_x, icon_y, gfx::icons::Icon::Lock, Color::Fill);
            text::draw_str(
                canvas,
                start_x + 12,
                (by + 17) as i16,
                "KEYPAD LOCKED",
                &TOMTHUMB,
                Color::Fill,
            );
        } else {
            // PromptUnlock: 2-line layout
            // Line 1: 8x8 closed padlock + "KEYPAD LOCKED"
            let total_w1 = 8 + 4 + text::str_width("KEYPAD LOCKED", &TOMTHUMB);
            let start_x1 = center_x - (total_w1 as i16 / 2);
            let icon_y = (by + 5) as i16;
            gfx::icons::draw_icon(
                canvas,
                start_x1,
                icon_y,
                gfx::icons::Icon::Lock,
                Color::Fill,
            );
            text::draw_str(
                canvas,
                start_x1 + 12,
                (by + 11) as i16,
                "KEYPAD LOCKED",
                &TOMTHUMB,
                Color::Fill,
            );

            // Line 2 (Subtitle): Actionable unlock instruction centered horizontally using standard badges
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
