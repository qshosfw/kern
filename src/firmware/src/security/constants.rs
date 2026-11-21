//! Security Domain Separation Constants, Format Identifiers, and Defaults

pub const FILE_ID_DEVICE_KEY: &[u8; 4] = b"DEV1";
pub const FILE_ID_PASSCODE_KEY: &[u8; 4] = b"PIN1";
pub const FILE_ID_STORAGE_KEY: &[u8; 4] = b"STO1";

/// Default Passcode Seed (when no passcode is set)
pub const DEFAULT_PASSCODE_SEED: &[u8] = b"0000";

/// Standard PBKDF Key Stretching Rounds for Flash Key Containers
pub const DEFAULT_PBKDF_ITERATIONS: u32 = 8192;
pub const LEGACY_PBKDF_ITERATIONS: u32 = 8192;

/// Default Lockout Threshold for Passcode Verification Failure
pub const DEFAULT_MAX_TRIES: u8 = 5;

pub const PATH_DEVICE_KEY: &str = "/sys/device.key";
pub const PATH_PASSCODE_KEY: &str = "/sys/passcode.key";
pub const PATH_STORAGE_KEY: &str = "/sys/storage.key";
pub const KEY_PATHS: [&str; 3] = [PATH_DEVICE_KEY, PATH_PASSCODE_KEY, PATH_STORAGE_KEY];
