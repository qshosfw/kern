//! Keypad Lock, Deep Sleep & Sticky Function Key State Handler
//!
//! Provides keypad lock protection, sticky F-key toggles, hold-to-sleep / hold-to-close,
//! and springy bottom-sheet overlays synchronized with the system menu aesthetic.

use gfx::fonts::tomthumb::FONT as TOMTHUMB;
use gfx::gui::animation::{HoldProgress, TransitionProgress};
use gfx::{Canvas, Color, icons::Icon, text};
use kernel::event::{Event, KeyCode, KeyState};

const TOAST_DURATION_MS: u16 = 1800;
const SHEET_H: u8 = 36;

/// Lock notification modal state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LockToast {
    None = 0,
    /// Just unlocked: displays open padlock symbol & confirmation
    Unlocked = 1,
    /// Just locked: displays closed padlock symbol & confirmation
    Locked = 2,
    /// Intercepted key while locked: prompts user to hold # or F to unlock
    PromptUnlock = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum HoldKind {
    Sleep = 0,
    CloseApp = 1,
    Unlock = 2,
    Lock = 3,
}

struct HoldSpec {
    icon: Icon,
    required_ms: u16,
}

utils::static_bytes!(
    static HOLD_STRINGS = b"\
Deep Sleep\0Hold EXIT for sleep\0\
Close App\0Hold EXIT to close\0\
Keypad Locked\0Hold to unlock\0\
Keypad Lock\0Hold to lock\0\
Keypad\0Keypad Unlocked\0";
);

static HOLD_SPECS: [HoldSpec; 4] = [
    HoldSpec {
        icon: Icon::Moon,
        required_ms: 1000,
    },
    HoldSpec {
        icon: Icon::Keypad,
        required_ms: 500,
    },
    HoldSpec {
        icon: Icon::Lock,
        required_ms: 850,
    },
    HoldSpec {
        icon: Icon::Unlock,
        required_ms: 650,
    },
];

#[repr(C)]
pub struct KeypadLockManager {
    // ── Hot 1-byte scalars ──────────────────────────────────────────
    pub is_locked: bool,
    pub is_f_active: bool,
    pub sleep_requested: bool,
    pub force_close_requested: bool,
    held_confirmed: bool,
    is_long_press_triggered: bool,
    hold_ui_open: bool,
    hold_kind: HoldKind,
    toast: LockToast,
    held_code: Option<KeyCode>,

    // ── Shared animation & springy hold progress ────────────────────
    hold: HoldProgress,
    sheet_anim: TransitionProgress,
    f_drawer_anim: TransitionProgress,

    // ── 2-byte scalars ──────────────────────────────────────────────
    pub f_key_remaining_ms: u16,
    delay_elapsed_ms: u16,
    hold_start_threshold_ms: u16,
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
            sleep_requested: false,
            force_close_requested: false,
            held_confirmed: false,
            is_long_press_triggered: false,
            hold_ui_open: false,
            hold_kind: HoldKind::Sleep,
            toast: LockToast::None,
            held_code: None,
            hold: HoldProgress::new(),
            sheet_anim: TransitionProgress::closed(4),
            f_drawer_anim: TransitionProgress::closed(4),
            f_key_remaining_ms: 0,
            delay_elapsed_ms: 0,
            hold_start_threshold_ms: 550,
            toast_remaining_ms: 0,
            last_tick_ms: 0,
            inactivity_ms: 0,
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.hold_ui_open
            || self.hold.is_animating()
            || !self.sheet_anim.is_complete()
            || !self.f_drawer_anim.is_complete()
    }

    #[inline(always)]
    pub fn is_overlay_active(&self) -> bool {
        self.is_animating() || self.toast != LockToast::None
    }

    #[inline(always)]
    pub fn should_show_f_drawer(&self) -> bool {
        (self.is_f_active || self.f_drawer_anim.progress() > 0)
            && !self.is_locked
            && !self.hold_ui_open
            && self.toast == LockToast::None
    }

    #[inline(always)]
    pub fn is_f_drawer_animating(&self) -> bool {
        !self.f_drawer_anim.is_complete()
    }

    #[inline(always)]
    pub fn f_drawer_y_offset(&self, height: u8) -> i8 {
        self.f_drawer_anim.slide_offset(height)
    }

    pub fn clear_f_active(&mut self) {
        self.is_f_active = false;
        self.f_key_remaining_ms = 0;
        self.f_drawer_anim.close();
    }

    pub fn snap_f_drawer(&mut self, force_close: bool) {
        if force_close || !self.is_f_active {
            self.f_drawer_anim.reset_closed();
        } else {
            self.f_drawer_anim = TransitionProgress::completed(4);
        }
    }

    fn show_toast(&mut self, toast: LockToast) {
        self.toast = toast;
        self.toast_remaining_ms = TOAST_DURATION_MS;
        self.sheet_anim.open();
    }

    pub fn toggle_lock(&mut self) {
        self.is_locked = !self.is_locked;
        if self.is_locked {
            self.is_f_active = false;
            self.f_key_remaining_ms = 0;
            self.f_drawer_anim.reset_closed();
            self.show_toast(LockToast::Locked);
        } else {
            self.show_toast(LockToast::Unlocked);
        }
    }

    /// Reset inactivity timer when user input occurs.
    pub fn reset_input_tracking(&mut self) {
        self.inactivity_ms = 0;
    }

    #[inline(never)]
    pub fn reset_hold_tracking(&mut self) {
        self.held_code = None;
        self.delay_elapsed_ms = 0;
        self.held_confirmed = false;
        self.is_long_press_triggered = false;
        self.hold_ui_open = false;
        self.hold.reset();
        self.hold.required_ms = 0;
        if self.toast == LockToast::None {
            self.sheet_anim.close();
            self.sheet_anim.jump_complete();
        }
    }

    /// Stop advancing hold and let the progress bar springily rewind to 0 before closing.
    #[inline(never)]
    fn release_active_hold(&mut self) {
        self.held_code = None;
        self.delay_elapsed_ms = 0;
        self.held_confirmed = false;
        self.is_long_press_triggered = false;
        self.hold.active = false;
    }

    #[inline(never)]
    fn clear_hold_overlay(&mut self, reset_anim: bool) {
        self.hold.reset();
        self.hold.required_ms = 0;
        self.hold_ui_open = false;
        if reset_anim {
            self.sheet_anim.reset_closed();
        }
    }

    #[inline(never)]
    fn finish_hold_release(&mut self) -> bool {
        let was_long_press = self.is_long_press_triggered;
        let had_started_hold = self.hold_ui_open;
        if had_started_hold && !was_long_press {
            self.release_active_hold();
        } else {
            self.reset_hold_tracking();
        }
        was_long_press || had_started_hold
    }

    #[inline(never)]
    fn begin_hold(&mut self, code: KeyCode, start_ms: u16, kind: HoldKind) {
        self.held_code = Some(code);
        self.delay_elapsed_ms = 0;
        self.held_confirmed = false;
        self.is_long_press_triggered = false;
        self.hold_ui_open = false;
        self.hold_kind = kind;
        self.hold.reset();
        self.hold.required_ms = HOLD_SPECS[kind as usize].required_ms;
        self.hold_start_threshold_ms = start_ms;
        if self.toast == LockToast::None {
            self.sheet_anim.close();
        }
    }

    /// Process system events, hold-to-lock/unlock/sleep gestures, sticky F-key timeouts, and auto-lock.
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
        if let Event::Tick = event {
            let dt = super::tick_dt_ms(&mut self.last_tick_ms, 200);
            let mut consumed = false;

            if self.toast_remaining_ms > 0 {
                self.toast_remaining_ms = self.toast_remaining_ms.saturating_sub(dt);
                if self.toast_remaining_ms == 0 {
                    self.sheet_anim.close();
                    consumed = true;
                }
            }

            if self.is_f_active && self.f_key_remaining_ms > 0 {
                self.f_key_remaining_ms = self.f_key_remaining_ms.saturating_sub(dt);
                if self.f_key_remaining_ms == 0 {
                    self.clear_f_active();
                    consumed = true;
                }
            }

            if auto_lock_ms > 0
                && !self.is_locked
                && self.held_code.is_none()
                && !crate::system::is_headless()
            {
                self.inactivity_ms = self.inactivity_ms.saturating_add(dt as u32);
                if self.inactivity_ms >= auto_lock_ms {
                    self.toggle_lock();
                    self.inactivity_ms = 0;
                    return true;
                }
            }

            if self.held_code.is_some() && !self.is_long_press_triggered {
                self.delay_elapsed_ms = self.delay_elapsed_ms.saturating_add(dt);

                if self.hold_kind == HoldKind::Sleep && (!can_sleep || crate::system::is_headless())
                {
                    self.release_active_hold();
                } else if !self.hold_ui_open {
                    if !self.held_confirmed
                        && self.delay_elapsed_ms >= base_hold_delay_ms.saturating_add(150)
                    {
                        self.reset_hold_tracking();
                    } else if self.held_confirmed
                        && self.delay_elapsed_ms >= self.hold_start_threshold_ms
                    {
                        self.hold_ui_open = true;
                        self.hold.active = true;
                        self.hold.elapsed_ms = 0;
                        self.toast = LockToast::None;
                        self.toast_remaining_ms = 0;
                        self.sheet_anim.open();
                    }
                }

                if self.held_code.is_some() && self.delay_elapsed_ms > 4000 {
                    self.release_active_hold();
                }
            }

            if self.hold_ui_open {
                if self.hold.advance(dt) && !self.is_long_press_triggered {
                    self.is_long_press_triggered = true;
                    if matches!(self.hold_kind, HoldKind::Sleep | HoldKind::CloseApp) {
                        if self.hold_kind == HoldKind::Sleep {
                            self.sleep_requested = true;
                        } else {
                            self.force_close_requested = true;
                        }
                        self.clear_hold_overlay(true);
                    } else {
                        self.toggle_lock();
                        self.clear_hold_overlay(false);
                    }
                    return true;
                }

                if !self.hold.active && self.hold.elapsed_ms == 0 && !self.is_long_press_triggered {
                    self.sheet_anim.close();
                    if self.sheet_anim.is_complete() {
                        self.hold_ui_open = false;
                        self.hold.required_ms = 0;
                        consumed = true;
                    }
                }
            }

            let _ = self.sheet_anim.tick();
            let _ = self.f_drawer_anim.tick();
            if self.toast != LockToast::None
                && self.toast_remaining_ms == 0
                && !self.sheet_anim.is_opening()
                && self.sheet_anim.is_complete()
            {
                self.toast = LockToast::None;
                consumed = true;
            }

            return consumed;
        }

        if matches!(event, Event::Encoder(_)) {
            self.inactivity_ms = 0;
            return false;
        }

        if let Event::Key { code, state } = event {
            self.inactivity_ms = 0;

            if let Some(held) = self.held_code {
                if *code != held {
                    self.reset_hold_tracking();
                }
            }

            let is_hash_f = *code == KeyCode::Hash || *code == KeyCode::KeyF;
            if is_hash_f || (!self.is_locked && *code == KeyCode::Exit) {
                match state {
                    KeyState::Pressed => {
                        let (threshold, kind) = if is_hash_f {
                            (
                                base_hold_delay_ms.saturating_add(150).clamp(550, 750),
                                if self.is_locked {
                                    HoldKind::Unlock
                                } else {
                                    HoldKind::Lock
                                },
                            )
                        } else {
                            if can_sleep && crate::system::is_headless() {
                                return false;
                            }
                            (
                                base_hold_delay_ms.saturating_add(80).max(450),
                                if can_sleep {
                                    HoldKind::Sleep
                                } else {
                                    HoldKind::CloseApp
                                },
                            )
                        };
                        self.begin_hold(*code, threshold, kind);
                        if is_hash_f && !self.is_locked {
                            self.toast = LockToast::None;
                            self.toast_remaining_ms = 0;
                        }
                        if is_hash_f {
                            return true;
                        }
                    }
                    KeyState::Held => {
                        self.held_confirmed = true;
                        if is_hash_f || self.held_code == Some(KeyCode::Exit) {
                            return true;
                        }
                    }
                    KeyState::Released => {
                        let cancelled_hold = self.finish_hold_release();
                        if is_hash_f {
                            if self.is_locked && !cancelled_hold {
                                self.show_toast(LockToast::PromptUnlock);
                            } else if !self.is_locked && !cancelled_hold && *code == KeyCode::KeyF {
                                self.is_f_active = !self.is_f_active;
                                if self.is_f_active {
                                    self.f_key_remaining_ms = f_timeout_ms.min(10000) as u16;
                                    self.f_drawer_anim.open();
                                } else {
                                    self.f_key_remaining_ms = 0;
                                    self.f_drawer_anim.close();
                                }
                            }
                            return true;
                        } else if cancelled_hold {
                            return true;
                        }
                    }
                }
            } else if self.is_locked {
                if !matches!(code, KeyCode::Ptt | KeyCode::Side1 | KeyCode::Side2) {
                    if *state == KeyState::Pressed {
                        self.show_toast(LockToast::PromptUnlock);
                    }
                    return true;
                }
            } else if *state == KeyState::Pressed {
                self.reset_hold_tracking();
            }
        }

        false
    }

    /// Renders a menu-consistent springy bottom-sheet overlay for lock/unlock, deep sleep,
    /// app close, and keypad lock status toasts.
    #[inline(always)]
    pub fn render_overlay(&self, canvas: &mut Canvas) {
        if self.hold_ui_open || self.toast != LockToast::None {
            self.render_overlay_active(canvas);
        }
    }

    #[inline(never)]
    fn render_overlay_active(&self, canvas: &mut Canvas) {
        let y_offset = self.sheet_anim.slide_offset(SHEET_H);
        if y_offset >= SHEET_H as i8 {
            return;
        }

        let (title, msg, icon, show_bar, show_hint) = if self.hold_ui_open {
            let k = self.hold_kind as u8;
            (
                utils::format::nth_cstr(&HOLD_STRINGS, k * 2),
                utils::format::nth_cstr(&HOLD_STRINGS, k * 2 + 1),
                HOLD_SPECS[k as usize].icon,
                true,
                false,
            )
        } else {
            let kt = utils::format::nth_cstr(&HOLD_STRINGS, 8);
            match self.toast {
                LockToast::Unlocked => (
                    kt,
                    utils::format::nth_cstr(&HOLD_STRINGS, 9),
                    Icon::Unlock,
                    false,
                    false,
                ),
                LockToast::Locked => (
                    kt,
                    utils::format::nth_cstr(&HOLD_STRINGS, 4),
                    Icon::Lock,
                    false,
                    false,
                ),
                LockToast::PromptUnlock => (
                    utils::format::nth_cstr(&HOLD_STRINGS, 4),
                    "",
                    Icon::Lock,
                    false,
                    true,
                ),
                LockToast::None => return,
            }
        };

        let sheet_y = 64i16 - SHEET_H as i16 + y_offset as i16;
        gfx::widgets::draw_sheet_slide_up(canvas, title, SHEET_H, y_offset);

        if self.sheet_anim.progress() < 8 {
            return;
        }

        if show_bar {
            gfx::icons::draw_icon(canvas, 6, sheet_y + 11, icon, Color::Fill);
            text::draw_str(canvas, 17, sheet_y + 17, msg, &TOMTHUMB, Color::Fill);
            gfx::widgets::draw_progress_bar(
                canvas,
                4,
                sheet_y + 23,
                120,
                6,
                self.hold.percent(),
                true,
            );
        } else if show_hint {
            draw_unlock_hint(canvas, 64, sheet_y + 18);
        } else {
            let total_w = 11 + text::str_width(msg, &TOMTHUMB);
            let start_x = 64 - (total_w >> 1);
            gfx::icons::draw_icon(canvas, start_x, sheet_y + 17, icon, Color::Fill);
            text::draw_str(
                canvas,
                start_x + 11,
                sheet_y + 23,
                msg,
                &TOMTHUMB,
                Color::Fill,
            );
        }
    }
}

/// Render the bottom hint line: "HOLD [#] TO UNLOCK" with authentic inverted badge centered at center_x
fn draw_unlock_hint(canvas: &mut Canvas, center_x: i16, base_y: i16) {
    let start_x = center_x - 32;

    text::draw_str(canvas, start_x, base_y + 5, "HOLD", &TOMTHUMB, Color::Fill);
    gfx::widgets::dialog::draw_key_badge(canvas, start_x + 18, base_y, "#", "");
    text::draw_str(
        canvas,
        start_x + 30,
        base_y + 5,
        "TO UNLOCK",
        &TOMTHUMB,
        Color::Fill,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tick_n(mgr: &mut KeypadLockManager, n: usize, can_sleep: bool) {
        let tick = Event::Tick;
        for _ in 0..n {
            mgr.handle_event(&tick, 400, 3000, 0, can_sleep);
        }
    }

    #[test]
    fn test_short_press_f_does_not_show_lock_dialog() {
        let mut mgr = KeypadLockManager::new();
        assert!(!mgr.is_locked);
        assert!(!mgr.is_f_active);
        assert!(!mgr.is_animating());

        let pressed = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Pressed,
        };
        assert!(mgr.handle_event(&pressed, 400, 3000, 0, false));
        assert!(!mgr.is_animating());

        // 200ms (< 550ms threshold)
        tick_n(&mut mgr, 4, false);
        assert!(!mgr.is_animating());

        let released = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Released,
        };
        assert!(mgr.handle_event(&released, 400, 3000, 0, false));
        assert!(!mgr.is_locked);
        assert!(!mgr.hold_ui_open);
        assert!(mgr.is_f_active);
        assert!(mgr.is_f_drawer_animating());
        tick_n(&mut mgr, 4, false);
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

        // 400ms (< 550ms)
        tick_n(&mut mgr, 8, false);
        assert!(!mgr.is_animating());

        // Crosses 550ms -> sheet opens
        tick_n(&mut mgr, 4, false);
        assert!(mgr.is_animating());
        assert!(mgr.hold_ui_open);
        assert!(!mgr.is_locked);

        // Complete 650ms hold -> locks keypad
        tick_n(&mut mgr, 14, false);
        assert!(mgr.is_locked);
        assert_eq!(mgr.toast, LockToast::Locked);
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

        // 350ms (< 480ms)
        tick_n(&mut mgr, 7, true);
        assert!(!mgr.is_animating());

        // Past threshold -> sheet opens
        tick_n(&mut mgr, 4, true);
        assert!(mgr.is_animating());
        assert!(mgr.hold_ui_open);
        assert!(!mgr.sleep_requested);

        let mut canvas = Canvas::new();
        mgr.render_overlay(&mut canvas);
        let non_zero: usize = canvas
            .buffer
            .iter()
            .flat_map(|row| row.iter())
            .filter(|&&b| b != 0)
            .count();
        assert!(non_zero > 0, "Sleep bottom sheet must render during hold");

        // Hold through 1000ms -> sleep requested
        tick_n(&mut mgr, 20, true);
        assert!(mgr.sleep_requested);
        assert!(mgr.is_long_press_triggered);

        // Reset hold tracking (as drain_keypad_matrix does before/after sleep) must clear overlay
        mgr.reset_hold_tracking();
        let mut empty_canvas = Canvas::new();
        mgr.render_overlay(&mut empty_canvas);
        let empty_non_zero: usize = empty_canvas
            .buffer
            .iter()
            .flat_map(|row| row.iter())
            .filter(|&&b| b != 0)
            .count();
        assert_eq!(
            empty_non_zero, 0,
            "Overlay must be cleared after reset_hold_tracking"
        );
    }

    #[test]
    fn test_early_release_exit_springs_back_and_does_not_sleep() {
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

        // Open sheet + fill ~70% of 1000ms sleep bar
        tick_n(&mut mgr, 24, true);
        assert!(mgr.hold_ui_open);
        assert!(mgr.hold.elapsed_ms > 500);
        assert!(!mgr.sleep_requested);

        // Release Exit before 100%
        let released = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Released,
        };
        assert!(mgr.handle_event(&released, 400, 3000, 0, true));
        assert!(!mgr.hold.active);
        assert!(!mgr.sleep_requested);
        assert!(mgr.hold_ui_open);

        // Progress rewinds at 3x speed (700ms / 150ms per tick = ~5 ticks), then sheet slides out
        tick_n(&mut mgr, 2, true);
        assert!(mgr.hold.elapsed_ms < 500);
        assert!(!mgr.sleep_requested);

        tick_n(&mut mgr, 10, true);
        assert!(!mgr.hold_ui_open);
        assert!(!mgr.is_animating());
        assert!(!mgr.sleep_requested);
    }

    #[test]
    fn test_locked_exit_prompts_unlock_instead_of_sleep_or_close() {
        let mut mgr = KeypadLockManager::new();
        mgr.is_locked = true;

        let pressed = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        assert!(mgr.handle_event(&pressed, 400, 3000, 0, false));
        assert_eq!(mgr.toast, LockToast::PromptUnlock);
        assert_eq!(mgr.held_code, None);

        tick_n(&mut mgr, 20, false);
        assert!(!mgr.sleep_requested);
        assert!(!mgr.force_close_requested);
    }

    #[test]
    fn test_short_press_exit_aborts_without_held_confirmation() {
        let mut mgr = KeypadLockManager::new();
        let pressed = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        mgr.handle_event(&pressed, 400, 3000, 0, true);

        tick_n(&mut mgr, 12, true);

        assert_eq!(mgr.held_code, None);
        assert_eq!(mgr.delay_elapsed_ms, 0);
        assert!(!mgr.sleep_requested);
        assert!(!mgr.hold_ui_open);
    }

    #[test]
    fn test_exit_when_cannot_sleep_tracks_force_close() {
        let mut mgr = KeypadLockManager::new();
        let pressed = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Pressed,
        };
        assert!(!mgr.handle_event(&pressed, 400, 3000, 0, false));
        assert_eq!(mgr.held_code, Some(KeyCode::Exit));
        assert_eq!(mgr.hold_kind, HoldKind::CloseApp);

        let held = Event::Key {
            code: KeyCode::Exit,
            state: KeyState::Held,
        };
        assert!(mgr.handle_event(&held, 400, 3000, 0, false));
        assert!(!mgr.force_close_requested);

        tick_n(&mut mgr, 30, false);
        assert!(mgr.force_close_requested);
        assert!(!mgr.sleep_requested);
    }

    #[test]
    fn test_unlock_holding_f_does_not_activate_f_plus_until_pressed_again() {
        let mut mgr = KeypadLockManager::new();
        mgr.is_locked = true;
        assert!(!mgr.is_f_active);

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

        tick_n(&mut mgr, 32, false);

        assert!(!mgr.is_locked);
        assert!(!mgr.is_f_active);

        let held_more = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Held,
        };
        mgr.handle_event(&held_more, 400, 3000, 0, false);
        tick_n(&mut mgr, 1, false);
        assert!(!mgr.is_f_active);

        let released = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Released,
        };
        mgr.handle_event(&released, 400, 3000, 0, false);
        assert!(!mgr.is_f_active);
        assert!(!mgr.is_locked);

        // Short tap after unlock activates F+
        mgr.handle_event(&pressed, 400, 3000, 0, false);
        tick_n(&mut mgr, 3, false);
        mgr.handle_event(&released, 400, 3000, 0, false);
        assert!(mgr.is_f_active);
    }

    #[test]
    fn test_f_drawer_animates_slide_up_and_slide_down() {
        let mut mgr = KeypadLockManager::new();
        assert!(!mgr.should_show_f_drawer());
        assert!(!mgr.is_f_drawer_animating());

        let press = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Pressed,
        };
        let release = Event::Key {
            code: KeyCode::KeyF,
            state: KeyState::Released,
        };
        mgr.handle_event(&press, 400, 3000, 0, false);
        mgr.handle_event(&release, 400, 3000, 0, false);

        assert!(mgr.is_f_active);
        assert!(mgr.should_show_f_drawer());
        assert!(mgr.is_f_drawer_animating());
        assert_eq!(mgr.f_drawer_y_offset(40), 40);

        // Advance 4 ticks (step = 4 -> 4, 8, 12, 16)
        tick_n(&mut mgr, 1, false);
        assert_eq!(mgr.f_drawer_y_offset(40), 22);
        tick_n(&mut mgr, 3, false);
        assert_eq!(mgr.f_drawer_y_offset(40), 0);
        assert!(!mgr.is_f_drawer_animating());

        // Closing slides back down smoothly before hiding
        mgr.clear_f_active();
        assert!(!mgr.is_f_active);
        assert!(mgr.should_show_f_drawer());
        assert!(mgr.is_f_drawer_animating());

        tick_n(&mut mgr, 1, false);
        assert!(mgr.f_drawer_y_offset(40) > 0);
        assert!(mgr.should_show_f_drawer());

        tick_n(&mut mgr, 3, false);
        assert!(!mgr.should_show_f_drawer());
        assert!(!mgr.is_f_drawer_animating());
    }

    #[test]
    fn auto_lock_paused_while_key_is_held() {
        let mut mgr = KeypadLockManager::new();
        mgr.handle_event(
            &Event::Key {
                code: KeyCode::Exit,
                state: KeyState::Pressed,
            },
            400,
            3000,
            1000,
            false,
        );
        // Advance past auto_lock_ms (1000 ms) while Exit is held: must NOT auto-lock.
        for _ in 0..30 {
            mgr.last_tick_ms = 0;
            mgr.handle_event(&Event::Tick, 400, 3000, 1000, false);
        }
        assert!(!mgr.is_locked);
    }
}
