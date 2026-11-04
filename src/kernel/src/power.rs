//! Microkernel Power Management & Dynamic Power State Contracts

/// Operating System Power States
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    /// Active: Full LCD backlight and maximum responsiveness.
    Active,
    /// Standby: Backlight off, UI throttled, SPI blits suppressed.
    Standby,
    /// DeepSleep: Subsystems powered off, core downclocked to 24 MHz HSI, periodic strobe heartbeat.
    DeepSleep,
}

/// Power Management Profiles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    Balanced = 0,
    Performance = 1,
    PowerSave = 2,
    DeepSleep = 3,
}

impl PowerProfile {
    pub const fn from_u8(val: u8) -> Self {
        match val {
            0 => PowerProfile::Balanced,
            1 => PowerProfile::Performance,
            2 => PowerProfile::PowerSave,
            3 => PowerProfile::DeepSleep,
            _ => PowerProfile::Balanced,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_profile_conversion() {
        assert_eq!(PowerProfile::from_u8(0), PowerProfile::Balanced);
        assert_eq!(PowerProfile::from_u8(1), PowerProfile::Performance);
        assert_eq!(PowerProfile::from_u8(2), PowerProfile::PowerSave);
        assert_eq!(PowerProfile::from_u8(3), PowerProfile::DeepSleep);
        assert_eq!(PowerProfile::from_u8(99), PowerProfile::Balanced);
    }

    #[test]
    fn test_power_state_equality() {
        assert_eq!(PowerState::Active, PowerState::Active);
        assert_ne!(PowerState::Active, PowerState::Standby);
        assert_ne!(PowerState::Standby, PowerState::DeepSleep);
    }
}
