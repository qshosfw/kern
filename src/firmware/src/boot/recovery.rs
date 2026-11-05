//! System Safe-Mode Recovery Screen & Professional Hardware Crash Forensics.
//!
//! Provides exhaustive register dump, stack trace, fault attribution,
//! and interactive user recovery options (Safe Mode, Calibration Diagnostics, Reboot).

use crate::config::MasterConfig;
use kernel::boot::BootSlot;

#[cfg(all(feature = "panic_disasm", feature = "panic_gui_extended"))]
fn decode_thumb16(inst: u16) -> &'static str {
    const OP_CATS: [&str; 16] = [
        "SHIFT", "ADDSUB", "MOV", "CMP", "ALU", "LDRSTR", "LDRSTR", "BYTES", "HALFS", "SPOPS",
        "ADR", "STACK", "STM", "BRANCH", "B", "BL",
    ];
    OP_CATS[(inst >> 12) as usize]
}

#[inline(never)]
fn draw_item(canvas: &mut gfx::Canvas, col: u8, row: u8, label: &str, val: &str) {
    let font = &gfx::fonts::tomthumb::FONT;
    let x = if col == 0 { 2 } else { 66 };
    let y = 15 + (row as i16) * 8;
    gfx::text::draw_str(canvas, x, y, label, font, gfx::Color::Fill);
    gfx::text::draw_str(canvas, x + 18, y, val, font, gfx::Color::Fill);
}

#[inline(never)]
fn draw_item_hex(
    canvas: &mut gfx::Canvas,
    col: u8,
    row: u8,
    label: &str,
    val: u32,
    hbuf: &mut [u8; 8],
) {
    let s = if val == 0xFFFF_FFFF {
        "????????"
    } else {
        gfx::text::format_hex32(val, hbuf)
    };
    draw_item(canvas, col, row, label, s);
}

#[inline(never)]
fn draw_stack_free(canvas: &mut gfx::Canvas, col: u8, row: u8, sp: u32, nbuf: &mut [u8; 6]) {
    if sp >= 0x2000_0000 && sp <= 0x2000_4000 {
        let free_bytes = sp.saturating_sub(0x2000_0000);
        let s = gfx::text::format_u32(free_bytes, nbuf);
        draw_item(canvas, col, row, "STK:", s);
    } else {
        draw_item(canvas, col, row, "STK:", "OOB!");
    }
}

#[cfg(feature = "panic_gui_extended")]
#[inline(never)]
fn draw_inst_mnem(canvas: &mut gfx::Canvas, row: u8, label: &str, inst: u16) {
    let font = &gfx::fonts::tomthumb::FONT;
    let y = 15 + (row as i16) * 8;
    gfx::text::draw_str(canvas, 2, y, label, font, gfx::Color::Fill);
    if inst > 0 {
        let mut obuf = [0u8; 4];
        let hex_str = gfx::text::format_hex16(inst, &mut obuf);
        gfx::text::draw_str(canvas, 20, y, hex_str, font, gfx::Color::Fill);
        #[cfg(feature = "panic_disasm")]
        {
            let mnem = decode_thumb16(inst);
            gfx::text::draw_str(canvas, 42, y, mnem, font, gfx::Color::Fill);
        }
    } else {
        gfx::text::draw_str(canvas, 20, y, "NONE", font, gfx::Color::Fill);
    }
}

#[inline(never)]
fn draw_page_overview(
    canvas: &mut gfx::Canvas,
    slot: &BootSlot,
    hbuf: &mut [u8; 8],
    nbuf: &mut [u8; 6],
) {
    let font = &gfx::fonts::tomthumb::FONT;
    draw_item_hex(canvas, 0, 0, "PC:", slot.fault_pc, hbuf);
    draw_item_hex(canvas, 1, 0, "LR:", slot.fault_lr, hbuf);

    draw_item_hex(canvas, 0, 1, "SP:", slot.fault_sp, hbuf);
    let is_hard = slot.reason() == Some(kernel::boot::ResetReason::Hard);
    let (right_label, right_val) = if slot.panic_file_hash != 0 {
        (if is_hard { "ADR:" } else { "CRC:" }, slot.panic_file_hash)
    } else {
        ("PSR:", slot.xpsr)
    };
    draw_item_hex(canvas, 1, 1, right_label, right_val, hbuf);

    // Row 2 (y=31): Actor & Cause
    let mut actor_len = 0;
    while actor_len < slot.actor_tag.len() && slot.actor_tag[actor_len] != 0 {
        actor_len += 1;
    }
    let actor_str = if actor_len > 0 {
        gfx::text::from_ascii_bytes(&slot.actor_tag[..actor_len]).unwrap_or("UNKNOWN")
    } else {
        "CORE"
    };
    gfx::text::draw_str_max_x(canvas, 2, 31, actor_str, font, gfx::Color::Fill, 62);

    let cause_str = match slot.reason() {
        Some(kernel::boot::ResetReason::Hard) => "CAUSE: HARD",
        Some(kernel::boot::ResetReason::Appf) => "CAUSE: APPF",
        Some(kernel::boot::ResetReason::Wdog) => "CAUSE: WDOG",
        Some(kernel::boot::ResetReason::Tags) => "CAUSE: TAGS",
        Some(kernel::boot::ResetReason::Brow) => "CAUSE: BROW",
        None => "CAUSE: UNKN",
    };
    gfx::text::draw_str(canvas, 66, 31, cause_str, font, gfx::Color::Fill);

    // Row 3 (y=39): SRAM Stack Headroom & Slot CRC32
    draw_stack_free(canvas, 0, 3, slot.fault_sp, nbuf);
    draw_item_hex(canvas, 1, 3, "CRC:", slot.crc32, hbuf);

    // Row 4 (y=47): Instruction Opcode & Top Stack Word
    if is_hard {
        if slot.panic_line > 0 {
            let mut obuf = [0u8; 4];
            let hex_str = gfx::text::format_hex16(slot.panic_line, &mut obuf);
            draw_item(canvas, 0, 4, "OP:", hex_str);
        } else {
            draw_item(canvas, 0, 4, "OP:", "NONE");
        }
        draw_item_hex(canvas, 1, 4, "S0:", slot.stack_dump[8], hbuf);
    } else {
        draw_item_hex(canvas, 0, 4, "S0:", slot.stack_dump[0], hbuf);
        draw_item_hex(canvas, 1, 4, "S1:", slot.stack_dump[1], hbuf);
    }
}

#[cfg(feature = "panic_gui_extended")]
static REG_LABELS: [&str; 10] = [
    "R0:", "R1:", "R2:", "R3:", "R12:", "RET:", "PSR:", "SIG:", "S0:", "S1:",
];
#[cfg(feature = "panic_gui_extended")]
static BT_LABELS: [&str; 6] = ["PC:", "LR0:", "LR1:", "LR2:", "LR3:", "LR4:"];
#[cfg(feature = "panic_gui_extended")]
static STK_LABELS: [&str; 8] = [
    "+00:", "+04:", "+08:", "+0C:", "+10:", "+14:", "+18:", "+1C:",
];

#[cfg(feature = "panic_gui_extended")]
#[inline(never)]
fn draw_page_registers(canvas: &mut gfx::Canvas, slot: &BootSlot, hbuf: &mut [u8; 8]) {
    let vals = [
        slot.stack_dump[0],
        slot.stack_dump[1],
        slot.stack_dump[2],
        slot.stack_dump[3],
        slot.stack_dump[4],
        slot.fault_lr,
        slot.xpsr,
        slot.forensics.crash_signature,
        slot.stack_dump[8],
        slot.stack_dump[9],
    ];
    for (i, (&label, &val)) in REG_LABELS.iter().zip(vals.iter()).enumerate() {
        draw_item_hex(canvas, (i & 1) as u8, (i >> 1) as u8, label, val, hbuf);
    }
}

#[cfg(feature = "panic_gui_extended")]
#[inline(never)]
fn draw_page_backtrace(
    canvas: &mut gfx::Canvas,
    slot: &BootSlot,
    hbuf: &mut [u8; 8],
    nbuf: &mut [u8; 6],
) {
    let vals = [
        slot.fault_pc,
        slot.fault_lr,
        slot.stack_dump[8],
        slot.stack_dump[9],
        slot.stack_dump[10],
        slot.stack_dump[11],
    ];
    for (i, (&label, &val)) in BT_LABELS.iter().zip(vals.iter()).enumerate() {
        draw_item_hex(canvas, (i & 1) as u8, (i >> 1) as u8, label, val, hbuf);
    }

    draw_item(
        canvas,
        0,
        3,
        "UNWIND:",
        gfx::text::format_u8(slot.forensics.unwind_count, nbuf),
    );

    let vec_str = match slot.forensics.ipsr_num {
        0 => "THREAD",
        2 => "NMI",
        3 => "HARD",
        11 => "SVC",
        14 => "PENDSV",
        15 => "SYSTICK",
        v if v >= 16 => "IRQ",
        _ => "EXC",
    };
    draw_item(canvas, 1, 3, "VEC:", vec_str);

    draw_item_hex(canvas, 0, 4, "SIG:", slot.forensics.crash_signature, hbuf);
}

#[cfg(feature = "panic_gui_extended")]
#[inline(never)]
fn draw_page_stack(
    canvas: &mut gfx::Canvas,
    slot: &BootSlot,
    hbuf: &mut [u8; 8],
    nbuf: &mut [u8; 6],
) {
    let dump = slot.stack_dump;
    for (i, (&label, &val)) in STK_LABELS.iter().zip(dump[8..16].iter()).enumerate() {
        draw_item_hex(canvas, (i & 1) as u8, (i >> 1) as u8, label, val, hbuf);
    }

    draw_item_hex(canvas, 0, 4, "SP:", slot.fault_sp, hbuf);
    draw_stack_free(canvas, 1, 4, slot.fault_sp, nbuf);
}

#[cfg(feature = "panic_gui_extended")]
#[inline(never)]
fn draw_page_forensics(
    canvas: &mut gfx::Canvas,
    slot: &BootSlot,
    hbuf: &mut [u8; 8],
    nbuf: &mut [u8; 6],
) {
    draw_inst_mnem(canvas, 0, "PREV:", slot.forensics.inst_pre);

    let curr = if slot.reason() == Some(kernel::boot::ResetReason::Hard) {
        slot.panic_line
    } else {
        0
    };
    draw_inst_mnem(canvas, 1, "FLT: ", curr);

    let reg = slot.forensics.fault_reg;
    if reg <= 15 {
        let mut rbuf = [b'R', 0, 0];
        let num = gfx::text::format_u8(reg, nbuf);
        let nlen = num.len().min(2);
        rbuf[1..1 + nlen].copy_from_slice(&num.as_bytes()[..nlen]);
        let r_str = unsafe { core::str::from_utf8_unchecked(&rbuf[..1 + nlen]) };
        draw_item(canvas, 0, 2, "REG:", r_str);
    } else {
        draw_item(canvas, 0, 2, "REG:", "NONE");
    }

    let is_write = (slot.forensics.access_flags & 1) != 0;
    draw_item(
        canvas,
        1,
        2,
        "ACC:",
        if is_write { "WRITE" } else { "READ" },
    );

    let mode_str = if (slot.forensics.exec_mode & 1) != 0 {
        "PSP"
    } else {
        "MSP"
    };
    draw_item(canvas, 0, 3, "MODE:", mode_str);

    draw_item(
        canvas,
        1,
        3,
        "IPSR:",
        gfx::text::format_u8(slot.forensics.ipsr_num, nbuf),
    );

    draw_item_hex(canvas, 0, 4, "SIG:", slot.forensics.crash_signature, hbuf);
}

/// Render exhaustive forensic crash report on ST7565 128x64 monochrome LCD.
pub fn draw_recovery_screen(
    canvas: &mut gfx::Canvas,
    slot: &BootSlot,
    page: u8,
    countdown: Option<u8>,
    is_k1: bool,
) {
    let font = &gfx::fonts::tomthumb::FONT;
    let mut hbuf = [0u8; 8];
    let mut nbuf = [0u8; 6];

    // 1. Inverted Header Bar (y=0..8)
    canvas.fill_rect(0, 0, 128, 9, gfx::Color::Fill);
    gfx::text::draw_str(canvas, 3, 6, "PANIC!", font, gfx::Color::Clear);

    if let Some(sec) = countdown {
        let mut r_buf = [b'R', b'E', b'S', b'E', b'T', b' ', b'0', b'0', b's'];
        let sec_str = gfx::text::format_u8(sec, &mut nbuf);
        let slen = sec_str.len();
        r_buf[6..6 + slen].copy_from_slice(sec_str.as_bytes());
        r_buf[6 + slen] = b's';
        let s = unsafe { core::str::from_utf8_unchecked(&r_buf[..7 + slen]) };
        gfx::text::draw_str_align(
            canvas,
            64,
            6,
            s,
            font,
            gfx::Color::Clear,
            gfx::text::Align::Center,
        );
    }

    let cstr = gfx::text::format_u8(slot.crash_count, &mut nbuf);
    let clen = cstr.len().min(3);

    #[cfg(feature = "panic_gui_extended")]
    let p = page % 5;
    #[cfg(not(feature = "panic_gui_extended"))]
    let _ = page;

    #[cfg(feature = "panic_gui_extended")]
    let mut pbuf = [b'P', b'1' + p, b'/', b'5', b' ', b'#', 0, 0, 0];
    #[cfg(feature = "panic_gui_extended")]
    pbuf[6..6 + clen].copy_from_slice(&cstr.as_bytes()[..clen]);

    #[cfg(not(feature = "panic_gui_extended"))]
    let mut pbuf = [b'C', b'R', b'A', b'S', b'H', b' ', b'#', 0, 0, 0];
    #[cfg(not(feature = "panic_gui_extended"))]
    pbuf[7..7 + clen].copy_from_slice(&cstr.as_bytes()[..clen]);

    #[cfg(feature = "panic_gui_extended")]
    let pstr = unsafe { core::str::from_utf8_unchecked(&pbuf[..6 + clen]) };
    #[cfg(not(feature = "panic_gui_extended"))]
    let pstr = unsafe { core::str::from_utf8_unchecked(&pbuf[..7 + clen]) };
    gfx::text::draw_str(canvas, 86, 6, pstr, font, gfx::Color::Clear);

    #[cfg(feature = "panic_gui_extended")]
    match p {
        0 => draw_page_overview(canvas, slot, &mut hbuf, &mut nbuf),
        1 => draw_page_registers(canvas, slot, &mut hbuf),
        2 => draw_page_backtrace(canvas, slot, &mut hbuf, &mut nbuf),
        3 => draw_page_stack(canvas, slot, &mut hbuf, &mut nbuf),
        _ => draw_page_forensics(canvas, slot, &mut hbuf, &mut nbuf),
    }

    #[cfg(not(feature = "panic_gui_extended"))]
    draw_page_overview(canvas, slot, &mut hbuf, &mut nbuf);

    // 3. Inverted Footer / Actions (y=52..63)
    canvas.draw_horizontal_line(0, 52, 128, gfx::Color::Fill);
    gfx::widgets::draw_key_badge(canvas, 1, 54, "M", "Reset");

    #[cfg(feature = "panic_gui_extended")]
    if is_k1 {
        gfx::widgets::draw_key_badge(canvas, 38, 54, "<", "");
        gfx::widgets::draw_key_badge(canvas, 48, 54, ">", "Page");
    } else {
        gfx::widgets::draw_key_badge(canvas, 36, 54, "^", "");
        gfx::widgets::draw_key_badge(canvas, 47, 54, "v", "Page");
    }
    #[cfg(not(feature = "panic_gui_extended"))]
    let _ = is_k1;

    gfx::widgets::draw_key_badge_right(canvas, 126, 54, "EXIT", "Reboot");
}

#[cfg(feature = "panic_gui_extended")]
pub fn next_recovery_page(page: u8, code: kernel::event::KeyCode) -> u8 {
    const MAX_PAGES: u8 = 5;
    match code {
        kernel::event::KeyCode::Up | kernel::event::KeyCode::Key1 => {
            if page == 0 {
                MAX_PAGES - 1
            } else {
                page - 1
            }
        }
        kernel::event::KeyCode::Key0 => 0,
        kernel::event::KeyCode::Key2 => 1,
        kernel::event::KeyCode::Key3 => 2,
        kernel::event::KeyCode::Key4 => 3,
        kernel::event::KeyCode::Key5 => 4,
        _ => (page + 1) % MAX_PAGES,
    }
}

#[inline(never)]
pub fn run_safe_mode_recovery(
    hw: &mut crate::boot::BootHardware,
    slot: &BootSlot,
    master_cfg: &mut MasterConfig,
) {
    hw.lcd.turn_on();
    hw.lcd.set_contrast(50);
    hw.backlight.wake();
    bsp::set_status_led(bsp::StatusLed::Orange);

    #[allow(unused_mut)]
    let mut current_page = 0u8;
    let is_k1 = master_cfg.is_k1();

    loop {
        let canvas = crate::system_canvas();
        canvas.clear();
        draw_recovery_screen(canvas, slot, current_page, None, is_k1);
        hw.lcd.blit(canvas);

        match crate::boot::boot_wait_key(hw.key_scanner, 0) {
            Some(kernel::event::KeyCode::Menu) => {
                crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
                *master_cfg = MasterConfig::new();
                master_cfg.audio.boot_jingle = 0; // Disable jingle in safe mode recovery
                let _ = master_cfg.save_all(hw.filesystem);
                bsp::set_status_led(bsp::StatusLed::Off);
                break;
            }
            Some(kernel::event::KeyCode::Exit) => {
                crate::panic::BOOT_RECORD.get_mut().clear_crash_loop();
                bsp::set_status_led(bsp::StatusLed::Off);
                cortex_m::peripheral::SCB::sys_reset();
            }
            #[cfg(feature = "panic_gui_extended")]
            Some(code) => {
                current_page = next_recovery_page(current_page, code);
            }
            #[cfg(not(feature = "panic_gui_extended"))]
            Some(_) => {}
            None => {}
        }
    }
}
