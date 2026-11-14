#![cfg_attr(test, allow(dead_code))]
//! Bare-Metal LCD SPI Framebuffer Blitter, UART Forensics Streamer & Interactive Recovery Loop.

use super::BOOT_RECORD;
use py32_hal as hal;

/// Low-level bare-metal UART crash forensics streamer (§13.4)
#[cfg(feature = "panic_uart")]
pub fn baremetal_uart_dump(slot: &kernel::boot::BootSlot) {
    hal::uart::init();
    if hal::uart::write_line("\r\n=== KERN HARDWARE CRASH FORENSICS ===").is_err() {
        return;
    }
    let _ = hal::uart::write_str("CAUSE:   ");
    let _ = hal::uart::write_line(slot.reason().map_or("UNKNOWN", |r| r.as_str()));

    let dump_hex = |label: &str, val: u32| {
        let mut hbuf = [0u8; 8];
        let _ = hal::uart::write_str(label);
        let _ = hal::uart::write_line(gfx::text::format_hex32(val, &mut hbuf));
    };

    dump_hex("PC:      0x", slot.fault_pc);
    dump_hex("LR:      0x", slot.fault_lr);
    dump_hex("SP:      0x", slot.fault_sp);
    dump_hex("PSR:     0x", slot.xpsr);
    dump_hex("SIG:     0x", slot.forensics.crash_signature);

    let mut nbuf = [0u8; 6];
    let _ = hal::uart::write_str("UNWIND:  ");
    let _ = hal::uart::write_line(gfx::text::format_u8(slot.forensics.unwind_count, &mut nbuf));

    let _ = hal::uart::write_str("IPSR:    ");
    let _ = hal::uart::write_line(gfx::text::format_u8(slot.forensics.ipsr_num, &mut nbuf));

    let _ = hal::uart::write_line("======================================");
}

#[cfg(not(feature = "panic_uart"))]
#[inline(always)]
pub fn baremetal_uart_dump(_slot: &kernel::boot::BootSlot) {}

/// Low-level bare-metal ST7565 SPI framebuffer blit without OS task dependencies
#[cfg(feature = "panic_gui")]
pub fn baremetal_lcd_blit(canvas: &gfx::Canvas) {
    bsp::pins::LCD_CS.set_low();
    bsp::pins::LCD_A0.set_low();
    let _ = hal::spi::write_byte(0x40); // Start line 0

    for page in 0..8 {
        bsp::pins::LCD_A0.set_low();
        const COL_OFFSET: u8 = 4;
        let _ = hal::spi::write_byte(0xB0 | (page as u8 & 0x0F));
        let _ = hal::spi::write_byte(0x10 | (COL_OFFSET >> 4));
        let _ = hal::spi::write_byte(COL_OFFSET & 0x0F);

        bsp::pins::LCD_A0.set_high();
        for col in 0..128 {
            let _ = hal::spi::write_byte(canvas.buffer[page][col]);
        }
    }
    let _ = hal::spi::wait_idle();
    bsp::pins::LCD_CS.set_high();
}

#[inline(never)]
fn render_recovery_frame(canvas: &mut gfx::Canvas, page: u8, cd: Option<u8>, is_k1: bool) {
    canvas.clear();
    if let Some(slot) = BOOT_RECORD.get().latest_valid_slot() {
        crate::boot::recovery::draw_recovery_screen(canvas, slot, page, cd, is_k1);
    }
    baremetal_lcd_blit(canvas);
}

/// Interactive Bare-Metal Recovery Loop with Auto-Reboot Countdown, Key Interrupt, and RED Warning Light
#[cfg(feature = "panic_gui")]
pub fn recovery_loop() -> ! {
    hal::wdt::Watchdog::feed();

    bsp::pins::BACKLIGHT.set_high();
    bsp::pins::FLASHLIGHT.set_low();
    hal::spi::init();

    // Set contrast to optimal value (EV = 31 / 50% default per DeltaFW/Fagci) and turn display ON
    bsp::pins::LCD_CS.set_low();
    bsp::pins::LCD_A0.set_low();
    let _ = hal::spi::write_byte(0x81); // Electronic volume
    let _ = hal::spi::write_byte(31); // Optimal contrast EV 31
    let _ = hal::spi::write_byte(0x40); // Start line 0
    let _ = hal::spi::write_byte(0xAF); // Display ON
    let _ = hal::spi::wait_idle();
    bsp::pins::LCD_CS.set_high();

    let canvas = crate::system_canvas();
    #[allow(unused_mut)]
    let mut page = 0u8;

    let is_k1 = if crate::CONFIG_INITIALIZED.load(core::sync::atomic::Ordering::Acquire) {
        crate::master_config_ref().is_k1()
    } else {
        true
    };

    let mut timer_active = true;
    let mut countdown_secs = 10u8;
    let mut blink_accum_ms = 0u32;
    let mut second_accum_ms = 0u32;
    let mut led_on = true;
    let mut last_key: Option<kernel::event::KeyCode> = None;

    // Initial render FIRST: Draw screen and blit before any UART output!
    render_recovery_frame(canvas, page, Some(countdown_secs), is_k1);
    bsp::set_status_led(bsp::StatusLed::Red);

    // Stream UART dump only after the LCD is already updated
    if let Some(slot) = BOOT_RECORD.get().latest_valid_slot() {
        baremetal_uart_dump(slot);
    }

    // Explicitly re-initialize keypad GPIO matrix pins for guaranteed bare-metal scanning
    drivers::keyboard::KeypadScanner::reset_gpio_pins();

    // Require all keys to be released first before accepting interactive actions
    let mut key_released = false;

    loop {
        hal::wdt::Watchdog::feed();
        hal::delay::delay_ms(10);
        blink_accum_ms += 10;
        second_accum_ms += 10;

        // 1. Strobe / blink LED: fast red strobe (140ms period) while countdown is active, solid red when paused
        if blink_accum_ms >= 140 {
            blink_accum_ms = 0;
            led_on = !led_on;
            if timer_active {
                bsp::set_status_led(if led_on {
                    bsp::StatusLed::Red
                } else {
                    bsp::StatusLed::Off
                });
            } else {
                bsp::set_status_led(bsp::StatusLed::Red);
            }
        }

        // 2. 1-second countdown tick towards automatic recovery reboot
        if timer_active && second_accum_ms >= 1000 {
            second_accum_ms = 0;
            if countdown_secs > 0 {
                countdown_secs -= 1;
            }
            if countdown_secs == 0 {
                bsp::set_status_led(bsp::StatusLed::Off);
                cortex_m::peripheral::SCB::sys_reset();
            }

            render_recovery_frame(canvas, page, Some(countdown_secs), is_k1);
        }

        // 3. Direct hardware keypad matrix polling with release confirmation
        let raw_key = drivers::keyboard::KeypadScanner::poll_matrix();
        if let Some(code) = raw_key {
            if key_released && last_key != Some(code) {
                key_released = false;
                timer_active = false;
                bsp::set_status_led(bsp::StatusLed::Red);

                match code {
                    kernel::event::KeyCode::Menu | kernel::event::KeyCode::Exit => {
                        bsp::set_status_led(bsp::StatusLed::Off);
                        cortex_m::peripheral::SCB::sys_reset();
                    }
                    kernel::event::KeyCode::Side1 => {
                        if let Some(slot) = BOOT_RECORD.get().latest_valid_slot() {
                            baremetal_uart_dump(slot);
                        }
                    }
                    _ => {
                        #[cfg(feature = "panic_gui_extended")]
                        {
                            page = crate::boot::recovery::next_recovery_page(page, code);
                        }
                    }
                }

                render_recovery_frame(canvas, page, None, is_k1);
            }
        } else {
            key_released = true;
        }
        last_key = raw_key;
    }
}

#[cfg(not(feature = "panic_gui"))]
pub fn recovery_loop() -> ! {
    bsp::set_status_led(bsp::StatusLed::Red);
    if let Some(slot) = BOOT_RECORD.get().latest_valid_slot() {
        baremetal_uart_dump(slot);
    }
    for _ in 0..150 {
        hal::wdt::Watchdog::feed();
        hal::delay::delay_ms(20);
    }
    cortex_m::peripheral::SCB::sys_reset();
}
