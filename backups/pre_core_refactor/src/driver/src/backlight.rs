//! LCD & Keypad Backlight Controller for KERN
//! Provides SysTick-driven 100 Hz 10-step software PWM with zero extra hardware interrupts.

use core::sync::atomic::{AtomicU8, Ordering};

pub static BACKLIGHT_DUTY: AtomicU8 = AtomicU8::new(0);
static BACKLIGHT_STEP: AtomicU8 = AtomicU8::new(0);

/// SysTick 100 Hz Software PWM Tick (called inside 1ms SysTick interrupt handler)
#[inline(always)]
pub fn systick_pwm_tick() {
    let duty = BACKLIGHT_DUTY.load(Ordering::Relaxed);
    if duty == 0 {
        bsp::pins::BACKLIGHT.set_low();
        return;
    }
    let step = BACKLIGHT_STEP.load(Ordering::Relaxed);

    if step < duty {
        bsp::pins::BACKLIGHT.set_high();
    } else {
        bsp::pins::BACKLIGHT.set_low();
    }

    BACKLIGHT_STEP.store((step + 1) % 10, Ordering::Relaxed);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklightMode {
    Off,
    Auto(u16), // Timeout in milliseconds
    On,
}

pub struct BacklightController {
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

impl BacklightController {
    pub const fn new() -> Self {
        Self {
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
        bsp::pins::BACKLIGHT.set_low();
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
        bsp::pins::BACKLIGHT.set_low();
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
                bsp::pins::BACKLIGHT.set_low();
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

    fn apply_hardware(&self) {
        if self.mode == BacklightMode::Off || self.current_level == 0 {
            BACKLIGHT_DUTY.store(0, Ordering::Relaxed);
            bsp::pins::BACKLIGHT.set_low();
        } else {
            BACKLIGHT_DUTY.store(self.current_level.min(10), Ordering::Relaxed);
        }
    }
}
