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

const OVERVIEW_LABELS: &[u8] =
    b"PC:\0LR:\0SP:\0ADR:\0CRC:\0PSR:\0ACT:\0RSN:\0STK:\0OP:\0S0:\0S1:\0";
const LBL_PC: u8 = 0;
const LBL_LR: u8 = 1;
const LBL_SP: u8 = 2;
const LBL_ADR: u8 = 3;
const LBL_CRC: u8 = 4;
const LBL_PSR: u8 = 5;
const LBL_ACT: u8 = 6;
const LBL_RSN: u8 = 7;
const LBL_STK: u8 = 8;
const LBL_OP: u8 = 9;
const LBL_S0: u8 = 10;
const LBL_S1: u8 = 11;

#[inline(always)]
const fn item_spec(pos: u8, lbl: u8) -> u8 {
    (pos << 4) | (lbl & 0x0F)
}

fn draw_item_raw(canvas: &mut gfx::Canvas, pos: u8, label: &str, val: &str) {
    let font = &gfx::fonts::tomthumb::FONT;
    let x = if (pos & 1) == 0 { 2 } else { 66 };
    let y = 15 + ((pos >> 1) as i16) * 8;
    gfx::text::draw_str(canvas, x, y, label, font, gfx::Color::Fill);
    gfx::text::draw_str(canvas, x + 18, y, val, font, gfx::Color::Fill);
}

#[cfg(feature = "panic_gui_extended")]
#[inline(never)]
fn draw_item(canvas: &mut gfx::Canvas, pos: u8, label: &str, val: &str) {
    draw_item_raw(canvas, pos, label, val);
}

#[cfg(feature = "panic_gui_extended")]
#[inline(never)]
fn draw_item_hex(canvas: &mut gfx::Canvas, pos: u8, label: &str, val: u32) {
    let mut hbuf = [0u8; 8];
    let s = if val == 0xFFFF_FFFF {
        "????????"
    } else {
        gfx::text::format_hex32(val, &mut hbuf)
    };
    draw_item_raw(canvas, pos, label, s);
}

#[inline(never)]
fn draw_overview_item(canvas: &mut gfx::Canvas, spec: u8, val: &str) {
    draw_item_raw(
        canvas,
        spec >> 4,
        utils::format::nth_cstr(OVERVIEW_LABELS, spec & 0x0F),
        val,
    );
}

#[inline(never)]
fn draw_overview_hex(canvas: &mut gfx::Canvas, spec: u8, val: u32) {
    let mut hbuf = [0u8; 8];
    let s = if val == 0xFFFF_FFFF {
        "????????"
    } else {
        gfx::text::format_hex32(val, &mut hbuf)
    };
    draw_overview_item(canvas, spec, s);
}

#[inline(never)]
fn draw_stack_free(canvas: &mut gfx::Canvas, pos: u8, sp: u32) {
    let mut nbuf = [0u8; 6];
    let s = if sp >= 0x2000_0000 && sp <= 0x2000_4000 {
        gfx::text::format_u32(sp.saturating_sub(0x2000_0000), &mut nbuf)
    } else {
        "OOB!"
    };
    draw_overview_item(canvas, item_spec(pos, LBL_STK), s);
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

fn draw_page_overview(canvas: &mut gfx::Canvas, slot: &BootSlot) {
    let is_hard = slot.reason() == Some(kernel::boot::ResetReason::Hard);
    let (r1_lbl, r1_val) = if slot.panic_file_hash != 0 {
        (
            if is_hard { LBL_ADR } else { LBL_CRC },
            slot.panic_file_hash,
        )
    } else {
        (LBL_PSR, slot.xpsr)
    };
    draw_overview_hex(canvas, item_spec(0, LBL_PC), slot.fault_pc);
    draw_overview_hex(canvas, item_spec(1, LBL_LR), slot.fault_lr);
    draw_overview_hex(canvas, item_spec(2, LBL_SP), slot.fault_sp);
    draw_overview_hex(canvas, item_spec(3, r1_lbl), r1_val);
    draw_overview_hex(canvas, item_spec(7, LBL_CRC), slot.crc32);

    // Row 2 (y=31): Actor & Reason
    draw_overview_item(canvas, item_spec(4, LBL_ACT), slot.actor_str());
    draw_overview_item(
        canvas,
        item_spec(5, LBL_RSN),
        slot.reason().map_or("UNKN", |r| r.as_str()),
    );

    // Row 3 (y=39): SRAM Stack Headroom
    draw_stack_free(canvas, 6, slot.fault_sp);

    // Row 4 (y=47): Instruction Opcode & Top Stack Word
    if is_hard {
        if slot.panic_line > 0 {
            let mut obuf = [0u8; 4];
            let hex_str = gfx::text::format_hex16(slot.panic_line, &mut obuf);
            draw_overview_item(canvas, item_spec(8, LBL_OP), hex_str);
        } else {
            draw_overview_item(canvas, item_spec(8, LBL_OP), "NONE");
        }
        draw_overview_hex(canvas, item_spec(9, LBL_S0), slot.stack_dump[8]);
    } else {
        draw_overview_hex(canvas, item_spec(8, LBL_S0), slot.stack_dump[0]);
        draw_overview_hex(canvas, item_spec(9, LBL_S1), slot.stack_dump[1]);
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
fn draw_page_registers(canvas: &mut gfx::Canvas, slot: &BootSlot) {
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
        draw_item_hex(canvas, i as u8, label, val);
    }
}

#[cfg(feature = "panic_gui_extended")]
fn draw_page_backtrace(canvas: &mut gfx::Canvas, slot: &BootSlot) {
    let mut nbuf = [0u8; 6];
    let vals = [
        slot.fault_pc,
        slot.fault_lr,
        slot.stack_dump[8],
        slot.stack_dump[9],
        slot.stack_dump[10],
        slot.stack_dump[11],
    ];
    for (i, (&label, &val)) in BT_LABELS.iter().zip(vals.iter()).enumerate() {
        draw_item_hex(canvas, i as u8, label, val);
    }

    draw_item(
        canvas,
        6,
        "UNWIND:",
        gfx::text::format_u8(slot.forensics.unwind_count, &mut nbuf),
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
    draw_item(canvas, 7, "VEC:", vec_str);

    draw_item_hex(canvas, 8, "SIG:", slot.forensics.crash_signature);
}

#[cfg(feature = "panic_gui_extended")]
fn draw_page_stack(canvas: &mut gfx::Canvas, slot: &BootSlot) {
    let dump = slot.stack_dump;
    for (i, (&label, &val)) in STK_LABELS.iter().zip(dump[8..16].iter()).enumerate() {
        draw_item_hex(canvas, i as u8, label, val);
    }

    draw_item_hex(canvas, 8, "SP:", slot.fault_sp);
    draw_stack_free(canvas, 9, slot.fault_sp);
}

#[cfg(feature = "panic_gui_extended")]
fn draw_page_forensics(canvas: &mut gfx::Canvas, slot: &BootSlot) {
    let mut nbuf = [0u8; 6];
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
        let num = gfx::text::format_u8(reg, &mut nbuf);
        let nlen = num.len().min(2);
        rbuf[1..1 + nlen].copy_from_slice(&num.as_bytes()[..nlen]);
        let r_str = unsafe { core::str::from_utf8_unchecked(&rbuf[..1 + nlen]) };
        draw_item(canvas, 4, "REG:", r_str);
    } else {
        draw_item(canvas, 4, "REG:", "NONE");
    }

    let is_write = (slot.forensics.access_flags & 1) != 0;
    draw_item(canvas, 5, "ACC:", if is_write { "WRITE" } else { "READ" });

    let mode_str = if (slot.forensics.exec_mode & 1) != 0 {
        "PSP"
    } else {
        "MSP"
    };
    draw_item(canvas, 6, "MODE:", mode_str);

    draw_item(
        canvas,
        7,
        "IPSR:",
        gfx::text::format_u8(slot.forensics.ipsr_num, &mut nbuf),
    );

    draw_item_hex(canvas, 8, "SIG:", slot.forensics.crash_signature);
}

static RECOVERY_CHROME: [gfx::widgets::TextLine; 2] = [
    gfx::widgets::TextLine::new(58, 0, 0, "[M] Reset"),
    gfx::widgets::TextLine::new(58, 0, 2, "[EXIT] Reboot"),
];

/// Render exhaustive forensic crash report on ST7565 128x64 monochrome LCD.
pub fn draw_recovery_screen(
    canvas: &mut gfx::Canvas,
    slot: &BootSlot,
    page: u8,
    countdown: Option<u8>,
    is_k1: bool,
) {
    let font = &gfx::fonts::tomthumb::FONT;

    // 1. Inverted Header Bar (y=0..8) + Footer Chrome via shared TextScreen
    gfx::widgets::draw_text_screen(canvas, "", &RECOVERY_CHROME, None);
    gfx::widgets::draw_negated_header(canvas, 0, "PANIC!");

    if let Some(sec) = countdown {
        let (sd, sr) = utils::math::div10_u16(sec as u16);
        let mut r_buf = *b"RESET 00s";
        r_buf[6] += sd as u8;
        r_buf[7] += sr;
        let s = unsafe { core::str::from_utf8_unchecked(&r_buf) };
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

    let (cd, cr) = utils::math::div10_u16(slot.crash_count.min(99) as u16);

    #[cfg(feature = "panic_gui_extended")]
    let p = page % 5;
    #[cfg(not(feature = "panic_gui_extended"))]
    let _ = page;

    #[cfg(feature = "panic_gui_extended")]
    let pbuf = {
        let mut b = *b"P1/5 #00";
        b[1] += p;
        b[6] += cd as u8;
        b[7] += cr;
        b
    };
    #[cfg(not(feature = "panic_gui_extended"))]
    let pbuf = {
        let mut b = *b"#00";
        b[1] += cd as u8;
        b[2] += cr;
        b
    };
    let pstr = unsafe { core::str::from_utf8_unchecked(&pbuf) };
    gfx::text::draw_str_align(
        canvas,
        126,
        6,
        pstr,
        font,
        gfx::Color::Clear,
        gfx::text::Align::Right,
    );

    #[cfg(feature = "panic_gui_extended")]
    match p {
        0 => draw_page_overview(canvas, slot),
        1 => draw_page_registers(canvas, slot),
        2 => draw_page_backtrace(canvas, slot),
        3 => draw_page_stack(canvas, slot),
        _ => draw_page_forensics(canvas, slot),
    }

    #[cfg(not(feature = "panic_gui_extended"))]
    draw_page_overview(canvas, slot);

    // 3. Inverted Footer / Actions (y=52..63)
    canvas.draw_horizontal_line(0, 52, 128, gfx::Color::Fill);

    #[cfg(feature = "panic_gui_extended")]
    if is_k1 {
        gfx::text::draw_str(canvas, 48, 58, "<> Page", font, gfx::Color::Fill);
    } else {
        gfx::text::draw_str(canvas, 48, 58, "^v Page", font, gfx::Color::Fill);
    }
    #[cfg(not(feature = "panic_gui_extended"))]
    let _ = is_k1;
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
        draw_recovery_screen(canvas, slot, current_page, None, is_k1);
        hw.lcd.blit(canvas);

        match crate::boot::boot_wait_key(hw.key_scanner, 0) {
            Some(kernel::event::KeyCode::Menu) => {
                crate::boot::clear_crash_strikes();
                unsafe {
                    MasterConfig::init_in_place(master_cfg as *mut MasterConfig);
                }
                master_cfg.audio.boot_jingle = 0; // Disable jingle in safe mode recovery
                let _ = master_cfg.save_all(hw.filesystem);
                bsp::set_status_led(bsp::StatusLed::Off);
                break;
            }
            Some(kernel::event::KeyCode::Exit) => {
                crate::boot::prepare_clean_reboot();
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
