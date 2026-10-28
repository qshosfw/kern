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
#[cfg(feature = "filesystem")]
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

#[inline(never)]
pub fn show_queued_alerts(hw: &mut BootHardware) {
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
#[inline(never)]
fn prompt_and_format_storage<'a>(
    lcd: &mut St7565,
    key_scanner: &mut KeypadScanner,
    radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
    fs: &mut KernFS<Py25q16FlashDriver<'a>>,
) {
    bsp::pins::BACKLIGHT.set_high();
    lcd.turn_on();
    // Mount or calibration lookup failed. Turn ON Golden Orange status LED (Red + Green LEDs ON)
    radio.set_status_led(bsp::StatusLed::Orange);

    // Enter blocking UI loop to confirm format.
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

    // Read raw factory calibration from SPI Flash 0x010000 (Sector 16 / EEPROM 0x1E00) to rescue it
    let raw_calib = crate::services::staging_buf();
    let read_ok = fs.dev.read_data(0x010000, raw_calib).is_ok();
    if !(read_ok && crate::services::calibration::is_valid_calibration(raw_calib)) {
        raw_calib.copy_from_slice(crate::services::calibration::DEV_TARGET_FACTORY_CALIB);
    }

    // --- Step 2: Format Flash with Progress Bar ---
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

    // --- Step 3: Restore Factory Calibration ---
    render_progress(lcd, "Porting Calibration", "Writing /sys/calib.dat...", 95);

    let _ = fs.write_file("/sys/calib.dat", raw_calib);
    let _ = fs.write_file("/sys/calib.bak", raw_calib);

    let mcu_uid = bsp::uid::read_mcu_uid();
    let sig = crate::services::calibration::compute_device_signature(raw_calib, &mcu_uid);
    let _ = fs.write_file("/sys/calib.sig", &sig.to_bytes());

    // Turn OFF Yellow Status LED
    radio.set_status_led(bsp::StatusLed::Off);
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
    let mut restored = false;

    // 1. Try reading /sys/calib.bak
    if fs.exists("/sys/calib.bak") {
        if let Ok(len) = fs.read_file("/sys/calib.bak", raw_calib) {
            if len == 512 && crate::services::calibration::is_valid_calibration(raw_calib) {
                restored = true;
            }
        }
    }

    // 2. Try raw SPI Flash 0x010000 (Sector 16 / EEPROM 0x1E00)
    if !restored {
        if fs.dev.read_data(0x010000, raw_calib).is_ok()
            && crate::services::calibration::is_valid_calibration(raw_calib)
        {
            restored = true;
        }
    }

    // 3. Try /prov/calib.dat
    if !restored && fs.exists("/prov/calib.dat") {
        if let Ok(len) = fs.read_file("/prov/calib.dat", raw_calib) {
            if len == 512 && crate::services::calibration::is_valid_calibration(raw_calib) {
                restored = true;
            }
        }
    }

    // 4. Fallback to factory default calibration
    if !restored {
        raw_calib.copy_from_slice(crate::services::calibration::DEV_TARGET_FACTORY_CALIB);
    }

    let _ = fs.create_dir("/sys");
    let _ = fs.write_file("/sys/calib.dat", raw_calib);
    let _ = fs.write_file("/sys/calib.bak", raw_calib);

    // Re-sign with device MCU UID to /sys/calib.sig
    let mcu_uid = bsp::uid::read_mcu_uid();
    let sig = crate::services::calibration::compute_device_signature(raw_calib, &mcu_uid);
    let _ = fs.write_file("/sys/calib.sig", &sig.to_bytes());

    radio.set_status_led(bsp::StatusLed::Off);
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
    if filesystem.exists("/sys/init.done") {
        return;
    }

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
