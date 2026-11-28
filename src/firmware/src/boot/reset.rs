//! Boot stage 1 — reset-cause introspection and the backup-domain crash ledger.
//!
//! Runs before any peripheral other than the clock tree is touched. It classifies the
//! previous reset (clean reboot, watchdog, brownout, fault), folds the outcome into the
//! `BootRecord` / backup-tag strike counters, and owns the matching clean-reboot path.

use super::backup;
use kernel::boot::BackupCrashTag;

/// Classify the previous reset and update the crash ledger accordingly.
pub fn check_reset_cause() {
    let raw_csr = py32_hal::wdt::Watchdog::raw_reset_flags();
    let reset_cause = py32_hal::wdt::Watchdog::reset_cause();
    py32_hal::wdt::Watchdog::clear_reset_flags();

    let mut tag = backup::read();
    let clean_reboot = (tag.flags & BackupCrashTag::FLAG_CLEAN_REBOOT) != 0;

    if clean_reboot {
        // Intentional reboot: preserve strike counter unless cleared after >=10s healthy uptime,
        // clear CLEAN_REBOOT bit so unexpected resets next boot are caught.
        backup::update(0, BackupCrashTag::FLAG_CLEAN_REBOOT);
        return;
    }

    // Never overwrite a valid HardFault / Appf BOOT_RECORD slot with a PC=0 placeholder.
    let existing = crate::panic::BOOT_RECORD.get().latest_valid_slot();
    let is_watchdog_reset = matches!(
        reset_cause,
        py32_hal::wdt::ResetCause::IndependentWatchdog | py32_hal::wdt::ResetCause::WindowWatchdog
    );
    let is_brownout = matches!(reset_cause, py32_hal::wdt::ResetCause::PowerOn)
        && ((raw_csr & py32_pac::rcc::CSR_PWRRSTF) != 0 || tag.reason_code != 0);

    if is_watchdog_reset && existing.is_none_or(|s| s.crash_count == 0) {
        let slot = crate::panic::BOOT_RECORD.get_mut().begin_write();
        slot.reset_reason = kernel::boot::ResetReason::Wdog as u32;
        slot.set_actor(b"WATCHDOG");
        slot.panic_file_hash = raw_csr;
        slot.crash_count = slot.crash_count.max(tag.strikes.saturating_add(1));
        tag.strikes = slot.crash_count;
        tag.reason_code = 3;
        tag.flags |= BackupCrashTag::FLAG_DIRTY_CRASH;
        crate::panic::BOOT_RECORD.get_mut().commit_write();
        backup::write(tag);
    } else if is_brownout && existing.is_none_or(|s| s.crash_count == 0) && tag.strikes > 0 {
        let slot = crate::panic::BOOT_RECORD.get_mut().begin_write();
        slot.reset_reason = kernel::boot::ResetReason::Brow as u32;
        slot.set_actor(b"BROWNOUT");
        slot.panic_file_hash = raw_csr;
        slot.crash_count = tag.strikes;
        crate::panic::BOOT_RECORD.get_mut().commit_write();
    } else if let Some(s) = existing {
        if s.crash_count > tag.strikes {
            tag.strikes = s.crash_count;
            tag.flags |= BackupCrashTag::FLAG_DIRTY_CRASH;
            backup::write(tag);
        }
    }
}

/// Mark the backup-domain reboot tag before an intentional software reset.
pub fn prepare_clean_reboot() {
    backup::update(BackupCrashTag::FLAG_CLEAN_REBOOT, 0);
    crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
}

/// Reset both `.uninit` crash-loop strike counter and backup-domain strikes after healthy uptime.
pub fn clear_crash_strikes() {
    crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
    let mut tag = backup::read();
    tag.strikes = 0;
    tag.flags &= !(BackupCrashTag::FLAG_DIRTY_CRASH | BackupCrashTag::FLAG_SAFE_MODE_REQ);
    tag.reason_code = 0;
    backup::write(tag);
}

/// Board-aware reboot handler registered with `kernel::power::PowerControl`.
pub fn firmware_reboot_handler(mode: kernel::power::RebootMode) -> ! {
    use kernel::power::RebootMode;
    match mode {
        RebootMode::Firmware => prepare_clean_reboot(),
        #[cfg(not(feature = "krx_native"))]
        RebootMode::Bootloader => prepare_clean_reboot(),
        #[cfg(feature = "krx_native")]
        RebootMode::Bootloader => backup::update(
            BackupCrashTag::FLAG_BOOTLOADER_HOLD | BackupCrashTag::FLAG_CLEAN_REBOOT,
            0,
        ),
        RebootMode::SafeRecovery => backup::update(
            BackupCrashTag::FLAG_SAFE_MODE_REQ | BackupCrashTag::FLAG_CLEAN_REBOOT,
            0,
        ),
    }
    kernel::power::PowerControl::reboot_raw()
}
