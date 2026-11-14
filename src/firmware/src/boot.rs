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

#[cfg(feature = "crash_dialog")]
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
static CRASH_DIALOG_LINES: [gfx::widgets::TextLine; 2] = [
    gfx::widgets::TextLine::new(13, 0, 0, "Radio restarted unexpectedly"),
    gfx::widgets::TextLine::new(45, 0, 0, "Operational state restored."),
];

#[cfg(feature = "crash_dialog")]
#[inline(never)]
pub fn draw_crash_dialog(
    canvas: &mut gfx::Canvas,
    slot: &kernel::boot::BootSlot,
    countdown_secs: Option<u8>,
) {
    gfx::widgets::draw_text_screen(canvas, "SYSTEM", &CRASH_DIALOG_LINES, None);

    // 2. Dynamic cause and forensic detail
    let mut detail_buf = [0u8; 32];
    let detail_str = slot.natural_detail(&mut detail_buf);
    let dyn_lines = [(24i16, slot.natural_cause()), (34i16, detail_str)];
    for &(y, s) in &dyn_lines {
        gfx::text::draw_str(
            canvas,
            4,
            y,
            s,
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
        }
    }
}

/// Append a valid crash `BootSlot` (128 B) into the `/log/crash.bin` 1 KB ring (`8 x 128 B`).
#[cfg(feature = "filesystem")]
#[inline(never)]
pub fn flush_crash_log_if_pending(fs: &mut SystemStorage) {
    let mut tag = crate::boot::read_backup_tag();
    if (tag.flags & kernel::boot::BackupCrashTag::FLAG_DIRTY_CRASH) == 0 {
        return;
    }
    let Some(slot) = crate::panic::BOOT_RECORD.get().latest_valid_slot() else {
        return;
    };
    if !slot.is_crash() {
        return;
    }

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
    crate::boot::write_backup_tag(tag);
}

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
    lcd: &mut St7565,
    radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
) {
    // Set non-zero PWM duty alongside GPIO high so 20 kHz SysTick PWM does not zero BACKLIGHT.
    drivers::backlight::BACKLIGHT_DUTY.store(10, core::sync::atomic::Ordering::Relaxed);
    bsp::pins::BACKLIGHT.set_high();
    lcd.turn_on();
    radio.set_status_led(bsp::StatusLed::Orange);
}

#[cfg(feature = "filesystem")]
#[inline(never)]
fn prompt_and_format_storage<'a>(
    lcd: &mut St7565,
    key_scanner: &mut KeypadScanner,
    radio: &mut drivers::bk4829::Bk4829<drivers::bk4829::Disabled>,
    fs: &mut KernFS<Py25q16FlashDriver<'a>>,
) {
    prepare_boot_dialog(lcd, radio);

    render_dialog(
        lcd,
        "Uninitialized Storage",
        "Ensure calib is backed up",
        "Press MENU to Format Storage",
        true,
    );

    // Wait for any already-held keys (e.g. Exit held during reset/wake) to be released first.
    wait_keys_released(key_scanner);

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
    prepare_boot_dialog(lcd, radio);
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
const INIT_MARK_BASE: u8 = 1 << 0;
#[cfg(feature = "filesystem")]
const INIT_MARK_SPLASHES: u8 = if cfg!(feature = "boot_assets") {
    1 << 1
} else {
    0
};
#[cfg(feature = "filesystem")]
const INIT_MARK_ROGERS: u8 = if cfg!(feature = "provision_rogers") {
    1 << 2
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
    let desired: u8 = INIT_MARK_BASE | INIT_MARK_SPLASHES | INIT_MARK_ROGERS | INIT_MARK_JINGLES;

    let mut init_buf = [0u8; 8];
    let done_mask = match filesystem.read_file("/sys/init.done", &mut init_buf) {
        Ok(n) if n >= 5 && &init_buf[..4] == b"INIT" => init_buf[4],
        Ok(n) if n >= 4 && &init_buf[..4] == b"INIT" => INIT_MARK_BASE,
        _ => 0,
    };

    if (done_mask & desired) != desired {
        if (done_mask & INIT_MARK_BASE) == 0 {
            crate::security::ensure_keychain(filesystem);
        }
        #[cfg(feature = "boot_assets")]
        if (done_mask & INIT_MARK_SPLASHES) == 0 {
            splash::provision_splashes(filesystem);
        }
        #[cfg(feature = "provision_rogers")]
        if (done_mask & INIT_MARK_ROGERS) == 0 {
            crate::services::melody::provision_rogers(filesystem);
        }
        #[cfg(feature = "boot_jingle")]
        if (done_mask & INIT_MARK_JINGLES) == 0 {
            crate::services::melody::provision_jingles(filesystem);
        }

        let updated = [b'I', b'N', b'I', b'T', done_mask | desired];
        let _ = filesystem.write_file("/sys/init.done", &updated);
    }

    // Verify core /apps/*.krx packages, install any staged /prov/* items, and refresh launcher catalog
    provision::provision_packages(filesystem);
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
pub fn ensure_default_files(_filesystem: &mut SystemStorage) {}

/// Read the factory bootloader version string (e.g. `"7.02.02"`) directly from internal MCU Flash
/// (`0x0800_1F00..0x0800_2700`), falling back to `"7.02.02"` on host unit-test targets.
pub fn bootloader_version_str() -> &'static str {
    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        let rom = core::slice::from_raw_parts(0x0800_1F00 as *const u8, 0x0800);
        let mut i = 0usize;
        while i + 8 <= rom.len() {
            let w = &rom[i..i + 8];
            if w[1] == b'.'
                && w[4] == b'.'
                && w[7] == 0
                && w[0].is_ascii_digit()
                && w[2].is_ascii_digit()
                && w[3].is_ascii_digit()
                && w[5].is_ascii_digit()
                && w[6].is_ascii_digit()
            {
                return core::str::from_utf8_unchecked(&w[..7]);
            }
            i += 1;
        }
    }
    "7.02.02"
}

#[inline(always)]
pub fn is_bootloader_hold_requested() -> bool {
    (read_backup_tag().flags & kernel::boot::BackupCrashTag::FLAG_BOOTLOADER_HOLD) != 0
}

/// Board-aware reboot handler registered with `kernel::power::PowerControl`.
pub fn firmware_reboot_handler(mode: kernel::power::RebootMode) -> ! {
    match mode {
        kernel::power::RebootMode::Firmware => prepare_clean_reboot(),
        _ => {
            let extra = if matches!(mode, kernel::power::RebootMode::Bootloader) {
                kernel::boot::BackupCrashTag::FLAG_BOOTLOADER_HOLD
            } else {
                kernel::boot::BackupCrashTag::FLAG_SAFE_MODE_REQ
            };
            let mut tag = read_backup_tag();
            tag.flags |= extra | kernel::boot::BackupCrashTag::FLAG_CLEAN_REBOOT;
            write_backup_tag(tag);
        }
    }
    kernel::power::PowerControl::reboot_raw()
}

/// Enter the factory bootloader (`0x0800_0000`) at boot BEFORE `Watchdog::start()` is called
/// so `IWDG` is completely disabled and never resets the radio during DFU flashing.
#[inline(never)]
pub fn enter_dfu_bootloader(lcd: &mut St7565) -> ! {
    let mut tag = read_backup_tag();
    tag.flags = (tag.flags & !kernel::boot::BackupCrashTag::FLAG_BOOTLOADER_HOLD)
        | kernel::boot::BackupCrashTag::FLAG_CLEAN_REBOOT;
    write_backup_tag(tag);

    bsp::set_status_led(bsp::StatusLed::Off);
    drivers::backlight::BACKLIGHT_DUTY.store(10, core::sync::atomic::Ordering::Relaxed);
    bsp::pins::BACKLIGHT.set_high();
    lcd.turn_on();
    let canvas = crate::system_canvas();
    gfx::widgets::draw_dfu_screen(canvas, bootloader_version_str());
    lcd.blit(canvas);

    handoff_to_factory_bootloader();
}

/// Hand execution directly to the stock factory bootloader (`0x0800_0000`) while preserving
/// GPIO state (`PF8` LCD backlight HIGH, ST7565 GDDRAM intact, and `PB10` PTT locked LOW)
/// and cleanly tearing down firmware clocks, timers, UART/SPI peripherals, and SysTick.
#[inline(never)]
fn handoff_to_factory_bootloader() -> ! {
    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        cortex_m::interrupt::disable();

        // 1. Lock PB3..PB6 (KB_COLS) and PB14..PB15 (KB_ROWS[0..1]) as Input Pull-Up and
        //    PB10 (PTT) as Open-Drain Output LOW via GPIOB->LCKR (0x5000_041C).
        //    Why PB3..PB6 too? In GPIO_Init (0x0800_0688), the stock bootloader drives
        //    PB3..PB6 LOW and checks at 0x0800_1384 that PB14/PB15 are HIGH before checking
        //    PTT (PB10). If the user is still holding MENU (PB6 <-> PB15), an unlocked PB6
        //    driven LOW would pull PB15 LOW and abort DFU entry! Locking PB3..PB6 as Input
        //    Pull-Up ensures PB14/PB15 always read HIGH even while MENU is held.
        let gpiob = py32_pac::gpio::GPIOB_BASE as *mut u32;
        let ptt_pin = 1u32 << 10;
        let lock_pins = 0xC478u32; // PB3..PB6 (0x0078) | PB10 (0x0400) | PB14..PB15 (0xC000)
        let mask_2b = 0xF030_3FC0u32;
        gpiob.byte_add(0x28).write_volatile(ptt_pin);
        let otyper = gpiob.byte_add(0x04).read_volatile();
        gpiob.byte_add(0x04).write_volatile(otyper | ptt_pin);
        let pupdr = gpiob.byte_add(0x0C).read_volatile();
        gpiob
            .byte_add(0x0C)
            .write_volatile((pupdr & !mask_2b) | 0x5000_1540);
        let moder = gpiob.read_volatile();
        gpiob.write_volatile((moder & !mask_2b) | (0b01 << 20));
        let lckr = gpiob.byte_add(0x1C);
        let lckk = (1u32 << 16) | lock_pins;
        lckr.write_volatile(lckk);
        lckr.write_volatile(lock_pins);
        lckr.write_volatile(lckk);
        let _ = lckr.read_volatile();

        // 2. Switch SYSCLK back to direct HSI (SW = 0) and disable PLL directly via RCC
        //    registers (do NOT call set_sysclk_24mhz, which re-enables SysTick and USART1!).
        let rcc = py32_pac::rcc::RCC_BASE as *mut u32;
        rcc.byte_add(0x08).write_volatile(0);
        while (rcc.byte_add(0x08).read_volatile() & 0x38) != 0 {}
        let cr = rcc.read_volatile();
        rcc.write_volatile(cr & !py32_pac::rcc::CR_PLLON);

        // 3. Reset DMA1, APB1, and APB2 peripherals (clearing USART1->CR1.UE so the stock
        //    bootloader USART_Init at 0x0800_0CF8 does not abort!) while keeping RCC_IOPENR
        //    (PF8 BACKLIGHT HIGH, PB2 LCD_CS HIGH, and PB10 PTT locked LOW) intact.
        rcc.byte_add(0x28).write_volatile(1);
        rcc.byte_add(0x28).write_volatile(0);
        rcc.byte_add(0x2C).write_volatile(0xFFFF_FFFF);
        rcc.byte_add(0x2C).write_volatile(0);
        rcc.byte_add(0x30).write_volatile(0xFFFF_FFFF);
        rcc.byte_add(0x30).write_volatile(0);
        rcc.byte_add(0x3C).write_volatile(0);
        rcc.byte_add(0x40).write_volatile(0);

        // 4. Stop SysTick (bootloader vec[15] at 0x0800_00E4 is an infinite hang loop!),
        //    clear PENDSTCLR and all NVIC enables/pending, and set VTOR = 0x0800_0000.
        let scs = 0xE000_E000usize as *mut u32;
        scs.byte_add(0x010).write_volatile(0);
        scs.byte_add(0xD04).write_volatile(1 << 25);
        scs.byte_add(0x180).write_volatile(0xFFFF_FFFF);
        scs.byte_add(0x280).write_volatile(0xFFFF_FFFF);
        scs.byte_add(0xD08).write_volatile(0x0800_0000);

        let msp = (0x0800_0000 as *const u32).read_volatile();
        let entry = (0x0800_0004 as *const u32).read_volatile();
        core::arch::asm!(
            "dsb",
            "isb",
            "msr msp, {msp}",
            "cpsie i",
            "bx {entry}",
            msp = in(reg) msp,
            entry = in(reg) entry,
            options(noreturn)
        );
    }
    #[cfg(not(all(not(test), target_arch = "arm")))]
    {
        loop {
            core::hint::spin_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "crash_dialog")]
    use super::*;
    #[cfg(feature = "crash_dialog")]
    use kernel::boot::{BOOT_RECORD_IDENT, BootSlot, ResetReason};
    use kernel::boot::{BackupCrashTag, BootRecord, TrapCode};

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
