//! Industrial-Standard 19-Bit Parallel Multi-Key Matrix Scanner & Debounce Engine.

pub mod keys;
pub mod matrix;

pub use keys::*;
pub use matrix::*;

use heapless::Deque;
use kernel::event::{KeyCode, KeyState};

pub struct KeypadScanner {
    /// Multi-sample raw contact bitmask history (for robust flutter-free debounce confirmation)
    raw_history: [u32; 3],
    /// Currently debounced active key bitmask
    debounced: u32,

    /// Key index currently tracked for Hold and Repeat (0..19)
    held_idx: Option<u8>,
    /// Accumulated duration of currently held key in milliseconds
    held_ms: u16,
    /// Flag indicating whether the active key has crossed the initial hold threshold
    key_being_held: bool,

    /// Exact duration in milliseconds of the last released key (for Morse/CW)
    last_held_duration_ms: u16,

    /// Monotonic timestamp of last scan call
    last_scan_ms: u32,

    /// Timing configuration
    pub hold_delay_ms: u16,
    pub repeat_rate_ms: u16,
    pub fast_scroll_enabled: bool,

    /// FIFO event queue for dispatched key transitions
    event_queue: Deque<(KeyCode, KeyState), 16>,
}

impl KeypadScanner {
    /// Create a new scanner with DeltaFW standard timing defaults
    pub const fn new() -> Self {
        Self {
            raw_history: [0; 3],
            debounced: 0,
            held_idx: None,
            held_ms: 0,
            key_being_held: false,
            last_held_duration_ms: 0,
            last_scan_ms: 0,
            hold_delay_ms: 400, // DeltaFW standard: 400ms hold delay
            repeat_rate_ms: 80, // Standard UI repeat rate reference (80ms / 12.5 Hz)
            fast_scroll_enabled: true,
            event_queue: Deque::new(),
        }
    }

    pub fn clear(&mut self) {
        self.raw_history = [0; 3];
        self.debounced = 0;
        self.held_idx = None;
        self.held_ms = 0;
        self.key_being_held = false;
        self.last_held_duration_ms = 0;
        self.event_queue.clear();
    }

    #[inline(always)]
    pub fn poll_raw_mask() -> u32 {
        matrix::poll_raw_mask()
    }

    #[inline(always)]
    pub fn poll_matrix() -> Option<KeyCode> {
        matrix::poll_matrix()
    }

    #[inline(always)]
    pub fn reset_gpio_pins() {
        matrix::reset_gpio_pins();
    }

    /// Retrieve the hold duration in milliseconds of the last released key (for CW/Morse)
    #[inline(always)]
    pub fn last_press_duration_ms(&self) -> u16 {
        self.last_held_duration_ms
    }

    /// Retrieve the ongoing hold duration of the currently active key (for real-time CW keying)
    #[inline(always)]
    pub fn current_press_duration_ms(&self) -> u16 {
        self.held_ms
    }

    /// Query whether a specific key is currently debounced down
    #[inline(always)]
    pub fn is_key_down(&self, key: KeyCode) -> bool {
        (self.debounced & key_to_bit(key)) != 0
    }

    /// Query whether any key is currently pressed
    #[inline(always)]
    pub fn is_any_key_down(&self) -> bool {
        self.debounced != 0
    }

    /// Direct access to the active debounced bitmask
    #[inline(always)]
    pub fn active_mask(&self) -> u32 {
        self.debounced
    }

    /// Configure user timings from MasterConfig
    pub fn configure(&mut self, hold_delay_ms: u16, repeat_rate_ms: u16, fast_scroll: bool) {
        self.hold_delay_ms = hold_delay_ms;
        self.repeat_rate_ms = repeat_rate_ms;
        self.fast_scroll_enabled = fast_scroll;
    }

    /// Advance scanner state machine with elapsed time delta `dt_ms`.
    pub fn scan_with_dt(&mut self, dt_ms: u16) {
        let dt = dt_ms.max(1);
        let curr_raw = matrix::poll_raw_mask();

        let newly_pressed = (curr_raw & self.raw_history[0]) & !self.debounced;
        let newly_released =
            (!curr_raw & !self.raw_history[0] & !self.raw_history[1] & !self.raw_history[2])
                & self.debounced;

        self.raw_history[2] = self.raw_history[1];
        self.raw_history[1] = self.raw_history[0];
        self.raw_history[0] = curr_raw;
        self.debounced = (self.debounced | newly_pressed) & !newly_released;

        // 1. Process newly pressed keys
        if newly_pressed != 0 {
            for i in 0..19 {
                if (newly_pressed & (1 << i)) != 0 {
                    let key = ALL_KEYS[i];
                    let _ = self.event_queue.push_back((key, KeyState::Pressed));

                    if self.held_idx.is_none() {
                        self.held_idx = Some(i as u8);
                        self.held_ms = 0;
                        self.key_being_held = false;
                    }
                }
            }
        }

        // 2. Process newly released keys
        if newly_released != 0 {
            for i in 0..19 {
                if (newly_released & (1 << i)) != 0 {
                    let key = ALL_KEYS[i];
                    let _ = self.event_queue.push_back((key, KeyState::Released));

                    if self.held_idx == Some(i as u8) {
                        self.last_held_duration_ms = self.held_ms;
                        self.held_idx = None;
                        self.held_ms = 0;
                        self.key_being_held = false;
                    }
                }
            }
        }

        // 3. Handle Hold timing for active held_idx
        if let Some(idx) = self.held_idx {
            if (self.debounced & (1 << (idx as usize))) != 0 {
                self.held_ms = self.held_ms.saturating_add(dt);

                if !self.key_being_held && self.held_ms >= self.hold_delay_ms {
                    self.key_being_held = true;
                    let _ = self
                        .event_queue
                        .push_back((ALL_KEYS[idx as usize], KeyState::Held));
                }
            } else {
                self.held_idx = None;
                self.held_ms = 0;
                self.key_being_held = false;
            }
        }
    }

    /// Retrieve the next pending event from the FIFO queue
    #[inline]
    pub fn pop_event(&mut self) -> Option<(KeyCode, KeyState)> {
        self.event_queue.pop_front()
    }

    /// Primary scan routine called from main loop with monotonic time tracking
    pub fn scan(&mut self) -> Option<(KeyCode, KeyState)> {
        let now = py32_hal::delay::millis();
        let dt = if self.last_scan_ms == 0 {
            5
        } else {
            now.wrapping_sub(self.last_scan_ms).min(100) as u16
        };
        self.last_scan_ms = now;
        self.scan_with_dt(dt.max(1));
        self.pop_event()
    }
}
