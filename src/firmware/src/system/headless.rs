//! Headless node mode.
//!
//! A headless node is an unattended radio (junction box, mast, remote site) whose only
//! control plane is the RigWire link over USB CDC or UART. While headless:
//!
//! * the panel sleeps, the backlight is latched dark (`BacklightController::hold_dark`),
//!   the flashlight is forced off, and the local speaker amplifier stays disabled (tones
//!   route to the RigWire audio ring instead);
//! * physical input events (keypad presses, app key syscalls, boot chords, boot prompts)
//!   are discarded — wire-injected keys remain the single source of interaction, except
//!   for the deliberate physical escape chord (`MENU + EXIT` held for 3 seconds);
//! * deep sleep / APO and on-screen low-battery modal popups are inhibited, because STOP
//!   mode severs USB and unattended nodes report battery state via telemetry.
//!
//! The mode survives warm resets through `BackupCrashTag::FLAG_HEADLESS` (valid before
//! storage is mounted) and total power loss through `/pref/keys.cfg` (`KeyConfig::headless`).

use crate::boot::backup;
use crate::hardware::SystemHardware;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering};
use kernel::boot::BackupCrashTag;

static HEADLESS: AtomicBool = AtomicBool::new(false);
static HEADLESS_NEEDS_SAVE: AtomicBool = AtomicBool::new(false);
static HEADLESS_WAKE_RENDER: AtomicBool = AtomicBool::new(false);

/// Deliberate physical escape chord hold duration (`MENU + EXIT` held simultaneously).
pub const ESCAPE_HOLD_MS: u16 = 3_000;
static ESCAPE_HOLD_ACCUM_MS: AtomicU16 = AtomicU16::new(0);
static ESCAPE_WAIT_RELEASE: AtomicBool = AtomicBool::new(false);

/// Fixed 8-slot (32 B) zero-alloc ring buffer for headless audio/beep routing over RigWire.
const HEADLESS_TONE_CAP: usize = 8;
static mut HEADLESS_TONE_RING: [(u16, u16); HEADLESS_TONE_CAP] = [(0, 0); HEADLESS_TONE_CAP];
static HEADLESS_TONE_HEAD: AtomicU8 = AtomicU8::new(0);
static HEADLESS_TONE_TAIL: AtomicU8 = AtomicU8::new(0);

/// Queue tone/melody notes for RigWire playback instead of driving the local speaker.
#[inline(never)]
pub fn push_headless_notes(notes: &[drivers::audio::ToneNote]) {
    for n in notes {
        let head = HEADLESS_TONE_HEAD.load(Ordering::Relaxed);
        let next = (head + 1) & ((HEADLESS_TONE_CAP as u8) - 1);
        let tail = HEADLESS_TONE_TAIL.load(Ordering::Acquire);
        if next == tail {
            // Drop oldest note if host is not draining so ring never stalls.
            HEADLESS_TONE_TAIL.store(
                (tail + 1) & ((HEADLESS_TONE_CAP as u8) - 1),
                Ordering::Release,
            );
        }
        unsafe {
            HEADLESS_TONE_RING[head as usize] = (n.freq_hz, n.duration_ms);
        }
        HEADLESS_TONE_HEAD.store(next, Ordering::Release);
    }
}

/// Drain queued headless audio notes into `out` as packed `(u16 freq_hz LE, u16 duration_ms LE)` tuples.
#[inline(never)]
pub fn drain_headless_notes(out: &mut [u8]) -> usize {
    let mut pos = 0usize;
    while pos + 4 <= out.len() {
        let tail = HEADLESS_TONE_TAIL.load(Ordering::Relaxed);
        let head = HEADLESS_TONE_HEAD.load(Ordering::Acquire);
        if tail == head {
            break;
        }
        let (freq, dur) = unsafe { HEADLESS_TONE_RING[tail as usize] };
        HEADLESS_TONE_TAIL.store(
            (tail + 1) & ((HEADLESS_TONE_CAP as u8) - 1),
            Ordering::Release,
        );
        out[pos..pos + 2].copy_from_slice(&freq.to_le_bytes());
        out[pos + 2..pos + 4].copy_from_slice(&dur.to_le_bytes());
        pos += 4;
    }
    pos
}

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

/// Pure state step for the physical escape chord (`MENU + EXIT` held for `ESCAPE_HOLD_MS`).
#[inline(always)]
pub(super) fn step_physical_escape(accum_ms: &mut u16, dt_ms: u16, combo_held: bool) -> bool {
    if combo_held {
        *accum_ms = accum_ms.saturating_add(dt_ms);
        if *accum_ms >= ESCAPE_HOLD_MS {
            *accum_ms = 0;
            return true;
        }
    } else {
        *accum_ms = 0;
    }
    false
}

/// Advance the physical escape chord timer while headless.
/// Returns `true` once `MENU + EXIT` has been held continuously for `ESCAPE_HOLD_MS`.
#[inline(never)]
pub(super) fn poll_physical_escape(dt_ms: u16, combo_held: bool) -> bool {
    let mut accum = ESCAPE_HOLD_ACCUM_MS.load(Ordering::Relaxed);
    let fired = step_physical_escape(&mut accum, dt_ms, combo_held);
    ESCAPE_HOLD_ACCUM_MS.store(accum, Ordering::Relaxed);
    if fired {
        ESCAPE_WAIT_RELEASE.store(true, Ordering::Relaxed);
    }
    fired
}

/// Swallow residual key events right after a physical escape chord until all keys are released,
/// preventing `MENU` or `EXIT` from triggering UI actions as the user lifts their fingers.
#[inline(always)]
pub(super) fn waiting_escape_release(any_key_down: bool) -> bool {
    if !ESCAPE_WAIT_RELEASE.load(Ordering::Relaxed) {
        return false;
    }
    if !any_key_down {
        ESCAPE_WAIT_RELEASE.store(false, Ordering::Relaxed);
        return false;
    }
    true
}

/// Single transition point: latch the mode and drive the panel to match it. Leaving
/// headless wakes the panel and schedules a top-level frame repaint (`HEADLESS_WAKE_RENDER`)
/// rather than invoking `hw.render_frame()` inside a deep RigWire / keypad stack frame.
#[inline(never)]
fn engage(hw: &mut SystemHardware, enable: bool, wake_panel: bool) {
    latch(enable);
    ESCAPE_HOLD_ACCUM_MS.store(0, Ordering::Relaxed);
    hw.backlight.hold_dark(enable);
    if enable {
        hw.lcd.enter_sleep();
        hw.audio.disable();
        bsp::pins::FLASHLIGHT.set_low();
    } else {
        // Clear any unconsumed headless tone ring notes when returning to local speaker mode.
        HEADLESS_TONE_TAIL.store(
            HEADLESS_TONE_HEAD.load(Ordering::Acquire),
            Ordering::Release,
        );
        if wake_panel {
            hw.lcd.exit_sleep();
            hw.backlight.wake();
            HEADLESS_WAKE_RENDER.store(true, Ordering::Release);
        }
    }
}

/// Enter or leave headless mode at runtime.
///
/// Warm-reset durability is latched immediately in `BackupCrashTag::FLAG_HEADLESS` (0 stack),
/// while LittleFS `/pref/keys.cfg` persistence and panel repaint are deferred to the top-level
/// event loop (`service_deferred`) so entering/leaving headless while inside a KRX app never
/// stacks filesystem writes or guest `KRX_OP_RENDER` frames onto `dispatch_wire_record`.
#[inline(never)]
pub fn set_headless(hw: &mut SystemHardware, enable: bool) {
    let was = is_headless();
    let cfg = crate::master_config();
    if cfg.keys.headless != enable as u8 {
        cfg.keys.headless = enable as u8;
        cfg.mark_keys_dirty();
        HEADLESS_NEEDS_SAVE.store(true, Ordering::Release);
    }
    engage(hw, enable, was);
    hw.key_scanner.clear();
}

/// Execute deferred `/pref/keys.cfg` flush and wake-render flag at top-level main-loop stack depth.
#[inline(never)]
pub(super) fn service_deferred(hw: &mut SystemHardware, dirty_render: &mut bool) {
    if HEADLESS_NEEDS_SAVE.load(Ordering::Acquire) {
        HEADLESS_NEEDS_SAVE.store(false, Ordering::Release);
        let _ = crate::master_config().flush_dirty(hw.filesystem);
    }
    if HEADLESS_WAKE_RENDER.load(Ordering::Acquire) {
        HEADLESS_WAKE_RENDER.store(false, Ordering::Release);
        *dirty_render = true;
    }
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

    #[test]
    fn headless_audio_ring_round_trips_notes() {
        let mut scratch = [0u8; 64];
        let _ = drain_headless_notes(&mut scratch);
        let notes = [
            drivers::audio::ToneNote::new(880, 40),
            drivers::audio::ToneNote::new(1320, 60),
        ];
        push_headless_notes(&notes);
        let n = drain_headless_notes(&mut scratch);
        assert_eq!(n, 8);
        assert_eq!(u16::from_le_bytes([scratch[0], scratch[1]]), 880);
        assert_eq!(u16::from_le_bytes([scratch[2], scratch[3]]), 40);
        assert_eq!(u16::from_le_bytes([scratch[4], scratch[5]]), 1320);
        assert_eq!(u16::from_le_bytes([scratch[6], scratch[7]]), 60);
        assert_eq!(drain_headless_notes(&mut scratch), 0);
    }

    #[test]
    fn physical_escape_chord_requires_sustained_hold() {
        let mut accum = 0u16;
        // Brief presses never exit headless mode.
        assert!(!step_physical_escape(&mut accum, 500, true));
        assert_eq!(accum, 500);
        // Releasing early immediately resets the counter.
        assert!(!step_physical_escape(&mut accum, 5, false));
        assert_eq!(accum, 0);
        // Holding right up to threshold does not fire early.
        assert!(!step_physical_escape(&mut accum, ESCAPE_HOLD_MS - 1, true));
        assert_eq!(accum, ESCAPE_HOLD_MS - 1);
        // Reaching threshold fires once and resets accumulator.
        assert!(step_physical_escape(&mut accum, 1, true));
        assert_eq!(accum, 0);
    }

    #[test]
    fn escape_release_guard_blocks_until_keys_released() {
        ESCAPE_HOLD_ACCUM_MS.store(0, Ordering::Relaxed);
        ESCAPE_WAIT_RELEASE.store(false, Ordering::Relaxed);
        assert!(!waiting_escape_release(true));
        assert!(poll_physical_escape(ESCAPE_HOLD_MS, true));
        // Still holding MENU+EXIT after escape: events stay suppressed.
        assert!(waiting_escape_release(true));
        // Once all keys are released, normal keypad input resumes.
        assert!(!waiting_escape_release(false));
        assert!(!waiting_escape_release(true));
    }

    #[test]
    fn deferred_headless_flags_clear_atomically() {
        HEADLESS_NEEDS_SAVE.store(true, Ordering::Release);
        HEADLESS_WAKE_RENDER.store(true, Ordering::Release);
        assert!(HEADLESS_NEEDS_SAVE.load(Ordering::Acquire));
        HEADLESS_NEEDS_SAVE.store(false, Ordering::Release);
        assert!(!HEADLESS_NEEDS_SAVE.load(Ordering::Acquire));
        assert!(HEADLESS_WAKE_RENDER.load(Ordering::Acquire));
        HEADLESS_WAKE_RENDER.store(false, Ordering::Release);
        assert!(!HEADLESS_WAKE_RENDER.load(Ordering::Acquire));
    }
}
