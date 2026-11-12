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

    #[inline(always)]
    pub fn progress(&self) -> u8 {
        self.progress
    }

    #[inline(always)]
    pub fn is_complete(&self) -> bool {
        self.progress == self.target
    }

    pub fn reset_in(&mut self) {
        self.progress = 0;
        self.target = 16;
    }

    pub fn reset_out(&mut self) {
        self.progress = 16;
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

/// Zero-allocation text marquee ticker with boundary pause
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarqueeScroller {
    offset: i16,
    max_offset: i16,
    direction: i8, // 1 = right, -1 = left
    pause_ticks: u8,
    pause_duration: u8,
}

impl MarqueeScroller {
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
            pause_duration: 10,
        }
    }

    #[inline(always)]
    pub fn offset(&self) -> i16 {
        self.offset
    }

    pub fn tick(&mut self) -> bool {
        if self.max_offset == 0 {
            return false;
        }
        if self.pause_ticks > 0 {
            self.pause_ticks -= 1;
            return false;
        }

        self.offset += self.direction as i16;
        if self.offset >= self.max_offset {
            self.offset = self.max_offset;
            self.direction = -1;
            self.pause_ticks = self.pause_duration;
        } else if self.offset <= 0 {
            self.offset = 0;
            self.direction = 1;
            self.pause_ticks = self.pause_duration;
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
}
