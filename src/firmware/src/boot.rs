//! Boot pipeline.
//!
//! Boot is a strict, linear sequence of stages. Each stage owns exactly one concern, runs
//! once, and hands a well-defined machine state to the next:
//!
//! | # | Stage            | Module            | Establishes                                        |
//! |---|------------------|-------------------|----------------------------------------------------|
//! | 0 | Early hardware   | `main::early_init`| clocks, SRAM state, VTOR, interrupts, board GPIO   |
//! | 1 | Reset cause      | [`reset`]         | crash ledger, strike counters, clean-reboot intent |
//! | 2 | Panel & control  | [`run`]           | LCD/backlight, reboot handler, headless lock-out   |
//! | 3 | Recovery         | [`recovery`]      | bootloader-hold / chord → boot-module chain / DFU  |
//! | 4 | Watchdog         | [`run`]           | IWDG armed — every later stage must make progress  |
//! | 5 | Storage          | [`storage`]       | mounted KernFS, calibration present                |
//! | 6 | System           | [`orchestration`] | config, security gate, services, splash, handoff   |
//!
//! Only the backup-domain tag ([`backup`]) carries state across resets before storage
//! exists; every stage that reads or edits it goes through that module.

pub mod backup;
pub mod orchestration;
#[cfg(feature = "krx_native")]
pub mod recovery;
pub mod reset;
#[cfg(feature = "passcode")]
pub mod security;
pub mod splash;
pub mod storage;

pub use reset::clear_crash_strikes;

/// Stages 2–6. Returns the ADC handle owned by the steady-state system loop.
#[cfg(not(test))]
#[inline(never)]
pub fn run(hw: &mut crate::hardware::SystemHardware) -> py32_hal::adc::Adc {
    /// Independent watchdog window once recovery is behind us (ms).
    const WATCHDOG_TIMEOUT_MS: u32 = 6000;

    // ── Stage 2: panel & control plane ──────────────────────────────────────────────────
    #[cfg(feature = "krx_native")]
    let warm_tools = recovery::is_warm_tools_boot();
    #[cfg(not(feature = "krx_native"))]
    let warm_tools = false;
    if warm_tools {
        // Warm DFU → Recovery: panel and backlight stay live, no 42 ms LCD reset flash.
        hw.backlight.wake();
    } else {
        // Panel stays blank until the passcode prompt or splash reveals it.
        hw.lcd.init();
        bsp::pins::BACKLIGHT.set_low();
        let canvas = crate::system_canvas();
        canvas.clear();
        hw.lcd.blit(canvas);
        hw.backlight.init();
    }
    kernel::power::PowerControl::set_reboot_handler(reset::firmware_reboot_handler);
    // Lock out boot chords, prompts and the backlight before anything reads the keypad.
    crate::system::restore_from_backup(hw);

    // ── Stage 3: recovery ───────────────────────────────────────────────────────────────
    #[cfg(feature = "krx_native")]
    recovery::run(hw, warm_tools);
    if warm_tools {
        hw.lcd.init();
    }

    // ── Stage 4: watchdog ───────────────────────────────────────────────────────────────
    py32_hal::wdt::Watchdog::start(WATCHDOG_TIMEOUT_MS);

    // ── Stage 5: storage ────────────────────────────────────────────────────────────────
    #[cfg(feature = "filesystem")]
    storage::mount_or_format(
        &mut hw.lcd,
        &mut hw.key_scanner,
        hw.filesystem,
        &mut hw.radio,
    );
    bsp::Board::wdt_feed();

    // ── Stage 6: system ─────────────────────────────────────────────────────────────────
    orchestration::boot_orchestration(hw)
}

#[cfg(test)]
mod tests {
    use kernel::boot::{BOOT_RECORD_IDENT, BackupCrashTag, BootRecord, BootSlot, TrapCode};

    #[test]
    fn test_backup_crash_tag_roundtrip_and_trap_codes() {
        let tag = BackupCrashTag {
            strikes: 3,
            flags: BackupCrashTag::FLAG_DIRTY_CRASH | BackupCrashTag::FLAG_SAFE_MODE_REQ,
            reason_code: 1,
        };
        let decoded = BackupCrashTag::decode(tag.encode()).expect("valid ident byte");
        assert_eq!(decoded, tag);
        assert_eq!(BackupCrashTag::decode(0), None);
        assert_eq!(TrapCode::from_u8(0x02), Some(TrapCode::KernelLock));
        assert_eq!(TrapCode::from_u8(0xFE), Some(TrapCode::RustPanic));
        let rec = BootRecord::new();
        assert!(rec.latest_valid_slot().is_none());
    }

    #[test]
    fn test_intentional_reboot_is_not_crash() {
        let mut slot = BootSlot::empty();
        // Clean boot / intentional reboot has crash_count == 0 and empty slot
        assert_eq!(slot.crash_count, 0);
        assert!(!slot.is_crash());

        // Even if slot has ident but reason is None or crash_count is 0
        slot.ident = BOOT_RECORD_IDENT;
        slot.reset_reason = 0;
        slot.crash_count = 0;
        slot.crc32 = slot.compute_crc();
        assert_eq!(slot.crash_count, 0);
        assert!(!slot.is_crash());
    }
}
