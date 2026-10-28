//! Microkernel Power Management & Dynamic Power State Transitions
/// Operating System Power States
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// Active: Full LCD & Keypad backlight, active display, 100% responsiveness
    Active,
    /// Dimmed: Backlight dimmed, preparing for idle standby
    Dimmed,
    /// Standby: Backlight off, periodic LED heartbeat breathing pulse via BK4829 / Status LED
    Standby,
    /// Hibernate: Deep sleep MCU stop mode
    Hibernate,
}

/// Power Management Profiles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    Performance,
    Balanced,
    PowerSaver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    Active,
    DimScreen,
    Standby { led_pulse: u8 },
}

/// Dynamic OS Power Manager supporting key wake-up, screen dimming, and periodic LED heartbeat pulse
pub struct PowerManager {
    state: PowerState,
    profile: PowerProfile,
    inactivity_ms: u32,
    led_pulse_tick: u32,
}

impl PowerManager {
    pub const fn new(profile: PowerProfile) -> Self {
        Self {
            state: PowerState::Active,
            profile,
            inactivity_ms: 0,
            led_pulse_tick: 0,
        }
    }

    pub fn state(&self) -> PowerState {
        self.state
    }

    pub fn set_profile(&mut self, profile: PowerProfile) {
        self.profile = profile;
        self.wake();
    }

    /// Wake up power manager on any hardware keypad input or system event
    pub fn wake(&mut self) {
        self.inactivity_ms = 0;
        self.state = PowerState::Active;
    }

    /// Handle system tick elapsed milliseconds and calculate power state transitions & LED pulse actions
    pub fn tick(&mut self, delta_ms: u32) -> PowerAction {
        self.inactivity_ms = self.inactivity_ms.saturating_add(delta_ms);

        let (dim_timeout, standby_timeout) = match self.profile {
            PowerProfile::Performance => (15_000, 60_000),
            PowerProfile::Balanced => (5_000, 15_000),
            PowerProfile::PowerSaver => (2_000, 5_000),
        };

        if self.inactivity_ms >= standby_timeout {
            self.state = PowerState::Standby;
            self.led_pulse_tick = self.led_pulse_tick.wrapping_add(delta_ms);
            // Periodic 2-second LED breathing pulse in Standby mode
            let pulse_intensity = if (self.led_pulse_tick % 2000) < 300 {
                255
            } else {
                0
            };
            PowerAction::Standby {
                led_pulse: pulse_intensity,
            }
        } else if self.inactivity_ms >= dim_timeout {
            self.state = PowerState::Dimmed;
            PowerAction::DimScreen
        } else {
            self.state = PowerState::Active;
            PowerAction::Active
        }
    }
}
