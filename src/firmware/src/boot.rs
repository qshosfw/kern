use crate::services::SystemStorage;
pub mod orchestration;
#[cfg(feature = "filesystem")]
pub mod provision;
#[cfg(feature = "panic_gui")]
pub mod recovery;
#[cfg(feature = "passcode")]
pub mod security;
pub mod splash;

pub use orchestration::*;

use crate::services::radio::RadioMelodyExt;
#[cfg(feature = "filesystem")]
use drivers::filesystem::FileSystemDriver;
use drivers::keyboard::KeypadScanner;
#[cfg(feature = "filesystem")]
use drivers::py25q16::Py25q16FlashDriver;
use drivers::st7565::St7565;
use kernel::event::{KeyCode, KeyState};
#[cfg(feature = "filesystem")]
use kernfs::KernFS;

#[derive(Clone, Copy)]
pub struct BootAlert {
    pub title: &'static str,
    pub message: &'static str,
}

static mut PENDING_ALERT: Option<BootAlert> = None;

pub fn queue_alert(title: &'static str, message: &'static str) {
    unsafe {
        *core::ptr::addr_of_mut!(PENDING_ALERT) = Some(BootAlert { title, message });
    }
}

#[inline(never)]
pub fn boot_wait_key(key_scanner: &mut KeypadScanner, timeout_ms: u32) -> Option<KeyCode> {
    let start = py32_hal::delay::millis();
    loop {
        bsp::Board::wdt_feed();
        if let Some((code, KeyState::Pressed)) = key_scanner.scan() {
            return Some(code);
        }
        if timeout_ms > 0 && py32_hal::delay::millis().wrapping_sub(start) >= timeout_ms {
            return None;
        }
        py32_hal::delay::delay_ms(10);
    }
}

#[cfg(feature = "usb")]
#[inline(never)]
pub fn boot_wait_key_with_usb(
    key_scanner: &mut KeypadScanner,
    usb: &mut drivers::usb::UsbDriver,
    timeout_ms: u32,
) -> Option<KeyCode> {
    let start = py32_hal::delay::millis();
    loop {
        bsp::Board::wdt_feed();
        let _ = usb.poll();
        if let Some((code, KeyState::Pressed)) = key_scanner.scan() {
            return Some(code);
        }
        if timeout_ms > 0 && py32_hal::delay::millis().wrapping_sub(start) >= timeout_ms {
            return None;
        }
        py32_hal::delay::delay_ms(1);
    }
}

#[cfg(feature = "crash_dialog")]
#[inline(never)]
pub fn draw_crash_dialog(
    canvas: &mut gfx::Canvas,
    slot: &kernel::boot::BootSlot,
    countdown_secs: Option<u8>,
) {
    canvas.clear();

    // 1. Inverted Header Bar (minimalist and generic: "SYSTEM")
    canvas.fill_rect(0, 0, 128, 10, gfx::Color::Fill);
    gfx::text::draw_str_align(
        canvas,
        64,
        2,
        "SYSTEM",
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Clear,
        gfx::text::Align::Center,
    );

    // 2. Natural language explanation
    let mut detail_buf = [0u8; 32];
    let detail_str = slot.natural_detail(&mut detail_buf);
    let lines = [
        "Radio restarted unexpectedly",
        slot.natural_cause(),
        detail_str,
        "Operational state restored.",
    ];
    let ys = [13i16, 24, 34, 45];
    for i in 0..4 {
        gfx::text::draw_str(
            canvas,
            4,
            ys[i],
            lines[i],
            &gfx::fonts::tomthumb::FONT,
            gfx::Color::Fill,
        );
    }
    canvas.draw_horizontal_line(4, 21, 120, gfx::Color::Fill);

    // 3. Action Badges
    gfx::widgets::draw_key_badge(canvas, 4, 55, "OK", "Dismiss");

    let mut exit_buf = [b'0', b's'];
    let exit_label = match countdown_secs {
        Some(s) => {
            exit_buf[0] = b'0' + s.min(9);
            unsafe { core::str::from_utf8_unchecked(&exit_buf) }
        }
        None => "Close",
    };
    gfx::widgets::draw_key_badge_right(canvas, 124, 55, "EXIT", exit_label);
}

#[cfg(feature = "crash_dialog")]
#[inline(never)]
pub fn show_crash_dialog_if_needed(hw: &mut BootHardware) {
    let crash_slot = crate::panic::BOOT_RECORD.get().latest_valid_slot().copied();
    let crash_count = crash_slot.as_ref().map_or(0, |s| s.crash_count);

    // Intentional reboots and normal cold boots have crash_count == 0 or non-crash slots.
    // Only display warning dialog if an unexpected crash actually occurred!
    if crash_count > 0 {
        if let Some(ref slot) = crash_slot {
            if !slot.is_crash() {
                return;
            }

            hw.backlight.wake();
            hw.radio
                .play_beep(crate::services::melody::BeepType::Warning);
            bsp::set_status_led(bsp::StatusLed::Orange);

            // Ensure keypad is quiet and no key is held
            hw.key_scanner.clear();
            while drivers::keyboard::KeypadScanner::poll_raw_mask() != 0 {
                bsp::Board::wdt_feed();
                py32_hal::delay::delay_ms(10);
            }
            hw.key_scanner.clear();

            let canvas = crate::system_canvas();
            let mut sec = 5u8;
            while sec > 0 {
                draw_crash_dialog(canvas, slot, Some(sec));
                hw.lcd.blit(canvas);

                #[cfg(feature = "usb")]
                let key = boot_wait_key_with_usb(hw.key_scanner, hw.usb, 1000);
                #[cfg(not(feature = "usb"))]
                let key = boot_wait_key(hw.key_scanner, 1000);

                if key.is_some() {
                    hw.radio.play_beep(crate::services::melody::BeepType::Click);
                    break;
                }
                sec -= 1;
            }

            bsp::set_status_led(bsp::StatusLed::Off);
            crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
        }
    }
}

#[inline(never)]
pub fn show_queued_alerts(hw: &mut BootHardware) {
    #[cfg(feature = "crash_dialog")]
    show_crash_dialog_if_needed(hw);

    let alert = unsafe {
        let ptr = core::ptr::addr_of_mut!(PENDING_ALERT);
        let a = *ptr;
        *ptr = None;
        a
    };

    let Some(alert) = alert else {
        return;
    };

    hw.backlight.wake();
    hw.radio
        .play_beep(crate::services::melody::BeepType::Warning);

    let canvas = crate::system_canvas();
    canvas.clear();
    gfx::widgets::draw_sheet_frame(canvas, alert.title, 10, 44, None);
    gfx::text::draw_str_align(
        canvas,
        64,
        24,
        alert.title,
        &gfx::fonts::mumatrix_regular::FONT,
        gfx::Color::Fill,
        gfx::text::Align::Center,
    );
    gfx::text::draw_str_align(
        canvas,
        64,
        38,
        alert.message,
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Fill,
        gfx::text::Align::Center,
    );
    hw.lcd.blit(canvas);

    if boot_wait_key(hw.key_scanner, 2500).is_some() {
        hw.radio.play_beep(crate::services::melody::BeepType::Click);
    }
}

#[cfg(feature = "filesystem")]
pub fn render_dialog(
    lcd: &mut St7565,
    big_title: &str,
    sub_title: &str,
    hint: &str,
    has_btns: bool,
) {
    let canvas = crate::system_canvas();
    gfx::widgets::draw_dialog(canvas, big_title, sub_title, hint, has_btns);
    lcd.blit(canvas);
}

#[cfg(feature = "filesystem")]
pub fn render_progress(lcd: &mut St7565, big_title: &str, sub_title: &str, progress_percent: u8) {
    let canvas = crate::system_canvas();
    gfx::widgets::draw_dialog_progress(canvas, big_title, sub_title, progress_percent);
    lcd.blit(canvas);
}

#[cfg(feature = "filesystem")]
fn rescue_raw_calibration<'a>(fs: &mut KernFS<Py25q16FlashDriver<'a>>, raw_calib: &mut [u8; 512]) {
    let valid_at_10k = fs.dev.read_data(0x010000, raw_calib).is_ok()
        && crate::services::calibration::is_valid_calibration(raw_calib);
    if !valid_at_10k {
        raw_calib.copy_from_slice(crate::services::calibration::DEV_TARGET_FACTORY_CALIB);
    }
}

#[cfg(feature = "filesystem")]
fn save_rescued_calibration<'a>(
    fs: &mut KernFS<Py25q16FlashDriver<'a>>,
    raw_calib: &[u8; 512],
    radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
) {
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
fn prompt_and_format_storage<'a>(
    lcd: &mut St7565,
    key_scanner: &mut KeypadScanner,
    radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
    fs: &mut KernFS<Py25q16FlashDriver<'a>>,
) {
    bsp::pins::BACKLIGHT.set_high();
    lcd.turn_on();
    radio.set_status_led(bsp::StatusLed::Orange);

    render_dialog(
        lcd,
        "Uninitialized Storage",
        "Ensure calib is backed up",
        "Press MENU to Format Storage",
        true,
    );

    loop {
        match boot_wait_key(key_scanner, 2500) {
            Some(KeyCode::Menu) | None => break,
            Some(KeyCode::Exit) => {
                render_dialog(
                    lcd,
                    "Boot Cancelled",
                    "System Halted",
                    "Power cycle device to retry",
                    false,
                );
                loop {
                    bsp::Board::wdt_feed();
                }
            }
            _ => {}
        }
    }

    let raw_calib = crate::services::staging_buf();
    rescue_raw_calibration(fs, raw_calib);

    let mut last_rendered_progress = 0u8;
    let _ = fs.format(Some(&mut |progress| {
        bsp::Board::wdt_feed();
        if progress >= last_rendered_progress + 5 || progress == 100 {
            last_rendered_progress = progress;
            render_progress(
                lcd,
                "Formatting Flash...",
                "Erasing & initializing",
                progress,
            );
        }
    }));

    render_progress(lcd, "Porting Calibration", "Writing /sys/calib.dat...", 95);
    save_rescued_calibration(fs, raw_calib, radio);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn recover_calibration_to_fs<'a>(
    lcd: &mut St7565,
    radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
    fs: &mut KernFS<Py25q16FlashDriver<'a>>,
) {
    bsp::pins::BACKLIGHT.set_high();
    lcd.turn_on();
    radio.set_status_led(bsp::StatusLed::Orange);
    render_progress(
        lcd,
        "Calibration Recovery",
        "Restoring /sys/calib.dat...",
        50,
    );

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
    lcd: &mut St7565,
    key_scanner: &mut KeypadScanner,
    filesystem: &mut FileSystemDriver<'a, Py25q16FlashDriver<'a>>,
    radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
) {
    let fs = &mut filesystem.fs;

    if fs.mount().is_err() {
        prompt_and_format_storage(lcd, key_scanner, radio, fs);
    } else if !fs.exists("/sys/calib.dat") && !fs.exists("/sys/calib.bak") {
        recover_calibration_to_fs(lcd, radio, fs);
    }
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn report_step(lcd: &mut Option<&mut St7565>, step: &'static str, pct: u8) {
    if let Some(display) = lcd {
        render_progress(display, "System Setup", step, pct);
    }
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn ensure_default_files(filesystem: &mut SystemStorage, mut lcd: Option<&mut St7565>) {
    if !filesystem.exists("/sys/init.done") {
        #[cfg(feature = "boot_assets")]
        {
            report_step(&mut lcd, "Provisioning boot assets...", 20);
            splash::provision_splashes(filesystem);
        }

        #[cfg(feature = "provision_rogers")]
        {
            report_step(&mut lcd, "Provisioning roger beeps...", 40);
            crate::services::melody::provision_rogers(filesystem);
        }

        #[cfg(feature = "boot_jingle")]
        {
            report_step(&mut lcd, "Provisioning boot jingles...", 55);
            crate::services::melody::provision_jingles(filesystem);
        }

        report_step(&mut lcd, "Verifying security keychain...", 70);
        crate::security::ensure_keychain(filesystem);

        report_step(&mut lcd, "Installing core apps...", 90);
        provision::provision_packages(filesystem);

        let _ = filesystem.write_file("/sys/init.done", b"INIT");
        report_step(&mut lcd, "Initialization Complete", 100);
        py32_hal::delay::delay_ms(150);
    }

    // Always verify that core system packages in /apps/ are provisioned and current
    provision::ensure_core_packages(filesystem);
}

#[cfg(not(feature = "filesystem"))]
pub fn mount_or_format<'a>(
    _lcd: &mut St7565,
    _key_scanner: &mut KeypadScanner,
    _radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
) -> crate::services::storage::NoStorage {
    crate::services::storage::NoStorage
}

#[cfg(not(feature = "filesystem"))]
#[allow(dead_code)]
pub fn ensure_default_files(_filesystem: &mut SystemStorage, _lcd: Option<&mut St7565>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use kernel::boot::{BOOT_RECORD_IDENT, BootSlot, ResetReason};

    #[test]
    #[cfg(feature = "crash_dialog")]
    fn test_draw_crash_dialog_renders_elements() {
        let mut canvas = gfx::Canvas::new();
        let mut slot = BootSlot::empty();
        slot.reset_reason = ResetReason::Appf as u32;
        slot.set_actor(b"main.rs");
        slot.panic_line = 142;
        slot.crash_count = 1;
        slot.ident = BOOT_RECORD_IDENT;
        slot.crc32 = slot.compute_crc();

        draw_crash_dialog(&mut canvas, &slot, Some(5));

        // Header bar in page 0 must have fill pixels
        assert_ne!(canvas.buffer[0][64], 0);

        // Body area in pages 2..5 must have rendered text pixels
        assert_ne!(canvas.buffer[2][10], 0);
        assert_ne!(canvas.buffer[3][10], 0);
        assert_ne!(canvas.buffer[4][10], 0);

        // Action badges in page 7 must have rendered pixels
        assert_ne!(canvas.buffer[7][10], 0);
        assert_ne!(canvas.buffer[7][120], 0);
    }

    #[test]
    #[cfg(feature = "crash_dialog")]
    fn test_draw_crash_dialog_hardfault_and_watchdog() {
        let mut canvas = gfx::Canvas::new();
        let mut slot = BootSlot::empty();
        slot.reset_reason = ResetReason::Hard as u32;
        slot.set_actor(b"DACCVIOL");
        slot.fault_pc = 0x0800_2468;
        slot.crash_count = 1;
        slot.ident = BOOT_RECORD_IDENT;
        slot.crc32 = slot.compute_crc();

        draw_crash_dialog(&mut canvas, &slot, None);
        assert_ne!(canvas.buffer[0][64], 0);
        assert_ne!(canvas.buffer[3][10], 0);

        // Watchdog timeout test
        canvas.clear();
        slot.reset_reason = ResetReason::Wdog as u32;
        slot.set_actor(b"WATCHDOG");
        slot.crc32 = slot.compute_crc();

        draw_crash_dialog(&mut canvas, &slot, None);
        assert_ne!(canvas.buffer[0][64], 0);
        assert_ne!(canvas.buffer[3][10], 0);
    }

    #[test]
    #[cfg(feature = "crash_dialog")]
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
