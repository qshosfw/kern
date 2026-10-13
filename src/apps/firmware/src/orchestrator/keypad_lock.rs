//! Keypad Lock & Sticky Function Key State Handler
//!
//! Provides industrial-grade keypad lock protection, sticky F-key toggles,
//! authentic inverted key badges ('#' and 'F'), padlock status symbols,
//! live hold-to-unlock/lock progress bar animation, and auto-lock timer support.

use gfx::fonts::tomthumb::FONT as TOMTHUMB;
use gfx::{Canvas, Color, text};
use kernel::event::{Event, KeyCode, KeyState};

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
    pub f_key_ticks: u32,

    // Hold tracking for # and F keys
    held_code: Option<KeyCode>,
    hold_ticks: u32,
    is_long_press_triggered: bool,

    // Inactivity tracking for auto-lock
    inactivity_ticks: u32,

    // Dialog / Toast state
    toast: LockToast,
    toast_ticks: u32,
}

impl KeypadLockManager {
    pub const fn new() -> Self {
        Self {
            is_locked: false,
            is_f_active: false,
            f_key_ticks: 0,
            held_code: None,
            hold_ticks: 0,
            is_long_press_triggered: false,
            inactivity_ticks: 0,
            toast: LockToast::None,
            toast_ticks: 0,
        }
    }

    pub fn toggle_lock(&mut self) {
        self.is_locked = !self.is_locked;
        if self.is_locked {
            self.is_f_active = false;
            self.toast = LockToast::Locked;
        } else {
            self.toast = LockToast::Unlocked;
        }
        self.toast_ticks = 30; // ~1.5s @ 50ms tick
    }

    /// Reset inactivity and active key hold tracking when an app consumes input
    pub fn reset_input_tracking(&mut self) {
        self.inactivity_ticks = 0;
        self.held_code = None;
        self.hold_ticks = 0;
    }

    /// Process system events, hold-to-lock/unlock gestures, sticky F-key timeouts, and auto-lock.
    /// Returns `true` if the event was consumed/intercepted by the lock manager.
    pub fn handle_event(
        &mut self,
        event: &Event,
        hold_threshold: u32,
        f_timeout_ticks: u32,
        auto_lock_ticks: u32,
    ) -> bool {
        // 1. Tick handling (animations, timeouts, hold tracking, auto-lock)
        if let Event::Tick = event {
            // Decay toast dialog countdown
            if self.toast_ticks > 0 {
                self.toast_ticks -= 1;
                if self.toast_ticks == 0 {
                    self.toast = LockToast::None;
                }
            }

            // Sticky F-key timeout
            if self.is_f_active && self.f_key_ticks > 0 {
                self.f_key_ticks -= 1;
                if self.f_key_ticks == 0 {
                    self.is_f_active = false;
                }
            }

            // Auto-lock inactivity timer (when enabled and not already locked)
            if auto_lock_ticks > 0 && !self.is_locked {
                self.inactivity_ticks = self.inactivity_ticks.saturating_add(1);
                if self.inactivity_ticks >= auto_lock_ticks {
                    self.toggle_lock();
                    self.inactivity_ticks = 0;
                    return true;
                }
            }

            // Active hold tracking for # or F key
            if let Some(_code) = self.held_code {
                self.hold_ticks = self.hold_ticks.saturating_add(1);

                // Check if hold duration reached threshold
                if self.hold_ticks >= hold_threshold && !self.is_long_press_triggered {
                    self.is_long_press_triggered = true;
                    self.toggle_lock();
                    self.held_code = None;
                    return true;
                }

                // Safety watchdog: auto-clear if hold orphaned (>2s)
                if self.hold_ticks > 40 {
                    self.held_code = None;
                    self.hold_ticks = 0;
                }
            }
        }

        // 2. Key events handling
        if let Event::Key { code, state } = event {
            // Reset auto-lock inactivity on any physical key interaction
            self.inactivity_ticks = 0;

            // Handle # and F keys (Lock / Unlock trigger & Sticky F)
            if *code == KeyCode::Hash || *code == KeyCode::KeyF {
                match state {
                    KeyState::Pressed => {
                        self.held_code = Some(*code);
                        self.hold_ticks = 0;
                        self.is_long_press_triggered = false;
                        if self.is_locked {
                            return true; // Intercept key pressed while locked
                        }
                    }
                    KeyState::Held => {
                        if self.held_code.is_some() && !self.is_long_press_triggered {
                            self.is_long_press_triggered = true;
                            self.toggle_lock();
                            self.held_code = None;
                            return true;
                        }
                    }
                    KeyState::Released => {
                        let was_long_press = self.is_long_press_triggered;
                        self.held_code = None;
                        self.hold_ticks = 0;
                        self.is_long_press_triggered = false;

                        if self.is_locked {
                            // If released before long press triggered while locked, show unlock hint
                            if !was_long_press {
                                self.toast = LockToast::PromptUnlock;
                                self.toast_ticks = 30; // 1.5s
                            }
                            return true;
                        } else {
                            // Short press of F toggles sticky F
                            if !was_long_press && *code == KeyCode::KeyF {
                                self.is_f_active = !self.is_f_active;
                                self.f_key_ticks =
                                    if self.is_f_active { f_timeout_ticks } else { 0 };
                            }
                        }
                    }
                }
            }

            // 3. When locked, intercept all non-permitted keys
            if self.is_locked {
                let is_allowed = matches!(code, KeyCode::Ptt | KeyCode::Side1 | KeyCode::Side2);
                if !is_allowed {
                    if *state == KeyState::Pressed {
                        self.toast = LockToast::PromptUnlock;
                        self.toast_ticks = 30;
                    }
                    return true; // Strictly intercept key
                }
            }
        }

        false
    }

    /// Renders an authentic centered notification modal dialog with lock/unlock padlock symbols,
    /// authentic inverted key badges for '#' and 'F', and rounded corners matching system modals.
    pub fn render_overlay(&self, canvas: &mut Canvas) {
        if self.toast == LockToast::None {
            return;
        }

        let box_w = 104u8;
        let box_h = 28u8;
        let (bx, by) = gfx::widgets::dialog::draw_modal_box(canvas, box_w, box_h);
        let center_x = (bx as i16) + (box_w as i16 / 2);

        if self.toast == LockToast::Unlocked {
            // Unlocked confirmation: 8x8 open padlock + "KEYPAD UNLOCKED" (58px) = 70px total
            let total_w = 70i16;
            let start_x = center_x - (total_w / 2);
            let icon_y = (by + 10) as i16;
            draw_padlock_icon(canvas, start_x, icon_y, false);
            text::draw_str(
                canvas,
                start_x + 12,
                (by + 17) as i16,
                "KEYPAD UNLOCKED",
                &TOMTHUMB,
                Color::Fill,
            );
        } else {
            // Locked / PromptUnlock: 2-line layout
            // Line 1: 8x8 closed padlock + "KEYPAD LOCKED" (50px) = 62px total
            let total_w1 = 62i16;
            let start_x1 = center_x - (total_w1 / 2);
            let icon_y = (by + 5) as i16;
            draw_padlock_icon(canvas, start_x1, icon_y, true);
            text::draw_str(
                canvas,
                start_x1 + 12,
                (by + 12) as i16,
                "KEYPAD LOCKED",
                &TOMTHUMB,
                Color::Fill,
            );

            // Line 2 (Subtitle): Actionable unlock instruction centered horizontally
            draw_unlock_hint(canvas, center_x, (by + 16) as i16);
        }
    }
}

/// Draw 8x8 Padlock Icon (closed or open shackle)
fn draw_padlock_icon(canvas: &mut Canvas, x: i16, y: i16, is_closed: bool) {
    let shackle = if is_closed { 0x24 } else { 0x20 };
    let data: [u8; 8] = [0x3C, shackle, shackle, 0x7E, 0x7E, 0x66, 0x7E, 0x00];
    gfx::draw::draw_bitmap(canvas, x, y, 8, 8, &data, Color::Fill);
}

#[inline(always)]
fn draw_mini_badge(canvas: &mut Canvas, x: i16, y: i16, ch: &str) {
    canvas.fill_rect(x as u8, y as u8, 7, 7, Color::Fill);
    text::draw_str(canvas, x + 2, y + 5, ch, &TOMTHUMB, Color::Clear);
}

/// Render the bottom hint line: "HOLD [#] / [F] TO UNLOCK" with authentic inverted badges centered at center_x
fn draw_unlock_hint(canvas: &mut Canvas, center_x: i16, base_y: i16) {
    // Total width: HOLD (16) + 3 + [#] (7) + 2 + / (3) + 2 + [F] (7) + 3 + TO UNLOCK (35) = 78px
    let total_w = 78i16;
    let start_x = center_x - (total_w / 2);

    text::draw_str(canvas, start_x, base_y + 5, "HOLD", &TOMTHUMB, Color::Fill);
    draw_mini_badge(canvas, start_x + 19, base_y, "#");
    text::draw_str(
        canvas,
        start_x + 28,
        base_y + 5,
        "/",
        &TOMTHUMB,
        Color::Fill,
    );
    draw_mini_badge(canvas, start_x + 33, base_y, "F");
    text::draw_str(
        canvas,
        start_x + 43,
        base_y + 5,
        "TO UNLOCK",
        &TOMTHUMB,
        Color::Fill,
    );
}
