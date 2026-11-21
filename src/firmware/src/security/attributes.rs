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

    /// Returns whether this security level encrypts whole-file payloads at rest via `ATTR_ENCRYPTED`.
    #[inline]
    pub const fn encrypts_at_rest(self) -> bool {
        !matches!(self, Self::Public)
    }

    /// Classifies a filesystem path into its security tier:
    /// - Private app configs (`/pref/app/*`) and user vaults (`/user/vault/*`) encrypt at rest.
    /// - Boot/UI preferences (`/pref/*.cfg`), system calibration/keys (`/sys/*`),
    ///   executables (`/apps/*`), media, and large random-access radio tables remain `Public`
    ///   (or hardware-MAC authenticated like `/sys/calib.sig`) so locked boots never corrupt UI state.
    pub fn classify_path(path: &str) -> Self {
        if path.starts_with("/pref/app/") || path.starts_with("/user/vault/") {
            Self::UserAuthRead
        } else {
            Self::Public
        }
    }
}

#[cfg(test)]
mod tests {
    use super::FileSecurityLevel;

    #[test]
    fn path_classification_separates_boot_prefs_from_private_app_vaults() {
        assert_eq!(
            FileSecurityLevel::classify_path("/pref/display.cfg"),
            FileSecurityLevel::Public
        );
        assert_eq!(
            FileSecurityLevel::classify_path("/sys/calib.dat"),
            FileSecurityLevel::Public
        );
        assert_eq!(
            FileSecurityLevel::classify_path("/apps/morse.krx"),
            FileSecurityLevel::Public
        );
        assert_eq!(
            FileSecurityLevel::classify_path("/pref/app/secret.cfg"),
            FileSecurityLevel::UserAuthRead
        );
        assert_eq!(
            FileSecurityLevel::classify_path("/user/vault/keys.dat"),
            FileSecurityLevel::UserAuthRead
        );
        assert!(!FileSecurityLevel::Public.encrypts_at_rest());
        assert!(FileSecurityLevel::DeviceBound.encrypts_at_rest());
        assert!(FileSecurityLevel::UserAuthRead.encrypts_at_rest());
    }
}
