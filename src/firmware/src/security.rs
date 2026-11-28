//! KERN Firmware Security Subsystem

pub mod attributes;
#[cfg(feature = "secure_fs")]
pub mod constants;
pub mod kdf;
#[cfg(feature = "secure_fs")]
pub mod keychain;
#[cfg(feature = "passcode")]
pub mod passcode;
pub mod port_lock;

pub use attributes::FileSecurityLevel;
pub use kdf::ProgressHook;
#[cfg(feature = "secure_fs")]
pub use keychain::{
    PackedDeviceKey, PackedPasscodeKey, PackedStorageKey, derive_fs_key, derive_passcode_kek,
    ensure_keychain,
};
#[cfg(feature = "passcode")]
pub use passcode::PasscodeState;
pub use port_lock::set_swd_enabled;

/// Effective storage encryption status after reconciling firmware capabilities with on-flash state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageSecurityStatus {
    Plaintext,
    DeviceBoundActive,
    UnlockedByPasscode,
    LockedByPasscode { encrypted_files: usize },
    EncryptedWithoutEngine { encrypted_files: usize },
}

/// Counts files marked `ATTR_ENCRYPTED` across mounted storage metadata in a single pass.
#[inline(always)]
pub fn count_encrypted_files<D: kernfs::BlockDevice>(
    filesystem: &mut kernfs::KernFS<'_, D>,
) -> usize {
    filesystem.count_encrypted_files()
}

/// Synchronizes `KernFS` session key/lock state with the active security tier.
pub fn sync_filesystem_security<D: kernfs::BlockDevice>(
    filesystem: &mut kernfs::KernFS<'_, D>,
    state: &PasscodeState,
) -> StorageSecurityStatus {
    #[cfg(not(feature = "filesystem"))]
    {
        let _ = (filesystem, state);
        StorageSecurityStatus::Plaintext
    }

    #[cfg(all(feature = "filesystem", feature = "passcode"))]
    {
        if state.is_locked {
            filesystem.lock_session();
            StorageSecurityStatus::LockedByPasscode { encrypted_files: 0 }
        } else {
            let uid = bsp::uid::read_mcu_uid();
            let mut fs_key = keychain::derive_fs_key(state.master_key.as_slice(), &uid);
            filesystem.set_encryption_key(fs_key);
            fs_key.fill(0);
            StorageSecurityStatus::UnlockedByPasscode
        }
    }

    #[cfg(all(
        feature = "filesystem",
        feature = "secure_fs",
        not(feature = "passcode")
    ))]
    {
        let _ = state;
        if passcode::is_passcode_set(filesystem) {
            filesystem.lock_session();
            let encrypted_files = count_encrypted_files(filesystem);
            StorageSecurityStatus::LockedByPasscode { encrypted_files }
        } else if keychain::unlock_device_storage(filesystem) {
            StorageSecurityStatus::DeviceBoundActive
        } else {
            filesystem.lock_session();
            let encrypted_files = count_encrypted_files(filesystem);
            StorageSecurityStatus::LockedByPasscode { encrypted_files }
        }
    }

    #[cfg(all(feature = "filesystem", not(feature = "secure_fs")))]
    {
        let _ = state;
        let encrypted_files = count_encrypted_files(filesystem);
        if passcode::is_passcode_set(filesystem) {
            filesystem.lock_session();
            StorageSecurityStatus::LockedByPasscode { encrypted_files }
        } else if encrypted_files > 0 {
            filesystem.lock_session();
            StorageSecurityStatus::EncryptedWithoutEngine { encrypted_files }
        } else {
            filesystem.unlock_session();
            StorageSecurityStatus::Plaintext
        }
    }
}

/// Applies `ATTR_ENCRYPTED` to small confidential files (`FileSecurityLevel::classify_path`)
/// when `secure_fs` is compiled and unlocked.
pub fn write_classified_file<D: kernfs::BlockDevice>(
    filesystem: &mut kernfs::KernFS<'_, D>,
    path: &str,
    data: &[u8],
) -> Result<(), kernfs::FsError> {
    filesystem.write_file(path, data)?;
    #[cfg(feature = "secure_fs")]
    if FileSecurityLevel::classify_path(path).encrypts_at_rest()
        && data.len() <= 512
        && filesystem.is_session_unlocked()
    {
        let perm = filesystem.get_permission(path).unwrap_or(0);
        if (perm & kernfs::ATTR_ENCRYPTED) == 0 {
            let _ = filesystem.set_permission(path, perm | kernfs::ATTR_ENCRYPTED);
            filesystem.write_file(path, data)?;
        }
    }
    Ok(())
}

/// Formats a human-friendly notification describing why storage access is restricted.
pub fn format_storage_alert_message(
    status: StorageSecurityStatus,
    out: &mut utils::format::InlineStr<88>,
) -> Option<&'static str> {
    out.clear();
    let mut num_buf = [0u8; 12];
    let (title, count, mid, tail) = match status {
        StorageSecurityStatus::LockedByPasscode { encrypted_files: 0 } => (
            "Storage Protected",
            0,
            "",
            "Passcode set; Flash a Passcode build to unlock.",
        ),
        StorageSecurityStatus::LockedByPasscode { encrypted_files } => (
            "Storage Protected",
            encrypted_files,
            if encrypted_files == 1 {
                " encrypted file"
            } else {
                " encrypted files"
            },
            ", passcode unlock disabled.",
        ),
        StorageSecurityStatus::EncryptedWithoutEngine { encrypted_files } => (
            "Encrypted Storage",
            encrypted_files,
            if encrypted_files == 1 {
                " file is"
            } else {
                " files are"
            },
            " encrypted; Secure Storage disabled.",
        ),
        _ => return None,
    };
    if count > 0 {
        let _ = out.push_str(gfx::text::format_u32(count as u32, &mut num_buf));
    }
    let _ = out.push_str(mid);
    let _ = out.push_str(tail);
    Some(title)
}

#[cfg(not(feature = "passcode"))]
pub mod passcode {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PasscodeError {
        InvalidLength,
        WrongPasscode,
        LockedOut,
        StorageError,
    }

    pub struct PasscodeState {
        pub is_locked: bool,
    }

    impl PasscodeState {
        pub const fn empty() -> Self {
            Self { is_locked: false }
        }

        pub const fn new() -> Self {
            Self { is_locked: false }
        }

        #[inline(always)]
        pub fn lock(&mut self) {
            self.is_locked = true;
        }

        #[inline(always)]
        pub fn unlock(&mut self) {
            self.is_locked = false;
        }
    }

    #[inline(always)]
    pub fn is_passcode_set<D: kernfs::BlockDevice>(filesystem: &mut kernfs::KernFS<'_, D>) -> bool {
        let mut hdr = [0u8; 6];
        if let Ok(n) = filesystem.read_file("/sys/passcode.key", &mut hdr) {
            return n >= 6 && &hdr[..4] == b"PIN1" && (hdr[4] & 1) != 0;
        }
        false
    }

    #[inline(always)]
    pub fn init_passcode_state<D: kernfs::BlockDevice>(
        filesystem: &mut kernfs::KernFS<'_, D>,
        state: &mut PasscodeState,
    ) {
        state.unlock();
        let _ = super::sync_filesystem_security(filesystem, state);
    }
}

#[cfg(not(feature = "passcode"))]
pub use passcode::PasscodeState;

#[cfg(not(feature = "secure_fs"))]
#[inline(always)]
pub fn ensure_keychain<D: kernfs::BlockDevice>(_filesystem: &mut kernfs::KernFS<'_, D>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_alert_formatting_covers_singular_plural_and_unlocked() {
        let mut msg = utils::format::InlineStr::<88>::new();
        assert!(format_storage_alert_message(StorageSecurityStatus::Plaintext, &mut msg).is_none());
        assert!(
            format_storage_alert_message(StorageSecurityStatus::DeviceBoundActive, &mut msg)
                .is_none()
        );
        assert!(
            format_storage_alert_message(StorageSecurityStatus::UnlockedByPasscode, &mut msg)
                .is_none()
        );

        let title = format_storage_alert_message(
            StorageSecurityStatus::LockedByPasscode { encrypted_files: 0 },
            &mut msg,
        );
        assert_eq!(title, Some("Storage Protected"));
        assert!(msg.as_str().contains("Flash a Passcode build"));

        let title = format_storage_alert_message(
            StorageSecurityStatus::LockedByPasscode { encrypted_files: 1 },
            &mut msg,
        );
        assert_eq!(title, Some("Storage Protected"));
        assert!(msg.as_str().contains("1 encrypted file,"));

        let title = format_storage_alert_message(
            StorageSecurityStatus::EncryptedWithoutEngine { encrypted_files: 2 },
            &mut msg,
        );
        assert_eq!(title, Some("Encrypted Storage"));
        assert!(msg.as_str().contains("2 files are encrypted"));
    }

    #[cfg(feature = "secure_fs")]
    #[test]
    fn derive_fs_key_is_uid_and_master_key_bound() {
        let mk_a = [0x11u8; 32];
        let mut mk_b = [0x11u8; 32];
        mk_b[31] ^= 0x01;
        let uid_a = [0xAAu8; 12];
        let mut uid_b = [0xAAu8; 12];
        uid_b[0] ^= 0x80;

        let k1 = keychain::derive_fs_key(&mk_a, &uid_a);
        let k2 = keychain::derive_fs_key(&mk_a, &uid_b);
        let k3 = keychain::derive_fs_key(&mk_b, &uid_a);

        assert_ne!(k1, k2);
        assert_ne!(k1, k3);
        assert_ne!(k1, mk_a[..16]);
    }
}
