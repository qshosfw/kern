use kernel::ipc::SysEvent;

/// Single Declarative Shortcut Action Registry
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Shortcut {
    None = 0,
    Flashlight = 1,
    Monitor = 2,
    Scanner = 3,
    Explorer = 4,
    Settings = 5,
    LockKeypad = 6,
    ToggleVfo = 7,
    ToggleTxPower = 8,
    MuteAudio = 9,
    FmRadio = 10,
    TaskManager = 11,
    Fault = 12,
    Sleep = 13,
    Reboot = 14,
    LaunchApp(u8) = 15,
    PlayBeep { freq_hz: u16, duration_ms: u16 } = 16,
}

impl Shortcut {
    /// Single source of truth for UI shortcut option strings (`NUL`-terminated).
    pub const PACKED_NAMES: &'static str = "None\0Flashlight\0Monitor\0Scanner\0Explorer\0Settings\0Lock Keypad\0Toggle VFO\0TX Power\0Mute Audio\0FM Radio\0Tasks\0Fault\0Sleep\0";

    pub const ALL: &'static [Shortcut] = &[
        Shortcut::None,
        Shortcut::Flashlight,
        Shortcut::Monitor,
        Shortcut::Scanner,
        Shortcut::Explorer,
        Shortcut::Settings,
        Shortcut::LockKeypad,
        Shortcut::ToggleVfo,
        Shortcut::ToggleTxPower,
        Shortcut::MuteAudio,
        Shortcut::FmRadio,
        Shortcut::TaskManager,
        Shortcut::Fault,
        Shortcut::Sleep,
    ];

    pub fn name(&self) -> &'static str {
        let idx = match self {
            Shortcut::None => 0,
            Shortcut::Flashlight => 1,
            Shortcut::Monitor => 2,
            Shortcut::Scanner => 3,
            Shortcut::Explorer => 4,
            Shortcut::Settings => 5,
            Shortcut::LockKeypad => 6,
            Shortcut::ToggleVfo => 7,
            Shortcut::ToggleTxPower => 8,
            Shortcut::MuteAudio => 9,
            Shortcut::FmRadio => 10,
            Shortcut::TaskManager => 11,
            Shortcut::Fault => 12,
            Shortcut::Sleep => 13,
            Shortcut::Reboot => return "Reboot",
            Shortcut::LaunchApp(_) => return "Launch App",
            Shortcut::PlayBeep { .. } => return "Play Beep",
        };
        let bytes = Self::PACKED_NAMES.as_bytes();
        let mut cur = 0usize;
        let mut start = 0usize;
        for (i, &b) in bytes.iter().enumerate() {
            if b == 0 {
                if cur == idx {
                    return Self::PACKED_NAMES.get(start..i).unwrap_or("None");
                }
                cur += 1;
                start = i + 1;
            }
        }
        "None"
    }

    pub fn from_u8(val: u8) -> Self {
        Self::ALL
            .get(val as usize)
            .copied()
            .unwrap_or(Shortcut::None)
    }
}

impl From<Shortcut> for SysEvent {
    fn from(val: Shortcut) -> Self {
        let (command, arg) = match val {
            Shortcut::None => (0, 0),
            Shortcut::Flashlight => (1, 0),
            Shortcut::Monitor => (2, 0),
            Shortcut::Scanner => (3, 0),
            Shortcut::Explorer => (4, 0),
            Shortcut::Settings => (5, 0),
            Shortcut::LockKeypad => (6, 0),
            Shortcut::ToggleVfo => (7, 0),
            Shortcut::ToggleTxPower => (8, 0),
            Shortcut::MuteAudio => (9, 0),
            Shortcut::FmRadio => (10, 0),
            Shortcut::TaskManager => (11, 0),
            Shortcut::Fault => (12, 0),
            Shortcut::Sleep => (13, 0),
            Shortcut::Reboot => (14, 0),
            Shortcut::LaunchApp(app_id) => (15, app_id as u16),
            Shortcut::PlayBeep {
                freq_hz,
                duration_ms: _,
            } => (16, freq_hz),
        };
        SysEvent {
            target: 0,
            command,
            arg,
        }
    }
}

impl From<SysEvent> for Shortcut {
    fn from(evt: SysEvent) -> Self {
        match evt.command {
            1..=13 => Self::from_u8(evt.command),
            14 => Shortcut::Reboot,
            15 => Shortcut::LaunchApp(evt.arg as u8),
            16 => Shortcut::PlayBeep {
                freq_hz: evt.arg,
                duration_ms: 100,
            },
            _ => Shortcut::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_names_and_sysevent_roundtrip() {
        for (idx, &sc) in Shortcut::ALL.iter().enumerate() {
            assert_eq!(Shortcut::from_u8(idx as u8), sc);
            assert!(!sc.name().is_empty());
            if idx > 0 {
                let evt = SysEvent::from(sc);
                assert_eq!(Shortcut::from(evt), sc);
            }
        }
        assert_eq!(Shortcut::Reboot.name(), "Reboot");
        assert_eq!(Shortcut::LaunchApp(2).name(), "Launch App");
    }
}
