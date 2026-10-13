//! System Panic & HardFault Handler.
//! Supports optional `panic_gui` feature for high-contrast on-screen LCD panic dumps during development,
//! and ultra-minimal zero-overhead hardware halt/reset for production release builds.

use py32_hal as hal;

#[repr(C)]
pub struct FlightRecorderCell(core::cell::UnsafeCell<kernel::flight::FlightRecord>);
unsafe impl Sync for FlightRecorderCell {}

impl FlightRecorderCell {
    pub const fn new() -> Self {
        Self(core::cell::UnsafeCell::new(
            kernel::flight::FlightRecord::new(),
        ))
    }

    #[allow(clippy::mut_from_ref)]
    #[inline(always)]
    pub fn get_mut(&self) -> &mut kernel::flight::FlightRecord {
        unsafe { &mut *self.0.get() }
    }

    #[inline(always)]
    pub fn get(&self) -> &kernel::flight::FlightRecord {
        unsafe { &*self.0.get() }
    }
}

#[used]
#[unsafe(link_section = ".uninit")]
pub static FLIGHT_RECORDER: FlightRecorderCell = FlightRecorderCell::new();

/// HardFault exception handler
#[cortex_m_rt::exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    cortex_m::interrupt::disable();

    bsp::pins::BACKLIGHT.set_high();
    bsp::pins::FLASHLIGHT.set_high();

    // Record crash telemetry in uninitialized SRAM surviving warm boot
    FLIGHT_RECORDER
        .get_mut()
        .record_fault(ef.pc(), ef.lr(), 0, 0);

    // Feed watchdog briefly before system reset using interrupt-free polled delays
    for _ in 0..50 {
        hal::wdt::Watchdog::feed();
        hal::delay::delay_us(10_000);
    }

    cortex_m::peripheral::SCB::sys_reset();
}

// ── Ultra-Minimal Production Panic Handler (Zero Flash Overhead) ────────────

#[cfg(not(feature = "panic_gui"))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    cortex_m::interrupt::disable();

    bsp::pins::BACKLIGHT.set_high();
    bsp::pins::FLASHLIGHT.set_high();

    let rec = FLIGHT_RECORDER.get_mut();
    rec.magic = kernel::flight::FLIGHT_MAGIC;
    rec.crash_counter = rec.crash_counter.saturating_add(1);

    // Feed watchdog briefly before system reset using interrupt-free polled delays
    for _ in 0..50 {
        hal::wdt::Watchdog::feed();
        hal::delay::delay_us(10_000);
    }

    cortex_m::peripheral::SCB::sys_reset();
}

// ── Rich Graphical Panic Dump Handler (Debug / Diagnostics Only) ────────────

#[cfg(feature = "panic_gui")]
use gfx::Canvas;

#[cfg(feature = "panic_gui")]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    cortex_m::interrupt::disable();

    // 1. Hardware Alert: Turn ON Backlight and Flashlight
    bsp::pins::BACKLIGHT.set_high();
    bsp::pins::FLASHLIGHT.set_high();

    // 2. Format Panic Location & Details
    let mut line_buf = [0u8; 10];

    let file_str = if let Some(loc) = info.location() {
        let f = loc.file();
        let name_start = gfx::text::rfind_byte(f, b'/').map_or(0, |i| i + 1);
        &f[name_start..]
    } else {
        "unknown"
    };

    let line_num = info.location().map_or(0, |loc| loc.line());
    let line_str = gfx::text::format_u32(line_num, &mut line_buf);

    // 3. Render High-Contrast Inverted Panic Screen Framebuffer
    let mut canvas = Canvas::new();
    canvas.clear();

    // Header (Inverted Bar)
    canvas.fill_rect(0, 0, 128, 9, gfx::Color::Fill);
    gfx::text::draw_str(
        &mut canvas,
        24,
        1,
        "KERNEL PANIC",
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Clear,
    );

    // Details
    gfx::text::draw_str(
        &mut canvas,
        2,
        14,
        "FILE:",
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Fill,
    );
    gfx::text::draw_str(
        &mut canvas,
        32,
        14,
        file_str,
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Fill,
    );

    // Line
    gfx::text::draw_str(
        &mut canvas,
        2,
        24,
        "LINE:",
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Fill,
    );
    gfx::text::draw_str(
        &mut canvas,
        32,
        24,
        line_str,
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Fill,
    );

    // Error box
    canvas.draw_rect(0, 34, 128, 20, gfx::Color::Fill);
    gfx::text::draw_str(
        &mut canvas,
        4,
        38,
        "CRITICAL HARD FAULT",
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Fill,
    );
    gfx::text::draw_str(
        &mut canvas,
        4,
        46,
        "System Halted",
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Fill,
    );

    // Footer
    gfx::text::draw_str(
        &mut canvas,
        14,
        57,
        "Rebooting in 5s...",
        &gfx::fonts::tomthumb::FONT,
        gfx::Color::Fill,
    );

    // 4. Low-level direct SPI blit to ST7565 LCD VRAM
    bsp::pins::LCD_CS.set_low();
    bsp::pins::LCD_A0.set_low();
    let _ = hal::spi::write_byte(0x40);

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
    bsp::pins::LCD_CS.set_high();

    // 5. Watchdog loop delay before MCU reboot
    for _ in 0..500 {
        hal::wdt::Watchdog::feed();
        hal::delay::delay_ms(10);
    }

    cortex_m::peripheral::SCB::sys_reset();
}
