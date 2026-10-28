//! Granular Storage Access Control & File Security Attributes

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileSecurityLevel {
    /// Level 0: Unencrypted public file (Fonts, UI icons, default presets)
    Public = 0,
    /// Level 1: Encrypted with MCU Silicon 96-bit UID (Prevents SPI Flash cloning to another radio)
    DeviceBound = 1,
    /// Level 2: Encrypted with Master Session Key (Requires User Passcode authentication)
    UserAuthRead = 2,
    /// Level 3: Requires Master Session Key + Capability Token
    UserAuthWrite = 3,
    /// Level 4: Auto-zeroizes SRAM key buffer on MAC verification failure or physical tamper event
    TamperZeroize = 4,
}

impl FileSecurityLevel {
    pub fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::DeviceBound,
            2 => Self::UserAuthRead,
            3 => Self::UserAuthWrite,
            4 => Self::TamperZeroize,
            _ => Self::Public,
        }
    }
}
