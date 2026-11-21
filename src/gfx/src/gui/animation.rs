//! Kinetic Animation Controller & State Transition Engines.
//!
//! Provides zero-allocation, deterministic integer animation state machines for:
//! 1. `KineticScroller`: Framebuffer and viewport scrolling interpolation (2px/step).
//! 2. `TransitionProgress`: 0..=16 progress tracker for Bayer dither dissolves,
//!    bottom drawer slide-ups, and popup materialization.
//! 3. `MarqueeScroller`: Text ticker with boundary hold delay.

/// Scrolling behavior mode for lists and menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListScrollMode {
    /// Paged: Flips whole page at once (0..4, 4..8) matching classic paginated menus.
    Paged,
    /// Windowed: Cursor moves to the absolute edge (slot 0 or slot vis-1) before scrolling 1 row.
    #[default]
    Windowed,
    /// Thirds / Margin: Keeps a margin from top/bottom borders (classic rule-of-thirds padding),
    /// scrolling row-by-row when pushing against the border in that direction.
    Thirds,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KineticScroller {
    current: i16,
    target: i16,
    step: u8,
}

impl KineticScroller {
    pub const fn new(step: u8) -> Self {
        Self {
            current: 0,
            target: 0,
            step: if step == 0 { 1 } else { step },
        }
    }

    #[inline(always)]
    pub fn current(&self) -> i16 {
        self.current
    }

    #[inline(always)]
    pub fn target(&self) -> i16 {
        self.target
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.current != self.target
    }

    pub fn set_target(&mut self, target: i16) {
        self.target = target;
    }

    /// Immediate jump to target without smooth scrolling
    pub fn jump(&mut self, target: i16) {
        self.target = target;
        self.current = target;
    }

    /// Advance one frame towards target with granular high-resolution stepping.
    /// Tuned for ST7565/ST7567 monochrome LCD pixel relaxation times (20-30ms)
    /// and 120 Hz refresh rates to eliminate multi-row jumping and motion streaking.
    pub fn tick(&mut self) -> bool {
        if self.current == self.target {
            return false;
        }
        let diff = self.target - self.current;
        let abs_diff = diff.abs();

        // Granular deceleration curve tuned for human visual perception and ST7565 LC response:
        // At 120 Hz (8ms frame interval), single-row displacement (<= 14px) advances at strictly
        // 1 pixel per frame (13 frames * 8ms = 104ms) when step <= 1, completely eliminating
        // liquid crystal fall-time ghosting and "white streaking" while creating a fluid, tactile glide.
        // Multi-row displacement (> 14px) scales smoothly to 2-3px per frame.
        let step = if abs_diff <= (self.step as i16) {
            abs_diff
        } else if abs_diff <= 6 {
            (self.step as i16).clamp(1, 2)
        } else if abs_diff <= 14 {
            3
        } else if abs_diff <= 28 {
            4
        } else {
            // Fast reciprocal division: abs_diff / 6 for abs_diff > 28
            (((abs_diff as u32 * 0xAAAB) >> 18) as i16).min(8)
        };

        if abs_diff <= step {
            self.current = self.target;
        } else if diff > 0 {
            self.current += step;
        } else {
            self.current -= step;
        }
        true
    }
}

impl Default for KineticScroller {
    fn default() -> Self {
        Self::new(1)
    }
}

/// 16-step transition progress controller (for Bayer dither, slide-up, zoom)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionProgress {
    progress: u8, // 0..=16
    target: u8,   // 0 or 16
    step: u8,
}

impl TransitionProgress {
    pub const fn new(step: u8) -> Self {
        Self {
            progress: 0,
            target: 16,
            step: if step == 0 { 2 } else { step },
        }
    }

    pub const fn completed(step: u8) -> Self {
        Self {
            progress: 16,
            target: 16,
            step: if step == 0 { 2 } else { step },
        }
    }

    pub const fn closed(step: u8) -> Self {
        Self {
            progress: 0,
            target: 0,
            step: if step == 0 { 2 } else { step },
        }
    }

    #[inline(always)]
    pub fn progress(&self) -> u8 {
        self.progress
    }

    #[inline(always)]
    pub fn is_complete(&self) -> bool {
        self.progress == self.target
    }

    #[inline(always)]
    pub fn is_opening(&self) -> bool {
        self.target == 16
    }

    #[inline(always)]
    pub fn open(&mut self) {
        self.target = 16;
    }

    #[inline(always)]
    pub fn close(&mut self) {
        self.target = 0;
    }

    /// Quadratic ease-out vertical slide offset from `height` down to `0`.
    #[inline]
    pub fn slide_offset(&self, height: u8) -> i8 {
        if self.progress >= 16 {
            0
        } else {
            let rem = (16 - self.progress) as u32;
            (((rem * rem) * height as u32) >> 8) as i8
        }
    }

    pub fn reset_in(&mut self) {
        self.progress = 0;
        self.target = 16;
    }

    pub fn reset_out(&mut self) {
        self.progress = 16;
        self.target = 0;
    }

    #[inline(always)]
    pub fn reset_closed(&mut self) {
        self.progress = 0;
        self.target = 0;
    }

    pub fn jump_complete(&mut self) {
        self.progress = self.target;
    }

    /// Advance one animation tick. Returns true if progress changed.
    pub fn tick(&mut self) -> bool {
        if self.progress == self.target {
            return false;
        }
        if self.progress < self.target {
            self.progress = (self.progress + self.step).min(self.target);
        } else {
            self.progress = self.progress.saturating_sub(self.step).max(self.target);
        }
        true
    }
}

/// Zero-allocation text marquee ticker with reading-speed pacing, start/end pauses,
/// and smooth return back to the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarqueeScroller {
    offset: i16,
    max_offset: i16,
    direction: i8, // 1 = scroll forward, -1 = return back to 0
    pause_ticks: u8,
    start_pause: u8,
    end_pause: u8,
    fwd_div: u8,
    rev_div: u8,
    tick_acc: u8,
}

impl MarqueeScroller {
    /// Idle reading-speed marquee controller (zero overflow until synced).
    pub const fn idle() -> Self {
        Self::reading(0)
    }

    /// Reading-speed marquee tuned for handheld LCDs: pauses to read the start,
    /// glides forward smoothly (1px per 2 ticks), pauses at the end, then glides back.
    pub const fn reading(overflow_px: u8) -> Self {
        Self {
            offset: 0,
            max_offset: overflow_px as i16,
            direction: 1,
            pause_ticks: 24,
            start_pause: 24,
            end_pause: 18,
            fwd_div: 2,
            rev_div: 1,
            tick_acc: 0,
        }
    }

    pub const fn new(text_width: u8, viewport_width: u8) -> Self {
        let max_offset = if text_width > viewport_width {
            (text_width - viewport_width) as i16
        } else {
            0
        };
        Self {
            offset: 0,
            max_offset,
            direction: 1,
            pause_ticks: 10,
            start_pause: 10,
            end_pause: 10,
            fwd_div: 1,
            rev_div: 1,
            tick_acc: 0,
        }
    }

    #[inline(always)]
    pub const fn offset(&self) -> i16 {
        self.offset
    }

    #[inline(always)]
    pub const fn is_active(&self) -> bool {
        self.max_offset > 0
    }

    /// Reset scroll position to 0 and restart the initial reading pause.
    #[inline(always)]
    pub fn reset(&mut self) {
        self.offset = 0;
        self.direction = 1;
        self.pause_ticks = self.start_pause;
        self.tick_acc = 0;
    }

    /// Update the overflow span; resets smoothly when the span changes or shrinks to 0.
    #[inline(always)]
    pub fn set_overflow(&mut self, overflow_px: u8) {
        let max_off = overflow_px as i16;
        if max_off != self.max_offset {
            self.max_offset = max_off;
            self.reset();
        }
    }

    pub fn tick(&mut self) -> bool {
        if self.max_offset <= 0 {
            return false;
        }
        if self.pause_ticks > 0 {
            self.pause_ticks -= 1;
            return false;
        }
        let div = if self.direction > 0 {
            self.fwd_div
        } else {
            self.rev_div
        };
        self.tick_acc = self.tick_acc.saturating_add(1);
        if self.tick_acc < div {
            return false;
        }
        self.tick_acc = 0;

        self.offset += self.direction as i16;
        if self.offset >= self.max_offset {
            self.offset = self.max_offset;
            self.direction = -1;
            self.pause_ticks = self.end_pause;
        } else if self.offset <= 0 {
            self.offset = 0;
            self.direction = 1;
            self.pause_ticks = self.start_pause;
        }
        true
    }
}

/// Slide-up animation controller for bottom sheets and confirmation dialogs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlideUpAnim {
    offset: i16,
    max_offset: i16,
    step: u8,
    is_open: bool,
}

impl SlideUpAnim {
    pub const fn new(height: u8, step: u8) -> Self {
        Self {
            offset: height as i16,
            max_offset: height as i16,
            step: if step == 0 { 4 } else { step },
            is_open: false,
        }
    }

    #[inline(always)]
    pub fn offset(&self) -> i16 {
        self.offset
    }

    #[inline(always)]
    pub fn is_open(&self) -> bool {
        self.is_open && self.offset == 0
    }

    #[inline(always)]
    pub fn is_closed(&self) -> bool {
        !self.is_open && self.offset == self.max_offset
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        if self.is_open {
            self.offset > 0
        } else {
            self.offset < self.max_offset
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn jump_open(&mut self) {
        self.is_open = true;
        self.offset = 0;
    }

    pub fn jump_close(&mut self) {
        self.is_open = false;
        self.offset = self.max_offset;
    }

    /// Advance slide-up motion one frame. Returns true if offset changed.
    pub fn tick(&mut self) -> bool {
        let step = self.step as i16;
        if self.is_open {
            if self.offset > 0 {
                self.offset = (self.offset - step).max(0);
                return true;
            }
        } else if self.offset < self.max_offset {
            self.offset = (self.offset + step).min(self.max_offset);
            return true;
        }
        false
    }
}

/// Shared hold-to-confirm progress tracker with springy 3x rewind on early release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct HoldProgress {
    pub active: bool,
    pub elapsed_ms: u16,
    pub required_ms: u16,
}

impl HoldProgress {
    pub const fn new() -> Self {
        Self {
            active: false,
            elapsed_ms: 0,
            required_ms: 0,
        }
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        self.active
            || (self.required_ms > 0 && self.elapsed_ms > 0 && self.elapsed_ms < self.required_ms)
    }

    #[inline(never)]
    pub fn percent(&self) -> u8 {
        if self.required_ms == 0 {
            return 0;
        }
        (((self.elapsed_ms as u32) * 100) / (self.required_ms as u32)).min(100) as u8
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.active = false;
        self.elapsed_ms = 0;
    }

    /// Advance forward while `active` (returns `true` once on reaching `required_ms`),
    /// or springily rewind at 3x speed back to 0 when released early.
    #[inline(never)]
    pub fn advance(&mut self, dt_ms: u16) -> bool {
        if self.required_ms == 0 {
            return false;
        }
        if self.active {
            self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);
            if self.elapsed_ms >= self.required_ms {
                self.elapsed_ms = self.required_ms;
                self.active = false;
                return true;
            }
        } else if self.elapsed_ms > 0 {
            self.elapsed_ms = self.elapsed_ms.saturating_sub(dt_ms.saturating_mul(3));
        }
        false
    }
}

impl Default for HoldProgress {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kinetic_scroller_stepping() {
        let mut scroller = KineticScroller::new(2);
        scroller.set_target(5);

        assert!(scroller.is_animating());
        assert!(scroller.tick());
        assert_eq!(scroller.current(), 2);

        assert!(scroller.tick());
        assert_eq!(scroller.current(), 4);

        assert!(scroller.tick());
        assert_eq!(scroller.current(), 5);
        assert!(!scroller.is_animating());
        assert!(!scroller.tick());

        scroller.jump(20);
        assert_eq!(scroller.current(), 20);
        assert!(!scroller.is_animating());
    }

    #[test]
    fn test_transition_progress() {
        let mut trans = TransitionProgress::new(4);
        trans.reset_in();
        assert_eq!(trans.progress(), 0);
        assert!(!trans.is_complete());

        assert!(trans.tick());
        assert_eq!(trans.progress(), 4);
        assert!(trans.tick());
        assert_eq!(trans.progress(), 8);
        assert!(trans.tick());
        assert_eq!(trans.progress(), 12);
        assert!(trans.tick());
        assert_eq!(trans.progress(), 16);
        assert!(trans.is_complete());
        assert!(!trans.tick());
    }

    #[test]
    fn test_marquee_scroller() {
        let mut marquee = MarqueeScroller::new(50, 40);
        assert_eq!(marquee.offset(), 0);

        // Consume initial pause
        for _ in 0..10 {
            assert!(!marquee.tick());
        }

        // Advance 1 pixel
        assert!(marquee.tick());
        assert_eq!(marquee.offset(), 1);
    }

    #[test]
    fn test_reading_marquee_pauses_scrolls_and_returns() {
        let mut m = MarqueeScroller::reading(4);
        assert!(m.is_active());
        assert_eq!(m.offset(), 0);

        // Start pause (24 ticks)
        for _ in 0..24 {
            assert!(!m.tick());
        }
        // Forward reading speed: 2 ticks per pixel (4 pixels = 8 ticks)
        for step in 1..=4 {
            assert!(!m.tick());
            assert!(m.tick());
            assert_eq!(m.offset(), step);
        }
        // End pause (18 ticks)
        for _ in 0..18 {
            assert!(!m.tick());
        }
        // Smooth return back to 0 (1 tick per pixel)
        for step in (0..4).rev() {
            assert!(m.tick());
            assert_eq!(m.offset(), step);
        }
        // Enters start pause again
        assert!(!m.tick());
        assert_eq!(m.offset(), 0);

        // Reset clears offset immediately
        m.set_overflow(10);
        assert_eq!(m.offset(), 0);
        m.set_overflow(0);
        assert!(!m.is_active());
    }

    #[test]
    fn test_slide_up_anim() {
        let mut slide = SlideUpAnim::new(20, 5);
        assert_eq!(slide.offset(), 20);
        assert!(slide.is_closed());
        assert!(!slide.is_open());
        assert!(!slide.is_animating());

        slide.open();
        assert!(slide.is_animating());
        assert!(slide.tick());
        assert_eq!(slide.offset(), 15);
        assert!(slide.tick());
        assert_eq!(slide.offset(), 10);
        assert!(slide.tick());
        assert_eq!(slide.offset(), 5);
        assert!(slide.tick());
        assert_eq!(slide.offset(), 0);
        assert!(slide.is_open());
        assert!(!slide.is_animating());
        assert!(!slide.tick());

        slide.close();
        assert!(slide.is_animating());
        assert!(slide.tick());
        assert_eq!(slide.offset(), 5);
        slide.jump_close();
        assert_eq!(slide.offset(), 20);
        assert!(slide.is_closed());
    }

    #[test]
    fn test_hold_progress_advance_and_spring_rewind() {
        let mut hold = HoldProgress::new();
        hold.required_ms = 300;
        assert!(!hold.is_animating());
        assert_eq!(hold.percent(), 0);

        hold.active = true;
        assert!(!hold.advance(150));
        assert_eq!(hold.percent(), 50);
        assert!(hold.is_animating());

        // Early release springs back 3x faster (50ms -> -150ms => 0)
        hold.active = false;
        assert!(!hold.advance(50));
        assert_eq!(hold.elapsed_ms, 0);
        assert!(!hold.is_animating());

        // Sustained hold reaches 100% and returns true once
        hold.active = true;
        assert!(!hold.advance(200));
        assert!(hold.advance(100));
        assert_eq!(hold.percent(), 100);
        assert!(!hold.active);
    }

    #[test]
    fn test_transition_progress_ease_slide_offset() {
        let mut trans = TransitionProgress::closed(4);
        assert_eq!(trans.slide_offset(40), 40);
        trans.open();
        assert!(trans.tick()); // 4
        assert_eq!(trans.slide_offset(40), 22);
        assert!(trans.tick()); // 8
        assert_eq!(trans.slide_offset(40), 10);
        assert!(trans.tick()); // 12
        assert_eq!(trans.slide_offset(40), 2);
        assert!(trans.tick()); // 16
        assert_eq!(trans.slide_offset(40), 0);
        assert!(trans.is_complete());
    }
}
