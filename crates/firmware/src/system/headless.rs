//! Headless node mode.
//!
//! A headless node is an unattended radio (junction box, mast, remote site) whose only
//! control plane is the RigWire link over USB CDC or UART. While headless:
//!
//! * the panel sleeps and the backlight is latched dark (`BacklightController::hold_dark`),
//!   so no keypress, syscall, dialog or config apply can light it;
//! * every physical input path (keypad events, app key syscalls, boot chords, boot prompts)
//!   is discarded — wire-injected keys remain the single source of interaction;
//! * deep sleep / APO are inhibited, because STOP mode severs USB and needs a physical wake.
//!
//! The mode survives warm resets through `BackupCrashTag::FLAG_HEADLESS` (valid before
//! storage is mounted) and total power loss through `/pref/keys.cfg` (`KeyConfig::headless`).

use crate::boot::backup;
use crate::hardware::SystemHardware;
use core::sync::atomic::{AtomicBool, Ordering};
use kernel::boot::BackupCrashTag;

static HEADLESS: AtomicBool = AtomicBool::new(false);

#[inline(always)]
pub fn is_headless() -> bool {
    HEADLESS.load(Ordering::Relaxed)
}

/// Physical keys, chords and panel prompts are honoured only on an attended radio.
#[inline(always)]
pub fn admits_physical_input() -> bool {
    !is_headless()
}

/// Latch the in-memory flag and the backup-domain bit without touching peripherals.
#[inline(never)]
fn latch(enable: bool) {
    HEADLESS.store(enable, Ordering::Relaxed);
    let bit = BackupCrashTag::FLAG_HEADLESS;
    backup::update(if enable { bit } else { 0 }, bit);
}

/// Single transition point: latch the mode and drive the panel to match it. Leaving
/// headless repaints and lights the panel only when `wake_panel` (a live transition).
#[inline(never)]
fn engage(hw: &mut SystemHardware, enable: bool, wake_panel: bool) {
    latch(enable);
    hw.backlight.hold_dark(enable);
    if enable {
        hw.lcd.enter_sleep();
    } else if wake_panel {
        hw.lcd.exit_sleep();
        hw.render_frame();
        hw.backlight.wake();
    }
}

/// Enter or leave headless mode at runtime and persist the choice across power loss.
#[inline(never)]
pub fn set_headless(hw: &mut SystemHardware, enable: bool) {
    let was = is_headless();
    let cfg = crate::master_config();
    if cfg.keys.headless != enable as u8 {
        cfg.keys.headless = enable as u8;
        cfg.mark_keys_dirty();
        let _ = cfg.flush_dirty(hw.filesystem);
    }
    engage(hw, enable, was);
    hw.key_scanner.clear();
}

/// Earliest boot stage (storage not yet mounted): honour the warm-reset backup bit so boot
/// chords and prompts are already locked out before `/pref` is readable.
#[inline(never)]
pub fn restore_from_backup(hw: &mut SystemHardware) {
    if backup::has(BackupCrashTag::FLAG_HEADLESS) {
        HEADLESS.store(true, Ordering::Relaxed);
        hw.backlight.hold_dark(true);
    }
}

/// After `/pref` is loaded: `keys.cfg` is the durable source of truth.
#[inline(never)]
pub fn restore_from_config(hw: &mut SystemHardware) {
    engage(hw, crate::master_config_ref().keys.headless != 0, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headless_latch_mirrors_into_backup_domain_and_back() {
        let before = backup::read();
        latch(true);
        assert!(is_headless());
        assert!(!admits_physical_input());
        assert_ne!(backup::read().flags & BackupCrashTag::FLAG_HEADLESS, 0);

        latch(false);
        assert!(admits_physical_input());
        let tag = backup::read();
        assert_eq!(tag.flags & BackupCrashTag::FLAG_HEADLESS, 0);
        // Crash bookkeeping in the same register is left untouched.
        assert_eq!(tag.strikes, before.strikes);
    }
}
