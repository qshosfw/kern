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
            Shortcut::TaskManager => "Tasks",
            Shortcut::Fault => "Fault",
            Shortcut::Sleep => "Sleep",
            Shortcut::Reboot => "Reboot",
            Shortcut::LaunchApp(_) => "Launch App",
            Shortcut::PlayBeep { .. } => "Play Beep",
        }
    }

    pub fn from_u8(val: u8) -> Self {
        Self::ALL.get(val as usize).copied().unwrap_or(Shortcut::None)
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
