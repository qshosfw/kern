//! KERN Firmware Security Subsystem

pub mod attributes;
pub mod auth;
pub mod constants;
pub mod kdf;
pub mod keychain;
pub mod passcode;
pub mod port_lock;

pub use attributes::FileSecurityLevel;
pub use auth::{AuthToken, TrustScope, authenticate, check_authorization};
pub use kdf::ProgressHook;
pub use keychain::{
    PackedDeviceKey, PackedPasscodeKey, PackedStorageKey, derive_passcode_kek, ensure_keychain,
};
pub use passcode::PasscodeState;
pub use port_lock::set_swd_enabled;
