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
    LaunchApp(u8) = 11,
    PlayBeep { freq_hz: u16, duration_ms: u16 } = 12,
    Reboot = 13,
}

impl Shortcut {
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
    ];

    pub const fn name(&self) -> &'static str {
        match self {
            Shortcut::None => "None",
            Shortcut::Flashlight => "Flashlight",
            Shortcut::Monitor => "Monitor",
            Shortcut::Scanner => "Scanner",
            Shortcut::Explorer => "Explorer",
            Shortcut::Settings => "Settings",
            Shortcut::LockKeypad => "Lock Keypad",
            Shortcut::ToggleVfo => "Toggle VFO",
            Shortcut::ToggleTxPower => "TX Power",
            Shortcut::MuteAudio => "Mute Audio",
            Shortcut::FmRadio => "FM Radio",
            Shortcut::LaunchApp(_) => "Launch App",
            Shortcut::PlayBeep { .. } => "Play Beep",
            Shortcut::Reboot => "Reboot",
        }
    }

    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => Shortcut::Flashlight,
            2 => Shortcut::Monitor,
            3 => Shortcut::Scanner,
            4 => Shortcut::Explorer,
            5 => Shortcut::Settings,
            6 => Shortcut::LockKeypad,
            7 => Shortcut::ToggleVfo,
            8 => Shortcut::ToggleTxPower,
            9 => Shortcut::MuteAudio,
            10 => Shortcut::FmRadio,
            _ => Shortcut::None,
        }
    }
}

impl From<Shortcut> for SysEvent {
    fn from(val: Shortcut) -> Self {
        match val {
            Shortcut::None => SysEvent {
                target: 0,
                command: 0,
                arg: 0,
            },
            Shortcut::Flashlight => SysEvent {
                target: 0,
                command: 1,
                arg: 0,
            },
            Shortcut::Monitor => SysEvent {
                target: 0,
                command: 2,
                arg: 0,
            },
            Shortcut::Scanner => SysEvent {
                target: 0,
                command: 3,
                arg: 0,
            },
            Shortcut::Explorer => SysEvent {
                target: 0,
                command: 4,
                arg: 0,
            },
            Shortcut::Settings => SysEvent {
                target: 0,
                command: 5,
                arg: 0,
            },
            Shortcut::LockKeypad => SysEvent {
                target: 0,
                command: 6,
                arg: 0,
            },
            Shortcut::ToggleVfo => SysEvent {
                target: 0,
                command: 7,
                arg: 0,
            },
            Shortcut::ToggleTxPower => SysEvent {
                target: 0,
                command: 8,
                arg: 0,
            },
            Shortcut::MuteAudio => SysEvent {
                target: 0,
                command: 9,
                arg: 0,
            },
            Shortcut::FmRadio => SysEvent {
                target: 0,
                command: 10,
                arg: 0,
            },
            Shortcut::LaunchApp(app_id) => SysEvent {
                target: 0,
                command: 11,
                arg: app_id as u16,
            },
            Shortcut::PlayBeep {
                freq_hz,
                duration_ms: _,
            } => SysEvent {
                target: 0,
                command: 12,
                arg: freq_hz,
            },
            Shortcut::Reboot => SysEvent {
                target: 0,
                command: 13,
                arg: 0,
            },
        }
    }
}

impl From<SysEvent> for Shortcut {
    fn from(evt: SysEvent) -> Self {
        match evt.command {
            1 => Shortcut::Flashlight,
            2 => Shortcut::Monitor,
            3 => Shortcut::Scanner,
            4 => Shortcut::Explorer,
            5 => Shortcut::Settings,
            6 => Shortcut::LockKeypad,
            7 => Shortcut::ToggleVfo,
            8 => Shortcut::ToggleTxPower,
            9 => Shortcut::MuteAudio,
            10 => Shortcut::FmRadio,
            11 => Shortcut::LaunchApp(evt.arg as u8),
            12 => Shortcut::PlayBeep {
                freq_hz: evt.arg,
                duration_ms: 100,
            },
            13 => Shortcut::Reboot,
            _ => Shortcut::None,
        }
    }
}
