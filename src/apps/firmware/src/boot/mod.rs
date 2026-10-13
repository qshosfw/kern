use crate::services::SystemStorage;
pub mod orchestration;
pub mod recovery;
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
fn render_dialog(lcd: &mut St7565, big_title: &str, sub_title: &str, hint: &str, has_btns: bool) {
    let canvas = crate::system_canvas();
    gfx::widgets::draw_dialog(canvas, big_title, sub_title, hint, has_btns);
    lcd.blit(canvas);
}

#[cfg(feature = "filesystem")]
fn render_progress(lcd: &mut St7565, big_title: &str, sub_title: &str, progress_percent: u8) {
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
    // Mount or calibration lookup failed. Turn ON Yellow status LED (Red + Green LEDs ON)
    radio.set_status_led(bsp::StatusLed::Yellow);

    // Enter blocking UI loop to confirm format.
    render_dialog(
        lcd,
        "Uninitialized Storage",
        "Ensure calib is backed up",
        "Press MENU to Format Storage",
        true,
    );

    loop {
        match boot_wait_key(key_scanner, 0) {
            Some(KeyCode::Menu) => break,
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
    let mut raw_calib = [0u8; 512];
    let read_ok = fs.dev.read_data(0x010000, &mut raw_calib).is_ok();
    if !(read_ok && crate::services::calibration::is_valid_calibration(&raw_calib)) {
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

    let _ = fs.write_file("/sys/calib.dat", &raw_calib);

    // Turn OFF Yellow Status LED
    radio.set_status_led(bsp::StatusLed::Off);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn mount_or_format<'a>(
    lcd: &mut St7565,
    key_scanner: &mut KeypadScanner,
    flash: &'a mut Py25q16FlashDriver<'a>,
    radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
) -> FileSystemDriver<'a, Py25q16FlashDriver<'a>> {
    let mut fs = KernFS::new(flash);

    if fs.mount().is_err() || !fs.exists("/sys/calib.dat") {
        prompt_and_format_storage(lcd, key_scanner, radio, &mut fs);
    }

    FileSystemDriver::new(fs)
}

#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn ensure_default_files(vfs: &mut SystemStorage) {
    // Provision boot splash assets into /user/splash
    splash::provision_splashes(vfs);

    // Provision roger tones into /user/rogers
    crate::services::melody::provision_rogers(vfs);

    crate::security::ensure_keychain(vfs);
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
pub fn ensure_default_files(_vfs: &mut SystemStorage) {}
