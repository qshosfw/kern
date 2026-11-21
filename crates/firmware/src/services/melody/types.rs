//! Melody, Roger Beep, and Sound Cue Type Definitions.

pub use drivers::audio::ToneNote;
use heapless::Vec;

/// Maximum notes in a standard melody sequence
pub const MAX_MELODY_NOTES: usize = 32;
pub type MelodyBuffer = Vec<ToneNote, MAX_MELODY_NOTES>;

/// Roger Beep (end-of-transmission) Melody Types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RogerType {
    Off = 0,
    Classic = 1,
    Tiny = 2,
    #[cfg(feature = "extra_rogers")]
    MotoTRBO = 3,
    #[cfg(feature = "extra_rogers")]
    Apx6000 = 4,
    #[cfg(feature = "extra_rogers")]
    T40 = 5,
    #[cfg(feature = "extra_rogers")]
    Tlkrt80 = 6,
    #[cfg(feature = "extra_rogers")]
    Cobra = 7,
    #[cfg(feature = "extra_rogers")]
    Police = 8,
    #[cfg(feature = "extra_rogers")]
    Uv5rc = 9,
    #[cfg(feature = "extra_rogers")]
    Quindar = 10,
    CustomFile = 12,
}

impl RogerType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => RogerType::Off,
            1 => RogerType::Classic,
            2 => RogerType::Tiny,
            #[cfg(feature = "extra_rogers")]
            3 => RogerType::MotoTRBO,
            #[cfg(feature = "extra_rogers")]
            4 => RogerType::Apx6000,
            #[cfg(feature = "extra_rogers")]
            5 => RogerType::T40,
            #[cfg(feature = "extra_rogers")]
            6 => RogerType::Tlkrt80,
            #[cfg(feature = "extra_rogers")]
            7 => RogerType::Cobra,
            #[cfg(feature = "extra_rogers")]
            8 => RogerType::Police,
            #[cfg(feature = "extra_rogers")]
            9 => RogerType::Uv5rc,
            #[cfg(feature = "extra_rogers")]
            10 => RogerType::Quindar,
            12 => RogerType::CustomFile,
            #[cfg(not(feature = "extra_rogers"))]
            10 => RogerType::CustomFile, // Backward compatibility
            _ => RogerType::Off,
        }
    }
}

/// Power-On Boot Jingle Melody Types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BootJingleType {
    Off = 0,
    Chime = 1,
    Nokia = 2,
    Chirp = 3,
    GameBoy = 4,
}

impl BootJingleType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => BootJingleType::Off,
            1 => BootJingleType::Chime,
            2 => BootJingleType::Nokia,
            3 => BootJingleType::Chirp,
            4 => BootJingleType::GameBoy,
            _ => BootJingleType::Off,
        }
    }
}

/// Keypress Beep Tone Types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BeepType {
    Off = 0,
    Tone = 1,
    Click = 2,
    Double = 3,
    Warning = 4,
    Scroll = 5,
    Custom = 6,
}

impl BeepType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            0 => BeepType::Off,
            1 => BeepType::Tone,
            2 => BeepType::Click,
            3 => BeepType::Double,
            4 => BeepType::Warning,
            5 => BeepType::Scroll,
            6 => BeepType::Custom,
            _ => BeepType::Tone,
        }
    }
}

/// System UI dialog / notification sound types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DialogSoundType {
    /// Keypad locked: descending double click (Nokia-style)
    Lock = 0,
    /// Keypad unlocked: ascending double chime
    Unlock = 1,
    /// Blocked key while locked: subtle gentle pip
    Prompt = 2,
    /// Confirmation dialog presented: inquiring double pip
    Confirm = 3,
    /// Action succeeded (save, delete, format complete)
    Success = 4,
    /// Soft warning (out of range, limit reached)
    Warning = 5,
    /// Error (storage fault, operation failed)
    Error = 6,
    /// Charging connected: bright ascending major power chime
    Charging = 7,
    /// USB / Serial connected: crisp rising digital handshake
    UsbConnect = 8,
    /// USB disconnected: gentle descending tone
    UsbDisconnect = 9,
}
