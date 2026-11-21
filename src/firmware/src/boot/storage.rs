//! Boot stage 3 — storage: mount (or guarded re-format), calibration rescue, first-boot
//! provisioning and the crash-log flush.
//!
//! An attended radio with unreadable storage asks for a deliberate hold-to-format; an
//! unattended (headless) node never erases flash on its own and boots unmounted so the
//! fault can be inspected and repaired over RigWire.

use crate::services::SystemStorage;
#[cfg(feature = "filesystem")]
use drivers::filesystem::FileSystemDriver;
use drivers::keyboard::KeypadScanner;
#[cfg(feature = "filesystem")]
use drivers::py25q16::Py25q16FlashDriver;
use drivers::st7565::St7565;
#[cfg(feature = "filesystem")]
use kernel::event::{KeyCode, KeyState};
#[cfg(feature = "filesystem")]
use kernfs::KernFS;

#[cfg(feature = "filesystem")]
use super::backup;

/// Append a valid crash `BootSlot` (128 B) into the `/log/crash.bin` 1 KB ring (`8 x 128 B`).
#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn flush_crash_log_if_pending(fs: &mut SystemStorage) {
    let mut tag = backup::read();
    if (tag.flags & kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH) == 0 {
        return;
    }
    let Some(slot) = crate::panic::BOOT_RECORD.get().latest_valid_slot() else {
        return;
    };
    if !slot.is_crash() {
        return;
    }

    let _ = fs.create_dir("/log");
    let ring = &mut crate::data::scratch_tail_bytes_mut()[..1024];
    let existing_len = fs.read_file("/log/crash.bin", ring).unwrap_or(0);
    let slot_bytes = unsafe {
        core::slice::from_raw_parts(
            slot as *const kernel::boot::BootSlot as *const u8,
            core::mem::size_of::<kernel::boot::BootSlot>(),
        )
    };
    let write_len = if existing_len + 128 <= ring.len() {
        ring[existing_len..existing_len + 128].copy_from_slice(slot_bytes);
        existing_len + 128
    } else {
        for i in 0..(1024 - 128) {
            ring[i] = ring[i + 128];
        }
        ring[1024 - 128..1024].copy_from_slice(slot_bytes);
        1024
    };
    let _ = fs.write_file("/log/crash.bin", &ring[..write_len]);

    tag.flags &= !kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH;
    backup::write(tag);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn render_progress(
    lcd: &mut St7565<bsp::LcdBus>,
    big_title: &str,
    sub_title: &str,
    progress_percent: u8,
) {
    let canvas = crate::system_canvas();
    gfx::widgets::draw_dialog_progress(canvas, big_title, sub_title, progress_percent);
    lcd.blit(canvas);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn render_hold_prompt(
    lcd: &mut St7565<bsp::LcdBus>,
    big_title: &str,
    sub_title: &str,
    progress_percent: u8,
) {
    let canvas = crate::system_canvas();
    gfx::widgets::draw_dialog_hold_prompt(
        canvas,
        big_title,
        sub_title,
        "Hold",
        "Cancel",
        progress_percent,
    );
    lcd.blit(canvas);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn rescue_raw_calibration(
    fs: &mut KernFS<Py25q16FlashDriver<bsp::FlashBus>>,
    raw_calib: &mut [u8; 512],
) -> bool {
    let valid_at_10k = fs.dev.read_data(0x010000, raw_calib).is_ok()
        && crate::services::calibration::is_valid_calibration(raw_calib);
    if !valid_at_10k {
        crate::services::calibration::fill_default_calibration(raw_calib);
    }
    valid_at_10k
}

#[cfg(feature = "filesystem")]
fn has_valid_calib_file(fs: &mut KernFS<Py25q16FlashDriver<bsp::FlashBus>>) -> bool {
    let buf = crate::services::staging_buf();
    (fs.read_file("/sys/calib.dat", buf) == Ok(512)
        && crate::services::calibration::is_valid_calibration(buf))
        || (fs.read_file("/sys/calib.bak", buf) == Ok(512)
            && crate::services::calibration::is_valid_calibration(buf))
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn save_rescued_calibration(
    fs: &mut KernFS<Py25q16FlashDriver<bsp::FlashBus>>,
    raw_calib: &mut [u8; 512],
    radio: &mut drivers::bk4829::Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
) {
    if !crate::services::calibration::is_valid_calibration(raw_calib) {
        crate::services::calibration::fill_default_calibration(raw_calib);
    }
    let _ = fs.create_dir("/sys");
    let _ = fs.write_file("/sys/calib.dat", raw_calib);
    let _ = fs.write_file("/sys/calib.bak", raw_calib);

    let mcu_uid = bsp::uid::read_mcu_uid();
    let sig = crate::services::calibration::compute_device_signature(raw_calib, &mcu_uid);
    let _ = fs.write_file("/sys/calib.sig", &sig.to_bytes());
    radio.set_status_led(bsp::StatusLed::Off);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn wait_keys_released(key_scanner: &mut KeypadScanner) {
    while KeypadScanner::poll_matrix().is_some() {
        bsp::Board::wdt_feed();
        py32_hal::delay::delay_ms(10);
    }
    KeypadScanner::reset_gpio_pins();
    key_scanner.clear();
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn prepare_boot_dialog(
    lcd: &mut St7565<bsp::LcdBus>,
    radio: &mut drivers::bk4829::Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
) {
    // Set non-zero PWM duty alongside GPIO high so 20 kHz SysTick PWM does not zero BACKLIGHT.
    drivers::backlight::BACKLIGHT_DUTY.store(10, core::sync::atomic::Ordering::Relaxed);
    bsp::pins::BACKLIGHT.set_high();
    lcd.turn_on();
    radio.set_status_led(bsp::StatusLed::Orange);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn halt_with_boot_dialog(lcd: &mut St7565<bsp::LcdBus>, title: &str, sub: &str, hint: &str) -> ! {
    let canvas = crate::system_canvas();
    gfx::widgets::draw_dialog(canvas, title, sub, hint, false);
    lcd.blit(canvas);
    loop {
        bsp::Board::wdt_feed();
        py32_hal::delay::delay_ms(50);
    }
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn prompt_and_format_storage(
    lcd: &mut St7565<bsp::LcdBus>,
    key_scanner: &mut KeypadScanner,
    radio: &mut drivers::bk4829::Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
    fs: &mut KernFS<Py25q16FlashDriver<bsp::FlashBus>>,
) {
    let raw_calib = crate::services::staging_buf();
    let has_hw_calib = rescue_raw_calibration(fs, raw_calib);
    let calib_sub = if has_hw_calib {
        "Calibration is safe"
    } else {
        "No calibration found"
    };

    // Blit initial frame before enabling LCD/backlight to prevent GRAM glitch.
    render_hold_prompt(lcd, "Storage unreadable", calib_sub, 0);
    prepare_boot_dialog(lcd, radio);

    // Wait for any already-held keys (e.g. Exit held during reset/wake) to be released first.
    wait_keys_released(key_scanner);

    let mut hold = gfx::gui::animation::HoldProgress::new();
    hold.required_ms = 1500;
    let mut last_ms = py32_hal::delay::millis();
    let mut last_pct = 0u8;

    loop {
        bsp::Board::wdt_feed();
        while let Some((code, state)) = key_scanner.scan() {
            match (code, state) {
                (KeyCode::Menu | KeyCode::Key5, KeyState::Pressed | KeyState::Held) => {
                    hold.active = true;
                }
                (KeyCode::Menu | KeyCode::Key5, KeyState::Released) => {
                    hold.active = false;
                }
                (KeyCode::Exit, KeyState::Pressed) => {
                    halt_with_boot_dialog(
                        lcd,
                        "Startup stopped",
                        "Storage left as-is",
                        "Turn off & on to retry",
                    );
                }
                _ => {}
            }
        }

        let now = py32_hal::delay::millis();
        let dt = now.wrapping_sub(last_ms).min(100) as u16;
        last_ms = now;
        if dt > 0 {
            if hold.advance(dt) {
                break;
            }
            let pct = hold.percent();
            if pct != last_pct {
                last_pct = pct;
                render_hold_prompt(lcd, "Storage unreadable", calib_sub, pct);
            }
        }
        py32_hal::delay::delay_ms(10);
    }

    let mut last_rendered_progress = 0u8;
    let fmt_res = fs.format(Some(&mut |progress| {
        bsp::Board::wdt_feed();
        if progress >= last_rendered_progress + 5 || progress == 100 {
            last_rendered_progress = progress;
            render_progress(lcd, "Preparing storage", "Erasing, keep power on", progress);
        }
    }));

    if fmt_res.is_err() {
        halt_with_boot_dialog(
            lcd,
            "Storage not responding",
            "Flash chip may be faulty",
            "Turn off & on to retry",
        );
    }

    render_progress(lcd, "Restoring calibration", "Almost done", 95);
    save_rescued_calibration(fs, raw_calib, radio);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn recover_calibration_to_fs(
    lcd: &mut St7565<bsp::LcdBus>,
    radio: &mut drivers::bk4829::Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
    fs: &mut KernFS<Py25q16FlashDriver<bsp::FlashBus>>,
) {
    render_progress(lcd, "Restoring calibration", "Rebuilding from backup", 50);
    prepare_boot_dialog(lcd, radio);

    let raw_calib = crate::services::staging_buf();
    let from_prov = fs.exists("/prov/calib.dat")
        && fs.read_file("/prov/calib.dat", raw_calib) == Ok(512)
        && crate::services::calibration::is_valid_calibration(raw_calib);
    if !from_prov {
        rescue_raw_calibration(fs, raw_calib);
    }

    save_rescued_calibration(fs, raw_calib, radio);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn mount_or_format<'a>(
    lcd: &mut St7565<bsp::LcdBus>,
    key_scanner: &mut KeypadScanner,
    filesystem: &mut FileSystemDriver<'a, Py25q16FlashDriver<bsp::FlashBus>>,
    radio: &mut drivers::bk4829::Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
) {
    let fs = &mut filesystem.fs;

    if fs.mount().is_err() {
        // An unattended node never blocks on a panel prompt nor erases flash on its own:
        // it boots unmounted and reports the fault over RigWire for a remote decision.
        if crate::system::admits_physical_input() {
            prompt_and_format_storage(lcd, key_scanner, radio, fs);
        }
    } else if !has_valid_calib_file(fs) {
        recover_calibration_to_fs(lcd, radio, fs);
    }
}

#[cfg(feature = "filesystem")]
const INIT_MARK_BASE: u8 = 1 << 0;
#[cfg(feature = "filesystem")]
const INIT_MARK_SPLASHES: u8 = if cfg!(feature = "boot_assets") {
    1 << 1
} else {
    0
};
#[cfg(feature = "filesystem")]
const INIT_MARK_JINGLES: u8 = if cfg!(feature = "boot_jingle") {
    1 << 3
} else {
    0
};

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn ensure_default_files(filesystem: &mut SystemStorage) {
    let desired: u8 = INIT_MARK_BASE | INIT_MARK_SPLASHES | INIT_MARK_JINGLES;

    let mut init_buf = [0u8; 8];
    let done_mask = match filesystem.read_file("/sys/init.done", &mut init_buf) {
        Ok(n) if n >= 5 && &init_buf[..4] == b"INIT" => init_buf[4],
        Ok(n) if n >= 4 && &init_buf[..4] == b"INIT" => INIT_MARK_BASE,
        _ => 0,
    };

    if (done_mask & desired) != desired {
        if (done_mask & INIT_MARK_BASE) == 0 {
            crate::security::ensure_keychain(filesystem);
            let _ = filesystem.create_dir("/apps");
        }
        #[cfg(feature = "boot_assets")]
        if (done_mask & INIT_MARK_SPLASHES) == 0 {
            super::splash::provision_splashes(filesystem);
        }
        #[cfg(feature = "boot_jingle")]
        if (done_mask & INIT_MARK_JINGLES) == 0 {
            crate::services::melody::provision_jingles(filesystem);
        }

        let updated = [b'I', b'N', b'I', b'T', done_mask | desired];
        let _ = filesystem.write_file("/sys/init.done", &updated);
    }
}

#[cfg(not(feature = "filesystem"))]
pub fn mount_or_format(
    _lcd: &mut St7565<bsp::LcdBus>,
    _key_scanner: &mut KeypadScanner,
    _radio: &mut drivers::bk4829::Bk4829<'_, bsp::RadioBus, drivers::bk4829::Disabled>,
) -> crate::services::storage::NoStorage {
    crate::services::storage::NoStorage
}

#[cfg(not(feature = "filesystem"))]
#[allow(dead_code)]
pub fn ensure_default_files(_filesystem: &mut SystemStorage) {}
