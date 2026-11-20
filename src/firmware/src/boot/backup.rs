//! Backup-domain boot tag (`RTC_ALRH/ALRL`): the only state that survives a warm reset
//! before storage is mounted.
//!
//! Every reader and writer goes through this module so flag edits are a single
//! read-modify-write (`update`) instead of ad-hoc decode/encode sequences at each site.

use kernel::boot::BackupCrashTag;

#[cfg(not(all(target_arch = "arm", not(test))))]
static TEST_BACKUP_TAG: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Current tag (`BackupCrashTag::empty()` when the register holds no valid ident).
#[inline(never)]
pub fn read() -> BackupCrashTag {
    #[cfg(all(target_arch = "arm", not(test)))]
    let raw = py32_hal::rtc::RealTimeClock::read_alarm_raw();
    #[cfg(not(all(target_arch = "arm", not(test))))]
    let raw = TEST_BACKUP_TAG.load(core::sync::atomic::Ordering::Relaxed);
    BackupCrashTag::decode(raw).unwrap_or(BackupCrashTag::empty())
}

#[inline(never)]
pub fn write(tag: BackupCrashTag) {
    #[cfg(all(target_arch = "arm", not(test)))]
    py32_hal::rtc::RealTimeClock::write_alarm_raw(tag.encode());
    #[cfg(not(all(target_arch = "arm", not(test))))]
    TEST_BACKUP_TAG.store(tag.encode(), core::sync::atomic::Ordering::Relaxed);
}

/// `true` when every bit of `mask` is set.
#[inline(never)]
pub fn has(mask: u8) -> bool {
    (read().flags & mask) == mask
}

/// Clear `clear`, then set `set`; the register is only written when the flags change.
#[inline(never)]
pub fn update(set: u8, clear: u8) {
    let mut tag = read();
    let flags = (tag.flags & !clear) | set;
    if flags != tag.flags {
        tag.flags = flags;
        write(tag);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_sets_and_clears_without_touching_strikes() {
        write(BackupCrashTag {
            strikes: 2,
            flags: BackupCrashTag::FLAG_BOOTLOADER_HOLD,
            reason_code: 7,
        });
        update(
            BackupCrashTag::FLAG_CLEAN_REBOOT,
            BackupCrashTag::FLAG_BOOTLOADER_HOLD,
        );
        let tag = read();
        assert_eq!(tag.flags, BackupCrashTag::FLAG_CLEAN_REBOOT);
        assert_eq!((tag.strikes, tag.reason_code), (2, 7));
        assert!(has(BackupCrashTag::FLAG_CLEAN_REBOOT));
        assert!(!has(
            BackupCrashTag::FLAG_CLEAN_REBOOT | BackupCrashTag::FLAG_HEADLESS
        ));
        write(BackupCrashTag::empty());
    }
}
