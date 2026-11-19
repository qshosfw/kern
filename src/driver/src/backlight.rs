//! LCD & Keypad Backlight Controller for KERN
//! Provides SysTick-driven 1000 Hz 10-step software PWM with zero extra hardware interrupts.

use core::sync::atomic::{AtomicU8, Ordering};
pub use kern_platform_traits::display::BacklightOutput;

pub static BACKLIGHT_DUTY: AtomicU8 = AtomicU8::new(0);
static BACKLIGHT_ACC: AtomicU8 = AtomicU8::new(0);
/// Active RGB channel bitmask (`bit0=R, bit1=G, bit2=B`) on RGB-capable targets.
#[cfg(feature = "rgb_backlight")]
pub static RGB_CHANNEL_MASK: AtomicU8 = AtomicU8::new(0b111);

#[inline(always)]
fn current_rgb_mask() -> u8 {
    #[cfg(feature = "rgb_backlight")]
    {
        RGB_CHANNEL_MASK.load(Ordering::Relaxed)
    }
    #[cfg(not(feature = "rgb_backlight"))]
    {
        0b111
    }
}

/// SysTick 20 kHz Delta-Sigma Rate Multiplier Software PWM Tick (called inside 50 µs SysTick interrupt handler)
#[inline(always)]
pub fn systick_pwm_tick<O: BacklightOutput>(out: &mut O) {
    let duty = BACKLIGHT_DUTY.load(Ordering::Relaxed);
    let mask = current_rgb_mask();
    if duty == 0 {
        out.set_output(false, 0);
        return;
    }
    if duty >= 10 {
        out.set_output(true, mask);
        return;
    }

    // Delta-Sigma (Bresenham) rate dispersal:
    // Distributes pulses evenly across the tick train, maximizing frequency
    // (up to 10 kHz) and eliminating low-frequency beat patterns against ST7565 LCD scan.
    let acc = BACKLIGHT_ACC.load(Ordering::Relaxed);
    let next = acc + duty;
    if next >= 10 {
        BACKLIGHT_ACC.store(next - 10, Ordering::Relaxed);
        out.set_output(true, mask);
    } else {
        BACKLIGHT_ACC.store(next, Ordering::Relaxed);
        out.set_output(false, 0);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklightMode {
    Off,
    Auto(u16), // Timeout in milliseconds
    On,
}

#[repr(align(4))]
pub struct BacklightController<O> {
    output: O,
    mode: BacklightMode,
    keypad_mode: u8, // 0=Off, 1=Sync Screen, 2=5s, 3=10s, 4=30s, 5=Always On
    keypad_elapsed_ms: u16,
    keypad_is_active: bool,
    backlight_max: u8, // 1..10
    backlight_min: u8, // 0..10
    elapsed_ms: u16,
    current_level: u8, // current PWM level 0..10
    target_level: u8,  // target PWM level 0..10
    fade_accumulator_ms: u16,
    is_active: bool,
}

impl<O: BacklightOutput> BacklightController<O> {
    pub const fn new(output: O) -> Self {
        Self {
            output,
            mode: BacklightMode::Auto(5000), // Default 5s
            keypad_mode: 1,                  // Default Sync Screen
            keypad_elapsed_ms: 0,
            keypad_is_active: false,
            backlight_max: 10,
            backlight_min: 0,
            elapsed_ms: 0,
            current_level: 0,
            target_level: 0,
            fade_accumulator_ms: 0,
            is_active: false,
        }
    }

    pub fn init(&mut self) {
        self.apply_hardware();
    }

    pub fn set_mode(&mut self, mode: BacklightMode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        match mode {
            BacklightMode::Off => {
                self.turn_off();
            }
            BacklightMode::On => {
                self.wake();
            }
            BacklightMode::Auto(_) => {
                if self.is_active {
                    self.elapsed_ms = 0;
                }
            }
        }
    }

    pub fn turn_off(&mut self) {
        self.is_active = false;
        self.keypad_is_active = false;
        self.current_level = 0;
        self.target_level = 0;
        self.fade_accumulator_ms = 0;
        self.apply_hardware();
    }

    pub fn set_keypad_mode(&mut self, mode: u8) {
        self.keypad_mode = mode;
        if self.mode != BacklightMode::Off {
            self.wake();
        }
    }

    pub fn set_levels(&mut self, max: u8, min: u8) {
        let max_clamped = max.clamp(1, 10);
        let min_clamped = min.clamp(0, 10);
        if self.backlight_max == max_clamped && self.backlight_min == min_clamped {
            return;
        }
        self.backlight_max = max_clamped;
        self.backlight_min = min_clamped;
        if self.mode == BacklightMode::Off {
            self.target_level = 0;
            self.current_level = 0;
        } else if self.is_active {
            self.target_level = self.backlight_max;
            self.current_level = self.backlight_max;
        } else {
            self.target_level = self.backlight_min;
            self.current_level = self.backlight_min;
        }
        self.apply_hardware();
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    /// Trigger backlight wake-up on key press or system event.
    pub fn wake(&mut self) {
        if self.mode == BacklightMode::Off {
            self.turn_off();
            return;
        }
        self.elapsed_ms = 0;
        self.is_active = true;
        self.keypad_elapsed_ms = 0;
        self.keypad_is_active = true;
        self.target_level = self.backlight_max;
        self.current_level = self.backlight_max;
        self.fade_accumulator_ms = 0;
        self.apply_hardware();
    }

    /// System tick handler called periodically with elapsed time. Ramps brightness smoothly toward target_level.
    pub fn tick(&mut self, delta_ms: u16) -> bool {
        match self.mode {
            BacklightMode::Off => {
                self.is_active = false;
                self.current_level = 0;
                self.target_level = 0;
                self.apply_hardware();
                return false;
            }
            BacklightMode::On => {
                self.is_active = true;
                self.target_level = self.backlight_max;
            }
            BacklightMode::Auto(timeout) => {
                if self.is_active {
                    self.elapsed_ms = self.elapsed_ms.saturating_add(delta_ms);
                    if self.elapsed_ms >= timeout {
                        self.is_active = false;
                        self.target_level = self.backlight_min;
                    }
                }
            }
        }

        // Smooth PWM Fading Logic (~50ms per step smooth ramp)
        if self.current_level != self.target_level {
            self.fade_accumulator_ms = self.fade_accumulator_ms.saturating_add(delta_ms);
            if self.fade_accumulator_ms >= 50 {
                self.fade_accumulator_ms = 0;
                if self.current_level < self.target_level {
                    self.current_level += 1;
                } else if self.current_level > self.target_level {
                    self.current_level -= 1;
                }
                self.apply_hardware();
            }
        } else {
            self.fade_accumulator_ms = 0;
        }

        self.is_active
    }

    fn apply_hardware(&mut self) {
        if self.mode == BacklightMode::Off || self.current_level == 0 {
            BACKLIGHT_DUTY.store(0, Ordering::Relaxed);
            self.output.set_output(false, 0);
        } else {
            BACKLIGHT_DUTY.store(self.current_level.min(10), Ordering::Relaxed);
        }
    }
}

impl<O: BacklightOutput + Default> Default for BacklightController<O> {
    fn default() -> Self {
        Self::new(O::default())
    }
}
