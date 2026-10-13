//! System Safe-Mode Recovery Screen and Crash Handling.

use crate::config::MasterConfig;

pub fn draw_recovery_screen(canvas: &mut gfx::Canvas, crash_count: u8, pc: u32, lr: u32) {
    let font = &gfx::fonts::tomthumb::FONT;
    gfx::widgets::draw_negated_header(canvas, 0, "SYSTEM RECOVERY");

    let mut buf = [0u8; 8];
    gfx::text::draw_str(canvas, 2, 14, "Crashes:", font, gfx::Color::Fill);
    gfx::text::draw_str(
        canvas,
        54,
        14,
        gfx::text::format_u8(crash_count, &mut buf),
        font,
        gfx::Color::Fill,
    );

    gfx::text::draw_str(canvas, 2, 23, "Fault PC: 0x", font, gfx::Color::Fill);
    gfx::text::draw_str(
        canvas,
        64,
        23,
        gfx::text::format_hex32(pc, &mut buf),
        font,
        gfx::Color::Fill,
    );
    gfx::text::draw_str(canvas, 2, 32, "Fault LR: 0x", font, gfx::Color::Fill);
    gfx::text::draw_str(
        canvas,
        64,
        32,
        gfx::text::format_hex32(lr, &mut buf),
        font,
        gfx::Color::Fill,
    );

    gfx::widgets::draw_key_badge(canvas, 2, 56, "M", "Reset");
    gfx::widgets::draw_key_badge_right(canvas, 126, 56, "EXIT", "Reboot");
}

#[inline(never)]
pub fn run_safe_mode_recovery(
    hw: &mut crate::boot::BootHardware,
    crash_count: u8,
    pc: u32,
    lr: u32,
    master_cfg: &mut MasterConfig,
) {
    hw.lcd.turn_on();
    hw.backlight.wake();
    let canvas = crate::system_canvas();
    canvas.clear();
    draw_recovery_screen(canvas, crash_count, pc, lr);
    hw.lcd.blit(canvas);

    loop {
        match crate::boot::boot_wait_key(hw.key_scanner, 0) {
            Some(kernel::event::KeyCode::Menu) => {
                crate::panic::FLIGHT_RECORDER.get_mut().clear();
                *master_cfg = MasterConfig::new();
                master_cfg.audio.boot_jingle = 0; // Disable jingle in safe mode recovery
                let _ = master_cfg.save_all(hw.vfs);
                break;
            }
            Some(kernel::event::KeyCode::Exit) => {
                crate::panic::FLIGHT_RECORDER.get_mut().clear();
                cortex_m::peripheral::SCB::sys_reset();
            }
            _ => {}
        }
    }
}
