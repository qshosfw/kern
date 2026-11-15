//! Factory bootloader DFU handoff, live DFU UI screen, and runtime SRAM hook.
//!
//! Keeps factory Flash (`0x0800_0000..0x0800_26FF`) unmodified while augmenting DFU sessions
//! with a relocated SRAM vector table (`SCB->VTOR = 0x2000_3A00`) and PIC Thumb-1 SysTick handler
//! in upper SRAM (`0x2000_3B00..`) that provides:
//! - Battery-saving `PC13` flashlight lock (`GPIOC->LCKR` Input Pull-Down)
//! - Clean warm-exit (`Side Key 1` or `EXIT` hold -> `SYSRESETREQ`) guarded against active flashing or invalid firmware
//! - Blinking splash arrow when USB is disconnected, multi-state `Page 4` capsule animation, `Page 5` live page progress, and `Page 1` cutout `%`
//! - Optional live debug UART telemetry (`feature = "settings_debug"`)

#[cfg(feature = "dfu_hooks")]
use super::bootloader_version_str;
use super::{read_backup_tag, write_backup_tag};
use drivers::st7565::St7565;
#[cfg(feature = "dfu_hooks")]
use gfx::{Canvas, Color, fonts, text, widgets};

#[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
const DFU_SRAM_BANNERS: u32 = 0x2000_3000; // 8 x 168 B [P5(84 B) + P6(84 B)] strips (`cols 22..105`, `0x2000_3000..0x2000_353F`)
#[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
const DFU_SRAM_ARROW: u32 = 0x2000_3540; // 38 B arrow snapshot: P2[45..64] + P3[45..64] (`0x3540..0x3565`)
#[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
const DFU_SRAM_VIDPID: u32 = 0x2000_3580; // 40 B Page 1 [73..113] initial VID:PID strip (`0x3580..0x35A7`)
#[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
const DFU_SRAM_VTOR: u32 = 0x2000_3600; // 48-entry relocated bootloader vector table (`0x100`-aligned, `0x3600..0x36BF`)
#[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
const DFU_SRAM_STATE: u32 = 0x2000_36C0; // Hook state scratch block (48 bytes, `0x36C0..0x36EF`)
#[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
const DFU_SRAM_CODE: u32 = 0x2000_3700; // Copied PIC Thumb-1 SysTick ISR + 3x5 glyph table (`0x3700..0x3FFF`, 2,304 B)

/// Starting X column of `"Flashing page "` on `Page 5`; digits start immediately at `+54 px` (`X=76`).
#[cfg(feature = "dfu_hooks")]
pub const DFU_FLASH_PREFIX_X: u8 = 22;
#[cfg(feature = "dfu_hooks")]
pub const DFU_FLASH_DIGITS_COL: usize = 76;

/// Compressed 128x64 DFU bitmap (PackBits RLE, 1bpp ST7565 page order).
/// Renders the raised handheld radio (`Y=4..31`, `"DFU"` LCD cutout `Y=13..17`), arrow (`X=45..=63`,
/// `Y=18..30`), USB-C plug/cable (`Y=17..31`, bigger rect `X=81..=104`), and 6 px Page 4+5 capsule
/// (`cols 12..=115`, `Y=36..41`), leaving `X>=45, Y=0..16` for right labels and `Y=44..63` open.
#[cfg(feature = "dfu_hooks")]
pub static DFU_SPLASH_RLE: [u8; 168] = [
    0xF1, 0x00, 0x03, 0xE0, 0xF0, 0xF0, 0xE0, 0x8B, 0x00, 0x02, 0xF8, 0xF8, 0xFC, 0xFE, 0xFE, 0xFE,
    0x07, 0x0A, 0xE7, 0x26, 0xC6, 0x06, 0xE6, 0xA6, 0xA6, 0x06, 0xE6, 0x06, 0xE6, 0xFD, 0x06, 0xFE,
    0xFE, 0x00, 0xFC, 0x9D, 0x00, 0x01, 0x07, 0x07, 0xFD, 0xFF, 0x07, 0xF0, 0xF0, 0x70, 0x73, 0x72,
    0x71, 0x70, 0x73, 0xFE, 0x70, 0x06, 0x73, 0x72, 0x73, 0x70, 0x70, 0xF0, 0xF0, 0xFD, 0xFF, 0xF9,
    0x00, 0x07, 0x80, 0xC0, 0xE0, 0xF0, 0xF8, 0xFC, 0xFC, 0xF8, 0xF8, 0xE0, 0x00, 0xC0, 0xFA, 0x00,
    0x00, 0xE0, 0xF9, 0xF0, 0x02, 0x00, 0xF8, 0xFC, 0xED, 0xFE, 0x02, 0xFC, 0xF8, 0x00, 0xFD, 0xF0,
    0xEF, 0xC0, 0xF5, 0x00, 0x00, 0x7F, 0xFC, 0xFF, 0xF3, 0xFB, 0xFC, 0xFF, 0x00, 0x7F, 0xFA, 0x00,
    0x08, 0x01, 0x03, 0x07, 0x0F, 0x1F, 0x3F, 0x7F, 0x7F, 0x3F, 0xF8, 0x0F, 0x00, 0x07, 0xFA, 0x00,
    0x00, 0x0F, 0xF9, 0x1F, 0x02, 0x00, 0x3F, 0x7F, 0xED, 0xFF, 0x02, 0x7F, 0x3F, 0x00, 0xFD, 0x1F,
    0xEF, 0x07, 0xF5, 0x00, 0x00, 0xE0, 0x9B, 0x10, 0x00, 0xE0, 0xE9, 0x00, 0x00, 0x01, 0x9B, 0x02,
    0x00, 0x01, 0x81, 0x00, 0x81, 0x00, 0xF5, 0x00,
];

/// Bitmask in `KeypadScanner::poll_raw_mask()` for `PTT` (bit 18) + `Side Key 2` (bit 1).
/// Booting with this chord skips stock raw-PTT interception (`PB14` LOW bypasses `0x0800_1384`)
/// so Kern launches its hooked DFU UI and SRAM runtime instead of a blank bootloader screen.
pub const PTT_SIDE2_DFU_MASK: u32 = (1 << 18) | (1 << 1);

/// Returns `true` when both `PTT` and `Side Key 2` are held simultaneously.
#[inline(always)]
pub const fn is_ptt_side2_chord(raw_mask: u32) -> bool {
    (raw_mask & PTT_SIDE2_DFU_MASK) == PTT_SIDE2_DFU_MASK
}

/// Packed length-prefixed `Page 5`+`Page 6` (`y=49`) state banners pre-rendered at handoff.
/// Indices: 0=`"Plug in cable"`, 1=`"USB connected"`, 2=`"Ready to flash"`,
/// 3=`"Link active"`, 4=`"Flashing page "`, 5=`"Flash complete"`,
/// 6=`"UART connected"`, 7=`"Aborted - reflash"`.
#[cfg(feature = "dfu_hooks")]
const DFU_STATUS_BANNERS_PACKED: &[u8] = b"\x0dPlug in cable\x0dUSB connected\x0eReady to flash\x0bLink active\x0eFlashing page \x0eFlash complete\x0eUART connected\x11Aborted - reflash";

#[cfg(all(test, feature = "dfu_hooks"))]
pub(crate) fn status_banner(idx: u8) -> &'static str {
    let mut off = 0usize;
    let mut cur = 0u8;
    while off < DFU_STATUS_BANNERS_PACKED.len() {
        let len = DFU_STATUS_BANNERS_PACKED[off] as usize;
        let start = off + 1;
        if cur == idx {
            return core::str::from_utf8(&DFU_STATUS_BANNERS_PACKED[start..start + len])
                .unwrap_or("");
        }
        off = start + len;
        cur += 1;
    }
    ""
}

#[cfg(feature = "dfu_hooks")]
#[inline(always)]
fn banner_plug() -> &'static str {
    unsafe {
        core::str::from_utf8_unchecked(core::slice::from_raw_parts(
            DFU_STATUS_BANNERS_PACKED.as_ptr().add(1),
            13,
        ))
    }
}

/// Extracts USB `((vid << 16) | pid)` and optional `YYYYMMDD` bootloader date from factory ROM
/// (`0x0800_1F00..0x0800_2700`), falling back cleanly if the serial string is absent or malformed.
#[cfg(feature = "dfu_hooks")]
#[inline(never)]
pub(crate) fn parse_bl_rom_meta(rom: &[u8], date_out: &mut [u8; 8]) -> u32 {
    *date_out = [0u8; 8];
    let rom_len = rom.len();
    let p = rom.as_ptr();
    let mut i = 0usize;
    let mut vid_pid = 0x36B7_0001u32;
    let mut i_serial = 0u8;

    unsafe {
        while i + 18 <= rom_len {
            if *p.add(i) == 0x12
                && *p.add(i + 1) == 0x01
                && *p.add(i + 2) == 0x00
                && *p.add(i + 3) == 0x02
                && *p.add(i + 7) == 0x40
            {
                let vid = ((*p.add(i + 9) as u32) << 8) | (*p.add(i + 8) as u32);
                let pid = ((*p.add(i + 11) as u32) << 8) | (*p.add(i + 10) as u32);
                vid_pid = (vid << 16) | pid;
                i_serial = *p.add(i + 16);
                i += 18;
                break;
            }
            i += 1;
        }
        if i_serial == 0 {
            return vid_pid;
        }

        while i + 4 <= rom_len {
            if *p.add(i) == 0x04
                && *p.add(i + 1) == 0x03
                && *p.add(i + 2) == 0x09
                && *p.add(i + 3) == 0x04
            {
                let mut pos = i + 4;
                let mut idx = 1u8;
                while pos + 2 <= rom_len {
                    let len = *p.add(pos) as usize;
                    if len < 2 || (len & 1) != 0 || *p.add(pos + 1) != 0x03 || pos + len > rom_len {
                        break;
                    }
                    if idx == i_serial && len >= 18 {
                        let mut ok = true;
                        let mut k = 0usize;
                        while k < 8 {
                            let lo = *p.add(pos + 2 + k * 2);
                            let hi = *p.add(pos + 3 + k * 2);
                            if hi != 0 || lo.wrapping_sub(b'0') > 9 {
                                ok = false;
                                break;
                            }
                            date_out[k] = lo;
                            k += 1;
                        }
                        if !ok
                            || date_out[0] != b'2'
                            || !(1..=12).contains(
                                &(date_out[4]
                                    .wrapping_sub(b'0')
                                    .wrapping_mul(10)
                                    .wrapping_add(date_out[5].wrapping_sub(b'0'))),
                            )
                            || !(1..=31).contains(
                                &(date_out[6]
                                    .wrapping_sub(b'0')
                                    .wrapping_mul(10)
                                    .wrapping_add(date_out[7].wrapping_sub(b'0'))),
                            )
                        {
                            *date_out = [0u8; 8];
                        }
                        break;
                    }
                    pos += len;
                    idx = idx.saturating_add(1);
                }
                break;
            }
            i += 1;
        }
    }
    vid_pid
}

#[cfg(feature = "dfu_hooks")]
#[inline(never)]
fn read_bl_rom_meta(date_out: &mut [u8; 8]) -> u32 {
    #[cfg(all(not(test), target_arch = "arm"))]
    {
        let rom = unsafe { core::slice::from_raw_parts(0x0800_1F00 as *const u8, 0x0800) };
        parse_bl_rom_meta(rom, date_out)
    }
    #[cfg(not(all(not(test), target_arch = "arm")))]
    {
        *date_out = *b"20250808";
        0x36B7_0001
    }
}

/// Minimal byte-copy loop avoiding inlined `__aeabi_memcpy` expansion across tiny DFU copies.
#[cfg(feature = "dfu_hooks")]
#[inline(never)]
unsafe fn copy_bytes(dst: *mut u8, src: *const u8, len: usize) {
    let mut i = 0;
    while i < len {
        unsafe { *dst.add(i) = *src.add(i) };
        i += 1;
    }
}

/// Bootloader Flash/UI strings with zero redundancy (`DFU` stays inside the radio cutout;
/// bootloader version, USB ID, live transfer status, and `Press [EXIT] to reboot` provide
/// strictly actionable DFU-mode information).
#[cfg(feature = "dfu_hooks")]
const DFU_EXIT_PRE: &str = "Press ";
#[cfg(feature = "dfu_hooks")]
const DFU_EXIT_KEY: &str = "EXIT";
#[cfg(feature = "dfu_hooks")]
const DFU_EXIT_POST: &str = "to reboot";

#[cfg(feature = "dfu_hooks")]
static DFU_ALIGNS: [text::Align; 4] = [
    text::Align::Left,
    text::Align::Center,
    text::Align::Right,
    text::Align::Right,
];

/// Packs `(x, y, align)` into one `u32` so `draw_thumb` passes all arguments in `r0..r3` with zero
/// stack-spill marshaling overhead at each DFU UI call site.
#[cfg(feature = "dfu_hooks")]
#[inline(always)]
const fn pack_xy_align(x: u8, y: u8, align: text::Align) -> u32 {
    let a = match align {
        text::Align::Left => 0u32,
        text::Align::Center => 1u32,
        text::Align::Right => 2u32,
    };
    (x as u32) | ((y as u32) << 8) | (a << 16)
}

#[cfg(feature = "dfu_hooks")]
#[inline(never)]
fn draw_thumb(canvas: &mut Canvas, xy_align: u32, s: &str) {
    text::draw_str_align(
        canvas,
        (xy_align & 0xFF) as i16,
        ((xy_align >> 8) & 0xFF) as i16,
        s,
        &fonts::tomthumb::FONT,
        Color::Fill,
        DFU_ALIGNS[((xy_align >> 16) & 3) as usize],
    );
}

#[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
#[inline(never)]
fn clear_draw_page5(canvas: &mut Canvas, xy_align: u32, s: &str) {
    // Preserve capsule bottom border on Page 5 (`bits 0..1`) and EXIT footer on Page 6 (`bits 3..7`).
    for b in &mut canvas.buffer[5] {
        *b &= 0x03;
    }
    for b in &mut canvas.buffer[6] {
        *b &= 0xF8;
    }
    canvas.dirty |= (1 << 5) | (1 << 6);
    draw_thumb(canvas, xy_align, s);
}

#[cfg(feature = "dfu_hooks")]
#[inline(never)]
fn draw_vid_pid(canvas: &mut Canvas, vid_pid: u32) {
    if vid_pid == 0 {
        return;
    }
    let font = &fonts::tomthumb::FONT;
    let mut line = [core::mem::MaybeUninit::<u8>::uninit(); 9];
    let p_line = line.as_mut_ptr() as *mut u8;
    let mut vp = vid_pid;
    let mut i = 0usize;
    while i < 9 {
        let ch = if i == 4 {
            b':'
        } else {
            let nib = (vp >> 28) as u8;
            vp <<= 4;
            nib + if nib < 10 { b'0' } else { b'A' - 10 }
        };
        unsafe { *p_line.add(i) = ch };
        i += 1;
    }
    let vp_str = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(p_line, 9)) };
    if text::str_width(vp_str, font) <= 44 {
        draw_thumb(canvas, pack_xy_align(93, 13, text::Align::Center), vp_str);
    }
}

#[cfg(feature = "dfu_hooks")]
/// Minimalist, borderless 128x64 DFU interface: raised radio/arrow/USB-C splash (`Y=4..31`),
/// right-side `v<ver> (YYYY-MM-DD)` (`Y=1..5`) and `<VID>:<PID>` centered directly over the
/// USB plug body (`X=93, Y=9..13`) only when active, 6 px capsule bar (`Page 4+5`, `Y=36..41`),
/// live status (`y=49`, `Y=45..49`, descenders `Y=50`), and centered `Press [EXIT] to reboot` (`Y=53..59`).
#[inline(never)]
fn draw_dfu_screen_inner(
    canvas: &mut Canvas,
    bl_ver: &str,
    bl_date: [u8; 8],
    vid_pid: u32,
    keys_held: bool,
) {
    gfx::draw::draw_rle_splash(canvas, &DFU_SPLASH_RLE);
    let font = &fonts::tomthumb::FONT;

    let mut line = [core::mem::MaybeUninit::<u8>::uninit(); 32];
    let p_line = line.as_mut_ptr() as *mut u8;

    // 1. Right-side version + formatted `(YYYY-MM-DD)` date (`Y=1..5`, clear of radio `X<=37`)
    let vlen = bl_ver.len().min(line.len() - 14);
    unsafe {
        *p_line = b'v';
        copy_bytes(p_line.add(1), bl_ver.as_ptr(), vlen);
    }
    let ver_only_len = 1 + vlen;
    let mut hdr_len = ver_only_len;
    if bl_date[0] != 0 {
        unsafe {
            *p_line.add(hdr_len) = b' ';
            *p_line.add(hdr_len + 1) = b'(';
            copy_bytes(p_line.add(hdr_len + 2), bl_date.as_ptr(), 4);
            *p_line.add(hdr_len + 6) = b'-';
            copy_bytes(p_line.add(hdr_len + 7), bl_date.as_ptr().add(4), 2);
            *p_line.add(hdr_len + 9) = b'-';
            copy_bytes(p_line.add(hdr_len + 10), bl_date.as_ptr().add(6), 2);
            *p_line.add(hdr_len + 12) = b')';
        }
        hdr_len += 13;
    }
    let mut hdr_str =
        unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(p_line, hdr_len)) };
    if text::str_width(hdr_str, font) > 84 {
        hdr_str = unsafe {
            core::str::from_utf8_unchecked(core::slice::from_raw_parts(p_line, ver_only_len))
        };
    }
    draw_thumb(canvas, pack_xy_align(125, 5, text::Align::Right), hdr_str);

    // 2. USB VID:PID centered directly on top of the USB bigger rectangle (`X=81..104`, `Y=9..13`)
    draw_vid_pid(canvas, vid_pid);

    // 3. Live status centered at y=49 (`Y=45..49`, descenders `Y=50`), spanning Page 5 + Page 6
    let initial_status = if keys_held {
        "Release side key..."
    } else {
        banner_plug()
    };
    draw_thumb(
        canvas,
        pack_xy_align(64, 49, text::Align::Center),
        initial_status,
    );

    // 4. Centered footer keybind (`Press [EXIT] to reboot`, `Y=53..59`) with normal 5 px word gaps
    const EXIT_PRE_W: i16 = 24;
    const EXIT_BOX_W: i16 = 20;
    const EXIT_POST_W: i16 = 36;
    const EXIT_GAP: i16 = 5;
    const EXIT_X0: i16 = (128 - (EXIT_PRE_W + EXIT_BOX_W + EXIT_GAP + EXIT_POST_W)) >> 1;
    const EXIT_BX: i16 = EXIT_X0 + EXIT_PRE_W;
    text::draw_str(canvas, EXIT_X0, 58, DFU_EXIT_PRE, font, Color::Fill);
    widgets::dialog::draw_key_badge(canvas, EXIT_BX, 53, DFU_EXIT_KEY, "");
    text::draw_str(
        canvas,
        EXIT_BX + EXIT_BOX_W + EXIT_GAP,
        58,
        DFU_EXIT_POST,
        font,
        Color::Fill,
    );
}

#[cfg(feature = "dfu_hooks")]
#[cfg_attr(not(test), allow(dead_code))]
pub fn draw_dfu_screen(canvas: &mut Canvas, bl_ver: &str, vid: u16, pid: u16) {
    draw_dfu_screen_inner(
        canvas,
        bl_ver,
        *b"20250808",
        ((vid as u32) << 16) | (pid as u32),
        false,
    );
}

#[cfg(feature = "dfu_hooks")]
#[inline(always)]
unsafe fn aligned_u16_at(base: *const u8, off: usize) -> u16 {
    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        *(base.add(off) as *const u16)
    }
    #[cfg(not(all(not(test), target_arch = "arm")))]
    unsafe {
        (base.add(off) as *const u16).read_unaligned()
    }
}

#[cfg(feature = "dfu_hooks")]
#[inline(always)]
unsafe fn aligned_u32_at(base: *const u8, off: usize) -> u32 {
    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        *(base.add(off) as *const u32)
    }
    #[cfg(not(all(not(test), target_arch = "arm")))]
    unsafe {
        (base.add(off) as *const u32).read_unaligned()
    }
}

#[cfg(feature = "dfu_hooks")]
#[inline(always)]
const fn is_bl_thumb_addr(addr: u32) -> bool {
    (addr & 1) != 0 && addr.wrapping_sub(0x0800_0010) < 0x27F0
}

/// Decodes `[msp, sys_init, c_main]` from the 4-halfword RVCT `Reset_Handler` sequence
/// (`ldr r0, [pc, #imm]; blx r0; ldr r0, [pc, #imm]; bx r0`) in a bootloader image mapped at
/// `0x0800_0000`. Validates `msp` stays safely below `0x2000_3200` so bootloader stack never
/// collides with SRAM hook state (`0x2000_3400..0x2000_3FFF`).
#[cfg(feature = "dfu_hooks")]
#[inline(never)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn resolve_stock_bl_trampoline(rom: &[u8], out: &mut [u32; 3]) -> bool {
    let rom_len = rom.len();
    if rom_len < 16 {
        return false;
    }
    let base = rom.as_ptr();
    let msp = unsafe { aligned_u32_at(base, 0) };
    let reset = unsafe { aligned_u32_at(base, 4) };
    let pc_off = (reset.wrapping_sub(0x0800_0000) & !1) as usize;
    if msp.wrapping_sub(0x2000_0400) > 0x2A00
        || (reset & 1) == 0
        || pc_off < 8
        || pc_off + 8 > rom_len
    {
        return false;
    }
    out[0] = msp;

    unsafe {
        let mut step = 0usize;
        while step < 2 {
            let off = pc_off + step * 4;
            if *base.add(off + 1) != 0x48
                || aligned_u16_at(base, off + 2) != if step == 0 { 0x4780 } else { 0x4700 }
            {
                return false;
            }
            let lit = ((off + 4) & !3) + ((*base.add(off) as usize) << 2);
            if lit + 4 > rom_len {
                return false;
            }
            let target = aligned_u32_at(base, lit);
            if !is_bl_thumb_addr(target) {
                return false;
            }
            out[step + 1] = target;
            step += 1;
        }
        true
    }
}

/// Exact pixel-for-pixel column bytes of `tomthumb` 3x5 glyphs (`'0'..'9'`, `'/'`, `'%'`, `'b'`, `'p'`, `'s'`)
/// at Page 5 baseline 46 (`Y=42..46`, `bits 2..6`; `'p'` descender on bit 7).
#[cfg(all(test, feature = "dfu_hooks"))]
const TOMTHUMB_3X5_COLS: [u8; 45] = [
    0x78, 0x44, 0x3C, // 0
    0x08, 0x7C, 0x00, // 1
    0x64, 0x54, 0x48, // 2
    0x44, 0x54, 0x28, // 3
    0x1C, 0x10, 0x7C, // 4
    0x5C, 0x54, 0x24, // 5
    0x78, 0x54, 0x74, // 6
    0x64, 0x14, 0x0C, // 7
    0x7C, 0x54, 0x7C, // 8
    0x5C, 0x54, 0x3C, // 9
    0x60, 0x10, 0x0C, // /
    0x24, 0x10, 0x48, // %
    0x7C, 0x48, 0x30, // b
    0xF8, 0x48, 0x30, // p
    0x50, 0x78, 0x28, // s
];

/// Stamp unsigned decimal integer (`0..=9999`) using `TOMTHUMB_3X5_COLS` at cursor `dst[pos..]`
/// (4 px column advance per digit). Used on host tests to verify exact SRAM Thumb-1 output.
#[cfg(all(test, feature = "dfu_hooks"))]
fn stamp_dec_cols(val: u16, dst: &mut [u8], pos: &mut usize) {
    let mut v = val.min(9999);
    let mut started = false;
    for div in [1000u16, 100, 10, 1] {
        let mut d = 0usize;
        while v >= div {
            v -= div;
            d += 1;
        }
        if d != 0 || started || div == 1 {
            started = true;
            if *pos + 3 <= dst.len() {
                dst[*pos..*pos + 3].copy_from_slice(&TOMTHUMB_3X5_COLS[d * 3..d * 3 + 3]);
            }
            *pos += 4;
        }
    }
}

#[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
macro_rules! dfu_sram_isr_asm {
    ($dbg_call:literal, $dbg_sub:literal) => {
        core::arch::global_asm!(
            concat!(
                r#"
    .section .rodata.dfu_hook,"a"
    .balign 4
    .global __dfu_hook_start
    .global __dfu_hook_end
__dfu_hook_start:
    .thumb
    .thumb_func
dfu_systick_isr:
    push {{r4, r5, r6, r7, lr}}

    // 1. Keep PC13 flashlight explicitly LOW even if GPIOC->LCKR is absent on a stepping
    ldr r1, =0x50000400         // GPIOB_BASE
    movs r0, #4
    lsls r0, r0, #8             // 0x400 -> GPIOC_BASE = GPIOB_BASE + 0x400
    lsls r2, r0, #3             // 1 << 13
    adds r0, r0, r1
    str r2, [r0, #0x28]         // GPIOC->BRR = PC13

    ldr r4, =0x200036C0         // DFU_SRAM_STATE
    ldr r5, [r4, #0]            // flags: b0=STOCK_K1, b1=KEY_SEEN_HIGH, b2=USB, b3=WAS_FLASH, b4=ARROW_VIS, b5=BL_OFF, b6=NO_TAP_TOGGLE

    // 2. Side Key 1 (PB15 short press toggles backlight; >= 0.5 s hold reboots) & EXIT (PB3 Col 4 + PB15 Row 0 >= 0.5 s reboots)
    ldr r0, [r1, #0x10]         // IDR
    lsls r0, r0, #16
    bpl 13f                     // PB15 low directly -> Side Key 1 pressed
12:
    movs r2, #8                 // PB3 = Col 4
    str r2, [r1, #0x28]         // GPIOB->BRR = PB3
    ldr r3, [r1, #0x00]         // MODER
    movs r2, #0xC0              // PB3 bits [7:6]
    bics r3, r2
    movs r2, #0x40              // PB3 Output
    orrs r2, r3
    str r2, [r1, #0x00]
    movs r2, #16                // ~1 us keypad matrix trace settling @ 48 MHz
19:
    subs r2, r2, #1
    bne 19b
    ldr r0, [r1, #0x10]         // Sample PB15 with PB3 driven LOW
    str r3, [r1, #0x00]         // Immediately restore PB3 Input
    lsls r0, r0, #16
    bmi 1f                      // Released high
    movs r0, #0x40              // EXIT pressed -> NO_TAP_TOGGLE so release never toggles backlight
    orrs r5, r5, r0
    str r5, [r4, #0]
13:
    movs r0, #2
    tst r5, r0
    beq 3f

    // Block reboot (and any 0x0800_2800 Flash read) when Flash busy (SR_BSY==1), unlocked (CR_LOCK==0), flashing (state==2), or aborted (b3)
    ldr r6, =0x40022010         // FLASH_REGS (+0=SR, +4=CR)
    ldr r0, [r6, #0]            // FLASH->SR
    lsls r0, r0, #31
    bmi 3f
    ldr r0, [r6, #4]            // FLASH->CR
    cmp r0, #0
    bge 3f
    movs r0, #1
    tst r5, r0
    beq 2f
    ldr r0, =0x2000001C
    ldrb r0, [r0, #4]
    cmp r0, #2
    beq 3f
    movs r0, #8
    tst r5, r0
    bne 3f
2:
    ldr r0, [r4, #4]
    adds r0, r0, #1
    str r0, [r4, #4]
    cmp r0, #15
    blo 4f
    movs r0, #0x40              // >= 0.5 s hold: suppress short-press backlight toggle if antibrick blocks
    orrs r5, r5, r0
    str r5, [r4, #0]

    // Hardened antibrick app vector check at 0x0800_2800:
    // - MSP 4-byte aligned in SRAM 0x2000_0400..=0x2000_4000
    // - Reset, NMI, and HardFault vectors all odd Thumb pointers inside 0x0800_2840..0x0802_0000
    // - First word at *(Reset & !3) is neither erased (0xFFFF_FFFF) nor 0x0000_0000
    ldr r0, =0x08002800
    ldr r1, [r0, #0]
    lsls r2, r1, #30
    bne 3f
    ldr r2, =0x20000400
    subs r1, r1, r2
    movs r2, #0x3C
    lsls r2, r2, #8
    cmp r1, r2
    bhi 3f

    movs r3, #4
    ldr r2, =0x1D7C0            // 0x0802_0000 - 0x0800_2840
16:
    ldr r1, [r0, r3]
    lsls r7, r1, #31
    bpl 3f
    subs r1, r1, r0
    subs r1, #0x40
    cmp r1, r2
    bhs 3f
    adds r3, r3, #4
    cmp r3, #16
    blo 16b

    ldr r1, [r0, #4]
    movs r2, #3
    bics r1, r1, r2
    ldr r1, [r1, #0]
    adds r2, r1, #1
    beq 3f
    cmp r1, #0
    beq 3f

    // Valid firmware verified: reboot cleanly
    b dfu_reboot

1:
    // Released high: short Side Key 1 press (1 <= hold < 15, KEY_SEEN_HIGH set, !NO_TAP_TOGGLE) toggles PF8 backlight
    movs r0, #0x42
    ands r0, r0, r5
    cmp r0, #2
    bne 30f
    ldr r0, [r4, #4]
    cmp r0, #1
    blo 30f
    cmp r0, #15
    bhs 30f
    movs r0, #0x20
    eors r5, r5, r0             // toggle BL_OFF (flags bit 5)
    ldr r2, =0x50001400         // GPIOF_BASE
    movs r1, #1
    lsls r1, r1, #8             // PF8
    tst r5, r0
    beq 32f
    str r1, [r2, #0x28]         // GPIOF->BRR (backlight OFF)
    b 30f
32:
    str r1, [r2, #0x18]         // GPIOF->BSRR (backlight ON)
30:
    movs r0, #2                 // KEY_SEEN_HIGH = 1
    orrs r5, r5, r0
    movs r0, #0x40              // clear NO_TAP_TOGGLE
    bics r5, r5, r0
    str r5, [r4, #0]
3:
    movs r0, #0
    str r0, [r4, #4]            // key_hold_ticks = 0
4:
    // 3. Require known stock K1 DFU state struct before touching USB/UART/LCD state
    movs r0, #1
    tst r5, r0
    bne 11f
    pop {{r4, r5, r6, r7, pc}}
11:
    ldr r6, [r4, #8]            // r6 = tick
    adds r0, r6, #1
    str r0, [r4, #8]

    // Track mid-flash abort antibrick latch: set b3 when state==2, clear when state==3
    ldr r1, =0x2000001C
    ldrb r0, [r1, #4]           // 0x2000_0020 (state)
    cmp r0, #2
    bne 14f
    movs r0, #8
    orrs r5, r5, r0
    b 15f
14:
    cmp r0, #3
    bne 15f
    movs r0, #8
    bics r5, r5, r0
15:

    // 4. Detect live USB link (RCC_APB1ENR.USBDEN + USBD_ADDR + advancing USBD_FRAME)
    movs r7, #0
    ldr r0, =0x4002103C
    ldr r0, [r0]
    lsrs r0, r0, #24
    bcc 42f
    ldr r1, =0x40005C00
    ldrb r0, [r1, #0]
    lsls r0, r0, #25
    beq 42f
    ldrh r0, [r1, #0x0C]
    ldrh r2, [r4, #16]
    strh r0, [r4, #16]
    cmp r0, r2
    beq 40f
    movs r0, #0
    strb r0, [r4, #18]
    movs r7, #1
    b 43f
40:
    ldrb r0, [r4, #18]
    cmp r0, #4
    bhs 42f
    adds r0, r0, #1
    strb r0, [r4, #18]
    lsrs r7, r5, #2
    movs r0, #1
    ands r7, r7, r0
    b 43f
42:
    movs r0, #4
    strb r0, [r4, #18]
43:
    movs r0, #4
    bics r5, r5, r0
    lsls r0, r7, #2
    orrs r5, r5, r0
    str r5, [r4, #0]

    // 4b. Smart UART connection timer ([r4, #19], 0..=150 = 5 s idle hold):
    // USB active resets UART cleanly; otherwise gated on USART1EN+UE so pre-init SRAM never
    // false-triggers, and refreshes on >= 2 new DMA RX bytes or active UART flashing (state == 2).
    ldrb r1, [r4, #19]
    cmp r7, #0
    beq 440f
    movs r1, #0
    b 443f
440:
    cmp r1, #0
    beq 441f
    subs r1, r1, #1
441:
    ldr r0, =0x40021040
    ldr r0, [r0]
    lsrs r0, r0, #15            // bit 14 = USART1EN
    bcc 443f
    ldr r0, =0x40013800
    ldr r0, [r0, #0x0C]         // USART1->CR1
    lsrs r0, r0, #14            // bit 13 = UE
    bcc 443f

    ldr r0, =0x40020020         // DMA1_Channel2->CNDTR (2048 circular RX ring)
    ldrh r0, [r0, #0]
    ldrh r2, [r4, #48]          // prev_cndtr (starts at 2048)
    subs r3, r2, r0
    lsls r3, r3, #21
    lsrs r3, r3, #21            // (prev_cndtr - cur_cndtr) & 0x7FF
    cmp r3, #2
    blo 442f
    strh r0, [r4, #48]
    b 444f
442:
    ldr r2, =0x2000001C
    ldrb r2, [r2, #4]           // state
    cmp r2, #2
    bne 443f
444:
    movs r1, #150               // 5 s @ 30 Hz
443:
    strb r1, [r4, #19]

    // Track serial/command activity pulse across USB + UART only while USART1 is armed
    ldr r0, =0x40013800
    ldr r0, [r0, #0x0C]
    lsrs r0, r0, #14
    bcc 46f
    ldr r2, =0x2000001C
    ldr r0, =0x200009A0
    ldrh r0, [r0, #0]
    ldr r1, =0x200019E0
    ldrh r1, [r1, #0]
    lsls r1, r1, #16
    eors r0, r0, r1
    ldr r1, [r2, #0]
    eors r0, r0, r1
    ldr r1, =0x40020020
    ldrh r1, [r1, #0]
    eors r0, r0, r1
    ldr r1, [r4, #32]
    cmp r0, r1
    beq 45f
    str r0, [r4, #32]
    movs r0, #12
    strb r0, [r4, #31]
    b 46f
45:
    ldrb r0, [r4, #31]
    cmp r0, #0
    beq 46f
    subs r0, r0, #1
    strb r0, [r4, #31]
46:
"#,
                $dbg_call,
                r#"
    // 5. Compute Page 4 capsule [fill_start..fill_end), banner_idx (r7 = 0..=7), flash timer, and abort/stall state
    // Stack temp words: [sp, #0]=cur_page_1based, [sp, #4]=tot_pages, [sp, #8]=cutout_val, [sp, #12]=usb_active
    sub sp, #16
    str r7, [sp, #12]
    movs r0, #0
    str r0, [sp, #0]
    mvns r0, r0
    str r0, [sp, #8]

    ldr r2, =0x2000001C         // [+4]=state, [+5]=hs, [+6]=cur, [+8]=tot
    ldrb r0, [r2, #4]
    cmp r0, #3
    beq 5f
    cmp r0, #2
    bne 6f

    // Flashing active -> reset elapsed cache, latch flash_start_tick, detect >= 10s mid-flash stall
    movs r0, #0
    mvns r0, r0
    strh r0, [r4, #36]          // flash_elapsed_s = 0xFFFF
    ldr r0, [r4, #24]
    cmp r0, #0
    bne 53f
    ldr r0, [r4, #8]            // updated tick (>= 1)
    str r0, [r4, #24]
53:
    ldrh r0, [r2, #6]           // cur
    ldrh r1, [r4, #44]          // last_flash_cur
    cmp r0, r1
    beq 54f
    strh r0, [r4, #44]
    movs r1, #0
    strh r1, [r4, #46]          // stall_ticks = 0
    b 55f
54:
    cmp r0, #0
    beq 55f
    ldrh r1, [r4, #46]
    movs r3, #150
    lsls r3, r3, #1             // 300 ticks = 10 s @ 30 Hz
    cmp r1, r3
    bhs 55f
    adds r1, r1, #1
    strh r1, [r4, #46]
55:
    movs r7, #4                 // banner 4: "Flashing page X/Y"
    ldrh r1, [r4, #46]
    movs r3, #150
    lsls r3, r3, #1
    cmp r1, r3
    blo 56f
    movs r7, #7                 // banner 7: "Aborted - reflash" (stalled)
56:
    adds r0, r0, #1             // cur_page_1based
    str r0, [sp, #0]
    ldrh r1, [r2, #8]           // tot_pages
    str r1, [sp, #4]

    // Shared pct calculation: r0=num, r1=den -> r2=0..=100, then fall into 51:
50:
    movs r3, #100
    muls r0, r3, r0
    movs r2, #0
    cmp r1, #0
    beq 51f
501:
    cmp r2, #100
    bhs 51f
    cmp r0, r1
    blo 51f
    subs r0, r0, r1
    adds r2, r2, #1
    b 501b
51:
    str r2, [sp, #8]
    movs r5, #0
    movs r6, r2
    b 7f
    .ltorg

5:
    // Flash complete -> latch elapsed seconds once, alternate "100%" / "<N>s" every ~1.06 s
    ldrh r0, [r4, #36]
    adds r1, r0, #1
    lsls r1, r1, #16
    bne 58f
    ldr r1, [r4, #24]
    movs r0, #0
    cmp r1, #0
    beq 57f
    ldr r0, [r4, #8]
    subs r0, r0, r1
    adds r0, r0, #15
    movs r1, #30
    bl udiv32
    movs r1, #125
    lsls r1, r1, #3             // 1000
    cmp r0, r1
    blo 57f
    subs r0, r1, #1             // clamp to 999 s
57:
    strh r0, [r4, #36]
    movs r1, #0
    str r1, [r4, #24]
    strh r1, [r4, #46]
58:
    movs r2, #100
    movs r1, #32
    tst r6, r1
    beq 59f
    ldrh r2, [r4, #36]
    movs r1, #4
    lsls r1, r1, #8             // 0x400 marker -> "<N>s"
    orrs r2, r2, r1
59:
    str r2, [sp, #8]
    movs r5, #0
    movs r6, #100
    movs r7, #5                 // banner 5: "Flash complete"
    b 7f

6:
    // Check mid-flash abort (WAS_FLASH latched while state != 2 && state != 3)
    ldr r0, [r4, #0]
    movs r1, #8
    tst r0, r1
    beq 66f
    movs r0, #0
    strh r0, [r4, #46]
    ldrh r0, [r2, #6]
    ldrh r1, [r2, #8]
    movs r7, #7                 // banner 7: "Aborted - reflash"
    b 50b

66:
    // If Flash is already unlocked (CR_LOCK==0) or busy erasing before state==2 settles,
    // dock the capsule cleanly at 0% ([0..2)) so the comet/ping-pong pill never freezes mid-bar.
    ldr r3, =0x40022010
    ldr r0, [r3, #4]            // FLASH->CR
    cmp r0, #0
    bge 691f
    ldr r0, [r3, #0]            // FLASH->SR
    lsls r0, r0, #31
    bpl 660f
691:
    movs r5, #0
    movs r6, #2
    movs r7, #2
    b 7f
660:
    ldrb r2, [r2, #5]           // hs (0/1)
    ldrb r3, [r4, #31]          // serial_pulse (0..12)
    cmp r3, #0
    beq 68f
    // Serial activity burst -> fast directional comet pill (16 px)
    lsls r0, r6, #1
    adds r0, r0, r6
    movs r1, #127
    ands r0, r0, r1
    cmp r0, #84
    bls 67f
    subs r0, r0, #84
67: movs r5, r0
    movs r1, #16
    movs r7, #3                 // banner 3: "Link active"
    cmp r2, #0
    beq 65f
    movs r7, #2                 // keep "Ready to flash" once handshake completed
    b 65f

68:
    ldrb r0, [r4, #19]          // uart_latched
    movs r1, r7
    orrs r1, r1, r0
    orrs r1, r1, r2
    bne 60f
    lsrs r6, r6, #1             // idle standby: slow gentle ping-pong
60:
    movs r0, #63
    ands r0, r0, r6
    movs r1, #64
    tst r6, r1
    beq 61f
    movs r1, #63
    subs r0, r1, r0
61: lsrs r1, r0, #1
    adds r5, r0, r1             // 0..=94

    cmp r2, #0
    beq 62f
    // Handshake done -> banner 2 ("Ready to flash"), 28 px pill
    movs r1, #28
    movs r7, #2
    b 69f
62:
    cmp r7, #0
    beq 63f
    movs r1, #14                // USB connected -> banner 1, 14 px pill
    b 69f
63:
    ldrb r0, [r4, #19]
    cmp r0, #0
    beq 64f
    movs r1, #14                // UART connected -> banner 6, 14 px pill
    movs r7, #6
    b 69f
64:
    movs r1, #8                 // Standby -> banner 0, 8 px pill
    movs r7, #0
69:
    movs r0, #100
    subs r0, r0, r1
    cmp r5, r0
    bls 65f
    movs r5, r0
65:
    movs r6, r5
    adds r6, r6, r1

7:
    // 6a. Page 4 capsule progress bar redraw on diff (dashed pattern on abort/stall r7 == 7)
    lsls r2, r5, #8
    orrs r2, r2, r6
    lsls r0, r7, #24
    orrs r2, r2, r0
    ldr r3, [r4, #12]
    cmp r3, r2
    beq 78f
    str r2, [r4, #12]

    adds r5, r5, #2
    adds r6, r6, #2
    movs r0, #0xB4
    movs r1, #16                // HW col 12 + 4 = 16
    bl st7565_begin_page
    movs r0, #0xE0              // left capsule cap (col 12, Y=37..39 -> 0xE0 on P4)
    bl spi_tx
    movs r2, #1
71:
    movs r0, #0x10              // sleek 6 px empty capsule top border (Y=36 -> P4 bit 4)
    cmp r2, r5
    blo 74f
    cmp r2, r6
    bhs 74f
    movs r0, #0xD0              // 2 px inner pill fill (`Y=38..39`: 0x10 | 0xC0)
    cmp r7, #7
    bne 74f
    movs r1, #2
    tst r2, r1                  // Aborted: 2 px dashed pattern (`0xD0, 0xD0, 0x10, 0x10`)
    bne 74f
    movs r0, #0x10
74:
    bl spi_tx
    adds r2, r2, #1
    cmp r2, #103
    blo 71b
    movs r0, #0xE0              // right capsule cap (col 115)
    bl spi_tx
    bl st7565_end

78:
    // 6b. Page 5 + Page 6 dynamic status banner redraw on (banner << 12) | cur_page_lsb change
    lsls r0, r7, #12
    ldr r1, [sp, #0]
    lsls r1, r1, #20
    lsrs r1, r1, #20
    orrs r0, r0, r1
    ldrh r1, [r4, #28]
    cmp r0, r1
    beq 79f
    strh r0, [r4, #28]

    ldr r5, =0x20003000         // DFU_SRAM_BANNERS: 8 x [84 B P5 + 84 B P6] (`cols 22..105`)
    movs r0, #168
    muls r0, r0, r7
    adds r5, r5, r0

    // When flashing (banner == 4), stamp dynamic "<cur>/<tot>" at X=76 (+54 inside slice) across P5 & P6
    cmp r7, #4
    bne 76f
    movs r0, #0
    movs r1, #0
    movs r2, #0
    movs r3, #0
    push {{r0, r1, r2, r3}}
    push {{r0, r1, r2, r3}}
    mov r4, sp
    ldr r0, [sp, #32]           // cur_page_1based
    bl stamp_dec
    movs r0, #10                // '/'
    bl stamp_glyph_3x5
    ldr r0, [sp, #36]           // tot_pages
    bl stamp_dec

    adds r5, #54
    mov r6, r5
    adds r6, #84
    movs r4, #7
    movs r2, #0
75:
    mov r0, sp
    ldrb r3, [r0, r2]           // raw 3x5 glyph byte (bits 2..6 at y=46)
    ldrb r1, [r5, r2]
    ands r1, r1, r4             // preserve capsule bottom border (P5 bits 0..1; bit 2 clear)
    lsls r0, r3, #3             // -> y=49 Page 5 (bits 5..7)
    orrs r1, r1, r0
    strb r1, [r5, r2]

    ldrb r1, [r6, r2]
    bics r1, r1, r4             // clear P6 bits 0..2, preserve EXIT (bits 5..7)
    lsrs r3, r3, #5             // -> y=49 Page 6 (bits 0..1)
    orrs r1, r1, r3
    strb r1, [r6, r2]

    adds r2, r2, #1
    cmp r2, #28
    blo 75b
    subs r5, #54
    add sp, #32
    ldr r4, =0x200036C0

76:
    movs r0, #0xB5
    movs r1, #26                // HW col 22 + 4 = 26
    movs r2, r5
    movs r3, #84
    bl st7565_write_strip
    adds r5, #84
    movs r0, #0xB6
    movs r1, #26
    movs r2, r5
    movs r3, #84
    bl st7565_write_strip

79:
    // 6c. Page 1+2 radio LCD cutout ("0%".."100%" or "<N>s", Y=13..17, X=17..=32) on change
    ldr r5, [sp, #8]
    lsrs r0, r5, #10
    cmp r0, #1
    beq 81f
    cmp r5, #100
    bhi 96f
    movs r6, r5
    b 800f
81:
    lsls r6, r5, #22
    lsrs r6, r6, #22
800:
    ldrh r0, [r4, #38]
    cmp r5, r0
    beq 96f
    strh r5, [r4, #38]

    movs r0, #0
    movs r1, #0
    movs r2, #0
    movs r3, #0
    push {{r0, r1, r2, r3}}     // 16 B raw y=46 glyph strip at [sp, #0..16]
    mov r4, sp
    movs r0, #0
    cmp r6, #100
    bhs 82f
    adds r0, r0, #2
    cmp r6, #10
    bhs 82f
    adds r0, r0, #2
82: adds r4, r4, r0
    movs r0, r6
    bl stamp_dec
    movs r0, #11                // '%'
    lsrs r1, r5, #10
    cmp r1, #1
    bne 83f
    movs r0, #14                // 's'
83: bl stamp_glyph_3x5

    // Stream Page 1 (pass 0: << 3 + 0x06 bezel / 0x07 antenna cols 0..2) and Page 2 (pass 1: >> 5 + 0x70 bezel / 0xF0 cols 0,15).
    movs r4, #0
101:
    movs r0, #0xB1
    adds r0, r0, r4
    movs r1, #21                // HW col 17 + 4 = 21
    bl st7565_begin_page        // sets r3=SPI1_BASE, CS=0, A0=1; spi_tx preserves r2, r4..r7
    mov r5, sp
    movs r6, #0
102:
    ldrb r0, [r5, r6]
    cmp r4, #0
    bne 103f
    lsls r0, r0, #3
    movs r1, #6
    cmp r6, #3
    bhs 105f
    adds r1, r1, #1
    b 105f
103:
    lsrs r0, r0, #5
    movs r1, #0x70
    cmp r6, #0
    beq 104f
    cmp r6, #15
    bne 105f
104:
    movs r1, #0xF0
105:
    orrs r0, r0, r1
    bl spi_tx
    adds r6, r6, #1
    cmp r6, #16
    blo 102b
    bl st7565_end
    adds r4, r4, #1
    cmp r4, #2
    blo 101b
    add sp, #16
    ldr r4, =0x200036C0

96:
    // 6c-top. Page 1 top label (`y=13, X=73..=112`):
    // r5 = 0 (blank when disconnected), 1 (VID:PID when USB connected), >= 1200 (<baud>bps when UART connected)
    movs r5, #0
    ldr r0, [sp, #12]           // usb_active
    cmp r0, #0
    beq 808f
    movs r5, #1
    b 810f
808:
    ldrb r0, [r4, #19]          // uart_timer
    cmp r0, #0
    beq 810f
    ldr r0, =0x40021040         // RCC_APB2ENR
    ldr r0, [r0]
    lsrs r0, r0, #15            // bit 14 = USART1EN
    bcc 810f
    ldr r0, =0x40013800         // USART1_BASE
    ldr r1, [r0, #0x0C]         // CR1
    lsrs r1, r1, #14            // bit 13 = UE
    bcc 810f
    ldrh r1, [r0, #0x08]        // BRR
    cmp r1, #49                 // 48e6/49 = 979_591 < 1_000_000 (fits "NNNNNNbps" in 40 cols)
    blo 810f
    ldr r0, =48000000
    bl udiv32
    movs r1, #150
    lsls r1, r1, #3             // 1200
    lsrs r2, r1, #1             // 600
    adds r0, r0, r2
    bl udiv32
    movs r1, #150
    lsls r1, r1, #3             // 1200
    muls r0, r0, r1
    cmp r0, r1
    blo 810f
    movs r5, r0
810:
    ldr r0, [r4, #40]           // last_top_key
    cmp r5, r0
    beq 819f
    str r5, [r4, #40]
    cmp r5, #1
    bne 811f
    movs r0, #0xB1
    movs r1, #77                // HW col 73 + 4 = 77
    ldr r2, =0x20003580         // DFU_SRAM_VIDPID (40 B)
    movs r3, #40
    bl st7565_write_strip
    b 819f
811:
    movs r0, #0
    movs r1, #0
    movs r2, #0
    push {{r0, r1, r2}}
    push {{r0, r1, r2}}
    push {{r0, r1, r2}}         // 36 B zeroed raw y=46 strip (also blank fill when r5 == 0)
    cmp r5, #0
    beq 814f
    mov r4, sp
    movs r0, r5
    bl stamp_dec
    movs r0, #12                // 'b'
    bl stamp_glyph_3x5
    movs r0, #13                // 'p'
    bl stamp_glyph_3x5
    movs r0, #14                // 's'
    bl stamp_glyph_3x5
    mov r0, sp
    subs r1, r4, r0             // len = 4 * n_chars
    movs r5, #42
    subs r5, r5, r1
    lsrs r5, r5, #1             // left_col = (42 - len) >> 1 (centered at X=93)
    adds r6, r5, r1             // end_col
    b 815f
814:
    movs r5, #40
    movs r6, #0
815:
    movs r0, #0xB1
    movs r1, #77                // HW col 73 + 4 = 77
    bl st7565_begin_page
    mov r4, sp
    movs r2, #0
812:
    movs r0, #0
    cmp r2, r5
    blo 813f
    cmp r2, r6
    bhs 813f
    subs r1, r2, r5
    ldrb r0, [r4, r1]
    lsrs r0, r0, #1             // shift y=46 -> y=13 Page 1 (bits 1..5, 'p' descender bit 6)
813:
    bl spi_tx
    adds r2, r2, #1
    cmp r2, #40
    blo 812b
    bl st7565_end
    add sp, #36
    ldr r4, =0x200036C0

819:
    // 6d. Blink splash arrow (P2/P3 cols 45..=62, HW col 49) only in Standby (r7 == 0)
    ldr r5, [r4, #0]            // flags
    movs r6, #1                 // desired arrow_vis = 1 whenever USB/UART/flashing/complete/aborted
    cmp r7, #0
    bne 97f
    ldr r0, [r4, #8]            // tick
    lsrs r0, r0, #4
    eors r6, r6, r0             // invert ~1 Hz bit
97:
    movs r1, #1
    ands r6, r6, r1
    lsrs r0, r5, #4
    ands r0, r0, r1             // previous arrow_vis (flags bit 4)
    cmp r0, r6
    beq 900f
    movs r0, #16
    eors r5, r5, r0             // toggle flags bit 4 cleanly to match r6
    str r5, [r4, #0]

    movs r5, #0
    cmp r6, #0
    beq 98f
    ldr r5, =0x20003540         // DFU_SRAM_ARROW (P2 +0, P3 +19)
98:
    movs r6, #0xB2
94:
    movs r0, r6
    movs r1, #49                // HW col 45 + 4 = 49
    movs r2, r5
    movs r3, #19
    bl st7565_write_strip
    cmp r5, #0
    beq 93f
    adds r5, r5, #19
93:
    adds r6, r6, #1
    cmp r6, #0xB4
    blo 94b

900:
    // 6e. BK4829 REG_33 status LED choreography (updated only on change):
    // 0=Off, 1=Red, 2=Green, 3=Orange
    movs r0, #2                 // connected / ready / complete -> solid Green
    cmp r7, #7
    bne 901f
    ldr r1, [r4, #8]            // Aborted / stalled -> fast Red blink (~3.7 Hz)
    movs r0, #0
    movs r2, #4
    tst r1, r2
    beq 905f
    movs r0, #1
    b 905f
901:
    cmp r7, #4
    bne 902f
    ldr r1, [sp, #0]            // Flashing -> Green/Orange alternating every flash page
    movs r0, #1
    ands r0, r0, r1
    adds r0, r0, #2
    b 905f
902:
    cmp r7, #3
    bne 903f
    ldr r1, [r4, #8]            // Link active -> Green/Orange flutter
    lsrs r1, r1, #1
    movs r0, #1
    ands r0, r0, r1
    adds r0, r0, #2
    b 905f
903:
    cmp r7, #0
    bne 905f
    ldr r1, [r4, #8]            // Standby -> subtle Green heartbeat blip every ~2.1 s
    movs r2, #0x3F
    ands r1, r1, r2
    movs r0, #0
    cmp r1, #3
    bhs 905f
    movs r0, #2
905:
    ldrb r1, [r4, #30]
    cmp r0, r1
    beq 99f
    strb r0, [r4, #30]
    bl bk4829_set_led

99:
    add sp, #16
    pop {{r4, r5, r6, r7, pc}}
    .ltorg

dfu_reboot:
    movs r0, #0
    bl bk4829_set_led
    cpsid i
    ldr r3, =0xE000E010
    movs r1, #0
    str r1, [r3]
    movs r2, #0x17
    lsls r2, r2, #4             // 0xE000_E180 (NVIC->ICER[0])
    mvns r1, r1
    str r1, [r3, r2]
    ldr r0, =0x4002103C
    subs r0, #0x10              // 0x4002_102C (RCC->APB1RSTR)
    movs r1, #1
    lsls r1, r1, #23
    str r1, [r0]
    movs r1, #0
    str r1, [r0]
    ldr r0, =0x50001428
    movs r1, #1
    lsls r1, r1, #8
    str r1, [r0]
    dsb
    ldr r0, =0xE000ED0C
    ldr r1, =0x05FA0004
    str r1, [r0]
    dsb
0:  b 0b
    .ltorg

// Bitbang BK4829 REG_33 status LED: r0 = led (0=Off, 1=Red, 2=Green, 3=Orange); clobbers r0..r3
bk4829_set_led:
    push {{r4, r5, r6, lr}}
    ldr r2, =0x50000400         // GPIOB_BASE
    movs r1, #16
    lsls r1, r1, #8             // +0x1000
    adds r1, r2, r1             // GPIOF_BASE = 0x5000_1400
    // Ensure PB8 (SCL) & PB9 (SDA) and PF9 (CSN) are push-pull outputs
    movs r3, #0xF
    lsls r3, r3, #16
    ldr r4, [r2, #0]
    bics r4, r3
    movs r5, #5
    lsls r5, r5, #16
    orrs r4, r5
    str r4, [r2, #0]
    movs r3, #3
    lsls r3, r3, #18
    ldr r4, [r1, #0]
    bics r4, r3
    movs r5, #1
    lsls r5, r5, #18
    orrs r4, r5
    str r4, [r1, #0]

    movs r4, #1
    lsls r4, r4, #8             // PB8 = BK_SCL
    str r4, [r2, #0x28]         // SCL = 0
    lsls r5, r4, #1             // PB9 = BK_SDA / PF9 = BK_CSN
    str r5, [r1, #0x28]         // CSN = 0
    movs r3, #8
201: subs r3, r3, #1
    bne 201b

    ldr r3, =0x33900000         // (REG 0x33 << 24) | (0x9000 << 8)
    lsls r0, r0, #8
    orrs r0, r0, r3
    movs r6, #24
202:
    lsls r0, r0, #1
    bcs 203f
    str r5, [r2, #0x28]         // SDA = 0
    b 204f
203:
    str r5, [r2, #0x18]         // SDA = 1
204:
    movs r3, #4
205: subs r3, r3, #1
    bne 205b
    str r4, [r2, #0x18]         // SCL = 1
    movs r3, #8
206: subs r3, r3, #1
    bne 206b
    str r4, [r2, #0x28]         // SCL = 0
    movs r3, #4
207: subs r3, r3, #1
    bne 207b
    subs r6, r6, #1
    bne 202b
    str r5, [r1, #0x18]         // CSN = 1
    pop {{r4, r5, r6, pc}}

// Unsigned 32-bit restoring division: r0 = r0 / r1 (requires 0 < r1 < 0x8000_0000); clobbers r1..r3
udiv32:
    movs r2, #0
    movs r3, #0
    push {{r4}}
    movs r4, #32
210:
    lsls r0, r0, #1
    adcs r3, r3, r3
    lsls r2, r2, #1
    cmp r3, r1
    blo 211f
    subs r3, r3, r1
    adds r2, r2, #1
211:
    subs r4, r4, #1
    bne 210b
    movs r0, r2
    pop {{r4}}
    bx lr

// Stamp unsigned integer r0 (0..=999_999) into cursor r4 (advancing r4 += 4 per digit)
stamp_dec:
    push {{r5, r6, r7, lr}}
    movs r6, r0
    movs r7, #0
    adr r5, stamp_divs
    movs r2, #0
85: ldr r3, [r5, r2]
    movs r0, #0
86: cmp r6, r3
    blo 87f
    subs r6, r6, r3
    adds r0, r0, #1
    b 86b
87: cmp r0, #9
    bls 84f
    movs r0, #9
84: movs r1, r0
    orrs r1, r7
    bne 88f
    cmp r3, #1
    bne 89f
88: movs r7, #1
    bl stamp_glyph_3x5
89: adds r2, r2, #4
    cmp r2, #24
    blo 85b
    pop {{r5, r6, r7, pc}}

// Leaf stamper: glyph index r0 (0..=14) at cursor r4, advancing r4 += 4; preserves r2, clobbers r0/r1/r3
stamp_glyph_3x5:
    adr r1, stamp_glyphs
    lsls r3, r0, #1
    adds r0, r0, r3
    adds r1, r1, r0
    ldrb r3, [r1, #0]
    strb r3, [r4, #0]
    ldrb r3, [r1, #1]
    strb r3, [r4, #1]
    ldrb r3, [r1, #2]
    strb r3, [r4, #2]
    adds r4, r4, #4
    bx lr

// r0=page_cmd, r1=hw_col, r2=ptr(or 0 for zeros), r3=len
st7565_write_strip:
    push {{r4, r5, lr}}
    movs r4, r2
    movs r5, r3
    bl st7565_begin_page
    movs r2, #0
90:
    movs r0, #0
    cmp r4, #0
    beq 95f
    ldrb r0, [r4, r2]
95:
    bl spi_tx
    adds r2, r2, #1
    cmp r2, r5
    blo 90b
    bl st7565_end
    pop {{r4, r5, pc}}

// r0=page_cmd, r1=hw_col; sets r3=SPI1_BASE, CS=0, A0=1 (spi_tx preserves r2, r4..r7)
st7565_begin_page:
    push {{r4, r5, r6, lr}}
    movs r4, r0
    lsrs r5, r1, #4
    movs r0, #0x10
    orrs r5, r5, r0             // col_hi
    movs r0, #0x0F
    ands r1, r1, r0
    movs r6, r1                 // col_lo
    ldr r3, =0x40013000
    ldr r1, =0x50000400         // GPIOB_BASE
    movs r0, #4
    lsls r2, r0, #8             // 0x400
    subs r2, r1, r2             // GPIOA_BASE = 0x5000_0000
    str r0, [r1, #0x28]         // CS = 0 (PB2)
    movs r1, #64
    str r1, [r2, #0x28]         // A0 = 0 (PA6)
    movs r0, r4
    bl spi_tx
    movs r0, r5
    bl spi_tx
    movs r0, r6
    bl spi_tx
80: ldr r1, [r3, #0x08]
    lsls r1, r1, #24
    bmi 80b
    movs r1, #64
    str r1, [r2, #0x18]         // A0 = 1
    pop {{r4, r5, r6, pc}}

st7565_end:
    ldr r0, [r3, #0x08]
    lsls r0, r0, #24
    bmi st7565_end
    ldr r0, =0x50000400
    movs r1, #4
    str r1, [r0, #0x18]         // CS = 1
    bx lr

// Sends byte r0 on SPI1 (r3=SPI1_BASE); preserves r2..r7
spi_tx:
    ldr r1, [r3, #0x08]
    lsrs r1, r1, #2
    bcc spi_tx
    strb r0, [r3, #0x0C]
    bx lr

    .balign 4
stamp_divs:
    .word 100000, 10000, 1000, 100, 10, 1
    .balign 4
stamp_glyphs:
    .byte 0x78, 0x44, 0x3C      // 0
    .byte 0x08, 0x7C, 0x00      // 1
    .byte 0x64, 0x54, 0x48      // 2
    .byte 0x44, 0x54, 0x28      // 3
    .byte 0x1C, 0x10, 0x7C      // 4
    .byte 0x5C, 0x54, 0x24      // 5
    .byte 0x78, 0x54, 0x74      // 6
    .byte 0x64, 0x14, 0x0C      // 7
    .byte 0x7C, 0x54, 0x7C      // 8
    .byte 0x5C, 0x54, 0x3C      // 9
    .byte 0x60, 0x10, 0x0C      // /
    .byte 0x24, 0x10, 0x48      // %
    .byte 0x7C, 0x48, 0x30      // b
    .byte 0xF8, 0x48, 0x30      // p
    .byte 0x50, 0x78, 0x28      // s
"#,
                $dbg_sub,
                r#"
    .ltorg
__dfu_hook_end:
"#
            )
        );
    };
}

#[cfg(all(
    feature = "dfu_hooks",
    not(test),
    target_arch = "arm",
    not(feature = "settings_debug")
))]
dfu_sram_isr_asm!("", "");

// When `settings_debug` is active, stream live DFU handshake/block/opcode transitions over USART1
// (only when USB is active and no UART DFU frames have arrived, preventing wire corruption).
#[cfg(all(
    feature = "dfu_hooks",
    not(test),
    target_arch = "arm",
    feature = "settings_debug"
))]
dfu_sram_isr_asm!(
    "    bl dbg_log_transition\n",
    r#"
dbg_log_transition:
    push {{r4, r5, r6, r7, lr}}
    cmp r7, #0
    beq 89f
    ldrb r0, [r4, #19]
    cmp r0, #0
    bne 89f
    ldr r5, =0x40013800
    ldr r0, [r5, #0x0C]
    lsrs r0, r0, #14
    bcc 89f
    ldr r6, =0x20000020
    ldr r7, =0x200009A0
    ldrb r0, [r6, #0]
    ldrb r1, [r6, #1]
    lsls r0, r0, #24
    lsls r1, r1, #28
    orrs r0, r0, r1
    ldrh r1, [r6, #2]
    orrs r0, r0, r1
    ldrh r1, [r7, #0]
    lsls r1, r1, #8
    eors r0, r0, r1
    ldr r1, [r4, #20]
    cmp r0, r1
    beq 89f
    str r0, [r4, #20]

    movs r0, #0x44              // 'D'
    bl uart_putc
    movs r0, #0x20              // ' '
    bl uart_putc
    ldrb r0, [r6, #0]
    lsls r0, r0, #4
    ldrb r1, [r6, #1]
    orrs r0, r0, r1
    movs r1, #2
    movs r2, #0x20
    bl uart_hex                 // "<state><hs> "
    ldrh r0, [r6, #2]
    movs r1, #4
    movs r2, #0x2F              // '/'
    bl uart_hex                 // "<cmd>/"
    ldrh r0, [r6, #4]
    movs r1, #4
    movs r2, #0x20
    bl uart_hex                 // "<blk> "
    ldrh r0, [r7, #0]
    movs r1, #4
    movs r2, #0x0D              // '\r'
    bl uart_hex
    movs r0, #0x0A              // '\n'
    bl uart_putc
89:
    pop {{r4, r5, r6, r7, pc}}

// Prints r1 hex digits of r0, then separator char r2 (if r2 != 0)
uart_hex:
    push {{r2, r4, r6, lr}}
    movs r4, r0
    movs r6, r1
    lsls r1, r6, #2
    movs r2, #32
    subs r2, r2, r1
    lsls r4, r4, r2
91:
    lsrs r0, r4, #28
    cmp r0, #10
    blo 92f
    adds r0, r0, #7
92:
    adds r0, r0, #48
    bl uart_putc
    lsls r4, r4, #4
    subs r6, r6, #1
    bne 91b
    pop {{r0, r4, r6}}
    cmp r0, #0
    beq 95f
    bl uart_putc
95:
    pop {{pc}}

uart_putc:
    ldr r2, [r5, #0x00]
    lsls r2, r2, #24
    bpl uart_putc
    str r0, [r5, #0x04]
    bx lr
"#
);

/// Enter the factory bootloader (`0x0800_0000`) at boot BEFORE `Watchdog::start()` is called
/// so `IWDG` is completely disabled and never resets the radio during DFU flashing.
#[inline(never)]
pub fn enter_dfu_bootloader(lcd: &mut St7565) -> ! {
    let mut tag = read_backup_tag();
    tag.flags = (tag.flags & !kernel::boot::BackupCrashTag::FLAG_BOOTLOADER_HOLD)
        | kernel::boot::BackupCrashTag::FLAG_CLEAN_REBOOT;
    write_backup_tag(tag);

    bsp::set_status_led(bsp::StatusLed::Off);

    #[cfg(feature = "dfu_hooks")]
    let vid_pid = {
        drivers::backlight::BACKLIGHT_DUTY.store(10, core::sync::atomic::Ordering::Relaxed);
        bsp::pins::BACKLIGHT.set_high();
        lcd.turn_on();

        #[cfg(all(not(test), target_arch = "arm"))]
        let keys_held = {
            let gpiob_idr = (py32_pac::gpio::GPIOB_BASE + 0x10) as *const u32;
            unsafe { (gpiob_idr.read_volatile() & 0xC000) != 0xC000 }
        };
        #[cfg(not(all(not(test), target_arch = "arm")))]
        let keys_held = false;

        let canvas = crate::system_canvas();
        let mut bl_date = [0u8; 8];
        let vid_pid = read_bl_rom_meta(&mut bl_date);
        let bl_ver = bootloader_version_str();
        // Start with neither VID:PID nor bps shown until USB or UART actually connects.
        draw_dfu_screen_inner(canvas, bl_ver, bl_date, 0, keys_held);
        lcd.blit(canvas);

        // If entered via PTT + Side Key 2 chord (or any physical hold on PB14/PB15), wait until both
        // side keys are released so `GPIO_Init` in stock bootloader sees PB14..15 HIGH (`0xC000`).
        #[cfg(all(not(test), target_arch = "arm"))]
        if keys_held {
            let gpiob_idr = (py32_pac::gpio::GPIOB_BASE + 0x10) as *const u32;
            unsafe {
                while (gpiob_idr.read_volatile() & 0xC000) != 0xC000 {
                    py32_hal::delay::delay_ms(10);
                }
                py32_hal::delay::delay_ms(20);
            }
            clear_draw_page5(
                canvas,
                pack_xy_align(64, 49, text::Align::Center),
                banner_plug(),
            );
            lcd.blit(canvas);
        }

        #[cfg(feature = "settings_debug")]
        {
            let _ = py32_hal::uart::write_str("[DFU] BL v");
            let _ = py32_hal::uart::write_str(bl_ver);
            let _ = py32_hal::uart::write_str(" ");
            let mut hbuf = [0u8; 4];
            let _ =
                py32_hal::uart::write_str(text::format_hex16((vid_pid >> 16) as u16, &mut hbuf));
            let _ = py32_hal::uart::write_str(":");
            let _ = py32_hal::uart::write_line(text::format_hex16(vid_pid as u16, &mut hbuf));
            py32_hal::uart::wait_tx_complete();
        }

        vid_pid
    };

    #[cfg(not(feature = "dfu_hooks"))]
    {
        bsp::pins::BACKLIGHT.set_low();
        lcd.turn_off();

        #[cfg(all(not(test), target_arch = "arm"))]
        unsafe {
            let gpiob_idr = (py32_pac::gpio::GPIOB_BASE + 0x10) as *const u32;
            while (gpiob_idr.read_volatile() & 0xC000) != 0xC000 {
                py32_hal::delay::delay_ms(10);
            }
            py32_hal::delay::delay_ms(20);
        }
    }

    #[cfg(all(feature = "dfu_hooks", not(test), target_arch = "arm"))]
    unsafe {
        // Evacuate Kern's MSP stack (`0x2000_3xxx..0x2000_4000`) down to `0x2000_2580` (unused
        // TRANSIENT_ARENA, >7 KB above CANVAS) BEFORE touching `0x2000_3400..0x2000_4000`.
        cortex_m::interrupt::disable();
        core::arch::asm!(
            "msr msp, r1",
            "isb",
            "bx {handoff}",
            handoff = in(reg) handoff_to_factory_bootloader as *const () as usize,
            in("r0") vid_pid,
            in("r1") 0x2000_2580u32,
            options(noreturn)
        );
    }
    #[cfg(all(feature = "dfu_hooks", not(all(not(test), target_arch = "arm"))))]
    unsafe {
        handoff_to_factory_bootloader(vid_pid);
    }
    #[cfg(not(feature = "dfu_hooks"))]
    handoff_to_factory_bootloader();
}

/// Applies `MODER`/`PUPDR` configuration and commits hardware pin lock via `GPIOx->LCKR`.
#[cfg(all(not(test), target_arch = "arm"))]
#[inline(never)]
unsafe fn gpio_apply_and_lock(
    base: u32,
    clear_2b: u32,
    pupdr_set: u32,
    moder_set: u32,
    lock_pins: u32,
) {
    unsafe {
        let port = base as *mut u32;
        let pupdr = port.byte_add(0x0C).read_volatile();
        port.byte_add(0x0C)
            .write_volatile((pupdr & !clear_2b) | pupdr_set);
        let moder = port.read_volatile();
        port.write_volatile((moder & !clear_2b) | moder_set);
        let lckr = port.byte_add(0x1C);
        let lckk = (1u32 << 16) | lock_pins;
        lckr.write_volatile(lckk);
        lckr.write_volatile(lock_pins);
        lckr.write_volatile(lckk);
        let _ = lckr.read_volatile();
    }
}

/// Locks keypad/flashlight pins, restores HSI clock, disconnects USB (`SE0`), resets DMA/APB
/// peripherals (keeping `SPI1` only if requested in `apb2_keep`), and clears SysTick/NVIC.
#[cfg(all(not(test), target_arch = "arm"))]
#[inline(never)]
unsafe fn quiesce_hw_for_dfu(apb2_keep: u32) {
    unsafe {
        // Lock PB4..PB6 + PB14..PB15 as Input Pull-Up and PB10 (PTT) Open-Drain LOW so holding
        // MENU never aborts bootloader entry (leaving PB3 Pull-Up unlocked for EXIT hold),
        // and lock PC13 flashlight Input Pull-Down to eliminate LED battery drain.
        let gpiob = py32_pac::gpio::GPIOB_BASE as *mut u32;
        let ptt_pin = 1u32 << 10;
        gpiob.byte_add(0x28).write_volatile(ptt_pin);
        let otyper = gpiob.byte_add(0x04).read_volatile();
        gpiob.byte_add(0x04).write_volatile(otyper | ptt_pin);
        gpio_apply_and_lock(
            py32_pac::gpio::GPIOB_BASE,
            0xF030_3FC0,
            0x5000_1540,
            0b01 << 20,
            0xC470,
        );

        let rcc = py32_pac::rcc::RCC_BASE as *mut u32;
        let iopenr = rcc.byte_add(0x34);
        iopenr.write_volatile(iopenr.read_volatile() | (1 << 2) | 0x1);
        let gpioc = py32_pac::gpio::GPIOC_BASE as *mut u32;
        gpioc.byte_add(0x28).write_volatile(1u32 << 13); // PC13 BRR = 0
        gpio_apply_and_lock(
            py32_pac::gpio::GPIOC_BASE,
            0b11u32 << 26,
            0b10u32 << 26,
            0,
            1u32 << 13,
        );

        // Switch SYSCLK back to direct HSI (SW = 0) and disable PLL directly via RCC registers.
        rcc.byte_add(0x08).write_volatile(0);
        while (rcc.byte_add(0x08).read_volatile() & 0x38) != 0 {}
        let cr = rcc.read_volatile();
        rcc.write_volatile(cr & !py32_pac::rcc::CR_PLLON);

        // Explicitly release internal USB SoftConn pull-up if USBD was clocked.
        if (rcc.byte_add(0x3C).read_volatile() & py32_pac::rcc::APB1ENR_USBD) != 0 {
            py32_pac::usbd::USBD_POWER_U8.write_volatile(0);
        }

        // Reset DMA1, APB1, and APB2 peripherals (clearing USART1->CR1.UE so stock USART_Init
        // does not abort) while preserving SPI1 when an active SRAM display hook requests it.
        rcc.byte_add(0x28).write_volatile(1);
        rcc.byte_add(0x28).write_volatile(0);
        rcc.byte_add(0x2C).write_volatile(0xFFFF_FFFF);
        rcc.byte_add(0x2C).write_volatile(0);
        rcc.byte_add(0x30).write_volatile(!apb2_keep);
        rcc.byte_add(0x30).write_volatile(0);
        rcc.byte_add(0x3C).write_volatile(0);
        rcc.byte_add(0x40).write_volatile(apb2_keep);

        // Pulse PA12 (USB D+) LOW for >= 25 ms (SE0 disconnect) so the host drops CDC first.
        let gpioa = py32_pac::gpio::GPIOA_BASE as *mut u32;
        gpioa.byte_add(0x28).write_volatile(1u32 << 12); // BRR = PA12 LOW
        let pa_pupdr = gpioa.byte_add(0x0C).read_volatile();
        gpioa
            .byte_add(0x0C)
            .write_volatile(pa_pupdr & !(0b11u32 << 24));
        let pa_moder = gpioa.read_volatile();
        gpioa.write_volatile((pa_moder & !(0b11u32 << 24)) | (0b01u32 << 24));
        cortex_m::asm::delay(600_000);
        gpioa.write_volatile(pa_moder & !(0b11u32 << 24));

        // Stop SysTick and clear all pending interrupts before handoff.
        let scs = 0xE000_E000usize as *mut u32;
        scs.byte_add(0x010).write_volatile(0);
        scs.byte_add(0xD04).write_volatile(1 << 25);
        scs.byte_add(0x180).write_volatile(0xFFFF_FFFF);
        scs.byte_add(0x280).write_volatile(0xFFFF_FFFF);
    }
}

/// Hand execution directly to the stock factory bootloader (`0x0800_0000`) while preserving
/// GPIO state (`PF8` LCD backlight HIGH, ST7565 GDDRAM intact, `PB10` PTT locked LOW, and
/// `PC13` flashlight locked Input Pull-Down), resetting conflicting peripherals, and attaching
/// SRAM `VTOR`, dynamic USB descriptors, and SysTick hooks when matched to RVCT layout.
/// Entered on already-evacuated `MSP = 0x2000_2580` so writing `0x2000_3000..0x2000_4000`
/// never corrupts the live stack.
#[cfg(feature = "dfu_hooks")]
#[inline(never)]
unsafe extern "C" fn handoff_to_factory_bootloader(vid_pid: u32) -> ! {
    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        unsafe extern "C" {
            static __dfu_hook_start: u8;
            static __dfu_hook_end: u8;
        }

        let is_stock_k1 = vid_pid == 0x36B7_0001;
        let bl_rom = core::slice::from_raw_parts(0x0800_0000 as *const u8, 0x2800);
        let mut tramp = [0u32; 3];
        let hook_start = core::ptr::addr_of!(__dfu_hook_start);
        let hook_len = (core::ptr::addr_of!(__dfu_hook_end) as usize) - (hook_start as usize);
        let use_hook = resolve_stock_bl_trampoline(bl_rom, &mut tramp) && hook_len <= 0x900;

        if use_hook && is_stock_k1 {
            let canvas = crate::system_canvas();
            let buf_ptr = canvas.buffer.as_ptr() as *const u8;
            let mut p = DFU_STATUS_BANNERS_PACKED.as_ptr();
            for idx in 0..8u8 {
                let len = *p as usize;
                p = p.add(1);
                let banner = core::str::from_utf8_unchecked(core::slice::from_raw_parts(p, len));
                p = p.add(len);
                let xy = if idx == 4 {
                    pack_xy_align(DFU_FLASH_PREFIX_X, 49, text::Align::Left)
                } else {
                    pack_xy_align(64, 49, text::Align::Center)
                };
                clear_draw_page5(canvas, xy, banner);
                let dst = (DFU_SRAM_BANNERS + ((idx as u32) * 168)) as *mut u8;
                copy_bytes(dst, buf_ptr.add((5 << 7) + 22), 84);
                copy_bytes(dst.add(84), buf_ptr.add((6 << 7) + 22), 84);
            }

            // Snapshot exact 19-column arrow bitmaps (`X=45..=63`) on Pages 2 & 3 for offline blinking,
            // and render `<VID>:<PID>` offscreen into Page 1 (`X=73..=112`) for USB-connect display.
            for p in 0..2usize {
                copy_bytes(
                    (DFU_SRAM_ARROW as *mut u8).add(p * 19),
                    buf_ptr.add(((2 + p) << 7) + 45),
                    19,
                );
            }
            draw_vid_pid(canvas, vid_pid);
            copy_bytes(DFU_SRAM_VIDPID as *mut u8, buf_ptr.add((1 << 7) + 73), 40);
        }

        quiesce_hw_for_dfu(if use_hook {
            py32_pac::rcc::APB2ENR_SPI1
        } else {
            0
        });

        let scs = 0xE000_E000usize as *mut u32;
        let (final_msp, final_entry) = if use_hook {
            // Install SRAM vector table (`0x2000_3600`), SysTick ISR (`0x2000_3700`), and
            // initial DfuHookState (`0x2000_36C0..0x2000_36F3`) above bootloader stack/scatterload RAM.
            let dst_vec = DFU_SRAM_VTOR as *mut u32;
            copy_bytes(dst_vec as *mut u8, 0x0800_0000usize as *const u8, 192);
            dst_vec.add(15).write_volatile(DFU_SRAM_CODE | 1);

            let sp = DFU_SRAM_STATE as *mut u32;
            sp.add(0).write_volatile(u32::from(is_stock_k1) | 0x10);
            sp.add(1).write_volatile(0);
            sp.add(2).write_volatile(0);
            sp.add(3).write_volatile(u32::MAX);
            sp.add(4).write_volatile(0x0004_0000);
            sp.add(5).write_volatile(u32::MAX);
            sp.add(6).write_volatile(0);
            sp.add(7).write_volatile(u32::MAX >> 8);
            sp.add(8).write_volatile(0x0000_0800); // baseline DMA1_CNDTR2 = 2048
            sp.add(9).write_volatile(u32::MAX);
            sp.add(10).write_volatile(0); // last_top_key = 0 (blank)
            sp.add(11).write_volatile(0);
            sp.add(12).write_volatile(2048); // [r4, #48] last_cndtr = 2048

            copy_bytes(DFU_SRAM_CODE as *mut u8, hook_start, hook_len);

            // Run stock SystemInit FIRST (sets clocks and overwrites VTOR = 0x0800_0000),
            // then switch VTOR to SRAM, arm 30 Hz SysTick, and enter stock __main.
            let sys_init_fn: extern "C" fn() = core::mem::transmute(tramp[1] as usize);
            sys_init_fn();

            scs.byte_add(0xD08).write_volatile(DFU_SRAM_VTOR);
            scs.byte_add(0x014).write_volatile(1_599_999);
            scs.byte_add(0x018).write_volatile(0);
            scs.byte_add(0x010).write_volatile(0b111);
            (tramp[0], tramp[2])
        } else {
            // Unrecognized bootloader fallback: restore raw VTOR and jump unmodified.
            scs.byte_add(0xD08).write_volatile(0x0800_0000);
            (
                (0x0800_0000 as *const u32).read_volatile(),
                (0x0800_0004 as *const u32).read_volatile(),
            )
        };

        core::arch::asm!(
            "dsb",
            "isb",
            "msr msp, {msp}",
            "cpsie i",
            "bx {entry}",
            msp = in(reg) final_msp,
            entry = in(reg) final_entry,
            options(noreturn)
        );
    }
    #[cfg(not(all(not(test), target_arch = "arm")))]
    {
        let _ = vid_pid;
        loop {
            core::hint::spin_loop();
        }
    }
}

/// Minimal raw DFU handoff without SRAM hooks (`feature = "dfu"` with `dfu_hooks = false`).
#[cfg(not(feature = "dfu_hooks"))]
#[inline(never)]
fn handoff_to_factory_bootloader() -> ! {
    #[cfg(all(not(test), target_arch = "arm"))]
    unsafe {
        cortex_m::interrupt::disable();
        quiesce_hw_for_dfu(0);

        let scs = 0xE000_E000usize as *mut u32;
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
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_draw_dfu_screen_renders_layout() {
        let mut canvas = Canvas::new();
        draw_dfu_screen(&mut canvas, "7.02.02", 0x36B7, 0x0001);
        // Raised radio antenna (`X=16..19, Y=4..8`) on left; `"v7.02.02 (2025-08-08)"` on top-right (`Y=1..5`)
        assert_ne!(canvas.buffer[0][17] & 0xF0, 0);
        assert!(canvas.buffer[0][22..44].iter().all(|&b| (b & 0x3E) == 0));
        assert!(canvas.buffer[0][47..125].iter().any(|&b| (b & 0x3E) != 0));
        // Radio chassis (`Y=9..15`) + `"DFU"` inside LCD cutout (`Y=13..17`), `"36B7:0001"` at `Y=9..13` over USB rect (`X=81..104`)
        assert_ne!(canvas.buffer[1][14], 0);
        assert!((19..=30).any(|c| (canvas.buffer[1][c] & 0xE0) != 0));
        assert!((19..=30).any(|c| (canvas.buffer[2][c] & 0x03) != 0));
        assert!(canvas.buffer[1][45..74].iter().all(|&b| b == 0));
        assert!(canvas.buffer[1][76..109].iter().any(|&b| (b & 0x3E) != 0));
        // Omitting VID:PID (`vid=0, pid=0`) leaves the area above the USB plug (`Y=9..13`) blank
        let mut no_usb = Canvas::new();
        draw_dfu_screen_inner(&mut no_usb, "7.02.02", [0u8; 8], 0, false);
        assert!(no_usb.buffer[1][74..118].iter().all(|&b| b == 0));
        // Radio lower body (`Y=18..31`), 19-col arrow (`X=45..=63, Y=18..30`), USB-C plug/cable (`Y=17..31`)
        assert_ne!(canvas.buffer[2][14], 0);
        assert_ne!(canvas.buffer[2][50], 0);
        assert_ne!(canvas.buffer[2][63], 0);
        assert_ne!(canvas.buffer[2][75], 0);
        assert_ne!(canvas.buffer[2][90], 0);
        assert_ne!(canvas.buffer[2][120], 0);
        assert_ne!(canvas.buffer[3][50], 0);
        assert_ne!(canvas.buffer[3][90], 0);
        // 4 px blank (`Y=32..35`) above Page 4+5 6 px capsule (`Y=36..41`: P4 `0xE0`/`0x10`/`0xE0`, P5 `0x01`/`0x02`/`0x01`)
        assert!(canvas.buffer[4].iter().all(|&b| (b & 0x0F) == 0));
        assert_eq!(canvas.buffer[4][12], 0xE0);
        assert_eq!(canvas.buffer[4][50], 0x10);
        assert_eq!(canvas.buffer[4][115], 0xE0);
        assert_eq!(canvas.buffer[5][12] & 0x03, 0x01);
        assert_eq!(canvas.buffer[5][50] & 0x03, 0x02);
        assert_eq!(canvas.buffer[5][115] & 0x03, 0x01);
        // 3 px blank (`Y=42..44`) above live status (`y=49`, `Y=45..49`, descenders `Y=50`), spanning Page 5 & Page 6
        assert!(canvas.buffer[5].iter().all(|&b| (b & 0x1C) == 0));
        assert!(canvas.buffer[5].iter().any(|&b| (b & 0xE0) != 0));
        assert!(canvas.buffer[6][22..106].iter().any(|&b| (b & 0x07) != 0));
        // 2 px blank (`Y=51..52`) above footer "Press [EXIT] to reboot" (`Y=53..59`) + 4 px bottom margin (`Y=60..63`)
        let f = &fonts::tomthumb::FONT;
        assert_eq!(text::str_width(DFU_EXIT_PRE, f), 24);
        assert_eq!(text::str_width(DFU_EXIT_KEY, f) + 4, 20);
        assert_eq!(text::str_width(DFU_EXIT_POST, f), 36);
        assert!(canvas.buffer[6].iter().all(|&b| (b & 0x18) == 0));
        assert!(canvas.buffer[7].iter().all(|&b| (b & 0xF0) == 0));
        assert!(canvas.buffer[6][40..70].iter().any(|&b| (b & 0xE0) == 0xE0));
        assert!(canvas.buffer[7][40..70].iter().any(|&b| (b & 0x0F) == 0x0F));
    }

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_parse_bl_rom_meta_extracts_vid_pid_and_date() {
        let mut rom = [0u8; 96];
        // USB Device Descriptor at 0x00 (`36B7:0001`, iSerial=3)
        rom[0..18].copy_from_slice(&[
            0x12, 0x01, 0x00, 0x02, 0, 0, 0, 0x40, 0xB7, 0x36, 0x01, 0x00, 0, 0, 1, 2, 3, 1,
        ]);
        // LangID (04 03 09 04) + Str1 + Str2 + Str3 ("2025080801")
        rom[18..22].copy_from_slice(&[0x04, 0x03, 0x09, 0x04]);
        rom[22..26].copy_from_slice(&[0x04, 0x03, b'Q', 0]);
        rom[26..30].copy_from_slice(&[0x04, 0x03, b'K', 0]);
        rom[30] = 22;
        rom[31] = 0x03;
        for (idx, &b) in b"2025080801".iter().enumerate() {
            rom[32 + idx * 2] = b;
        }
        let mut date = [0u8; 8];
        let vp = parse_bl_rom_meta(&rom, &mut date);
        assert_eq!(vp, 0x36B7_0001);
        assert_eq!(&date, b"20250808");

        // Malformed UTF-16LE date in slot 0 must cleanly zero date_out without integer underflow
        rom[32] = b'X';
        assert_eq!(parse_bl_rom_meta(&rom, &mut date), 0x36B7_0001);
        assert_eq!(date, [0u8; 8]);
    }

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_tomthumb_3x5_cols_matches_font_page5_and_lcd_cutout_shift() {
        let f = &fonts::tomthumb::FONT;
        let mut c = Canvas::new();
        for (idx, ch) in "0123456789/%bps".chars().enumerate() {
            c.buffer[5].fill(0);
            c.buffer[6].fill(0);
            let mut s = [0u8; 1];
            let s = ch.encode_utf8(&mut s);
            text::draw_str(&mut c, 0, 46, s, f, Color::Fill);
            assert_eq!(
                &c.buffer[5][..3],
                &TOMTHUMB_3X5_COLS[idx * 3..idx * 3 + 3],
                "glyph mismatch for '{ch}'"
            );
            // Shifting y=46 bytes (`bits 2..6`) -> y=49 (P5 << 3, P6 >> 5), y=17 cutout (P1 << 3, P2 >> 5),
            // and y=13 Page 1 baud overlay (`raw >> 1`, zero bleed into Page 2).
            c.buffer[5].fill(0);
            c.buffer[6].fill(0);
            text::draw_str(&mut c, 0, 49, s, f, Color::Fill);
            let mut c2 = Canvas::new();
            text::draw_str(&mut c2, 0, 17, s, f, Color::Fill);
            let mut c3 = Canvas::new();
            text::draw_str(&mut c3, 0, 13, s, f, Color::Fill);
            for col in 0..3 {
                let raw = TOMTHUMB_3X5_COLS[idx * 3 + col];
                assert_eq!(c.buffer[5][col], raw << 3);
                assert_eq!(c.buffer[6][col], raw >> 5);
                assert_eq!(c2.buffer[1][col], raw << 3);
                assert_eq!(c2.buffer[2][col], raw >> 5);
                assert_eq!(c3.buffer[1][col], raw >> 1);
                assert_eq!(c3.buffer[2][col], 0);
            }
        }
        // Verify centered UART baud strings fit cleanly within 40-col P1 strip (`X=73..=112`, center `93`)
        for label in ["9600bps", "38400bps", "115200bps"] {
            let mut c = Canvas::new();
            text::draw_str_align(&mut c, 93, 13, label, f, Color::Fill, text::Align::Center);
            assert!(c.buffer[1][..73].iter().all(|&v| v == 0));
            assert!(c.buffer[1][113..].iter().all(|&v| v == 0));
            assert!(c.buffer[2].iter().all(|&v| v == 0));
        }
    }

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_flashing_prefix_fits_before_dynamic_digits_col() {
        let mut canvas = Canvas::new();
        for b in 0..8u8 {
            canvas.buffer[5].fill(0);
            canvas.buffer[6].fill(0);
            let (xy_x, align, s) = if b == 4 {
                (
                    DFU_FLASH_PREFIX_X as i16,
                    text::Align::Left,
                    status_banner(b),
                )
            } else {
                (64, text::Align::Center, status_banner(b))
            };
            text::draw_str_align(
                &mut canvas,
                xy_x,
                49,
                s,
                &fonts::tomthumb::FONT,
                Color::Fill,
                align,
            );
            // All banners & Page 6 pixels must stay strictly within cols 22..105 (`DFU_SRAM_BANNERS`)
            assert!(canvas.buffer[5][..22].iter().all(|&v| v == 0));
            assert!(canvas.buffer[6][..22].iter().all(|&v| v == 0));
            assert!(canvas.buffer[6][106..].iter().all(|&v| v == 0));
            assert!(canvas.buffer[5].iter().all(|&v| (v & 0x1C) == 0));
            assert!(canvas.buffer[6].iter().all(|&v| (v & 0xF8) == 0));
        }
        // Banner 4 prefix ("Flashing page ") must leave cols 76..127 blank for dynamic digits
        canvas.buffer[5].fill(0);
        canvas.buffer[6].fill(0);
        text::draw_str(
            &mut canvas,
            DFU_FLASH_PREFIX_X as i16,
            49,
            status_banner(4),
            &fonts::tomthumb::FONT,
            Color::Fill,
        );
        assert!(
            canvas.buffer[5][..DFU_FLASH_DIGITS_COL]
                .iter()
                .any(|&b| b != 0)
        );
        assert!(
            canvas.buffer[5][DFU_FLASH_DIGITS_COL..]
                .iter()
                .all(|&b| b == 0)
        );
        assert!(
            canvas.buffer[6][DFU_FLASH_DIGITS_COL..]
                .iter()
                .all(|&b| b == 0)
        );
        assert!(TOMTHUMB_3X5_COLS[..36].iter().all(|&b| (b & 0x83) == 0));
        assert!(TOMTHUMB_3X5_COLS[36..].iter().all(|&b| (b & 0x03) == 0));

        // Verify simulated max page stamp ("240/240") fits strictly within Page 6 84-col slice (<= 105)
        let mut strip = canvas.buffer[5];
        let mut pos = DFU_FLASH_DIGITS_COL;
        stamp_dec_cols(240, &mut strip, &mut pos);
        strip[pos..pos + 3].copy_from_slice(&TOMTHUMB_3X5_COLS[30..33]); // '/'
        pos += 4;
        stamp_dec_cols(240, &mut strip, &mut pos);
        assert!(pos <= 104);
    }

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_resolve_stock_bl_trampoline_accepts_stock_k1_sequence() {
        let mut img = [0u8; 0x100];
        let write32 = |buf: &mut [u8], off: usize, v: u32| {
            buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
        };
        let write16 = |buf: &mut [u8], off: usize, v: u16| {
            buf[off..off + 2].copy_from_slice(&v.to_le_bytes());
        };
        write32(&mut img, 0x00, 0x2000_2580);
        write32(&mut img, 0x04, 0x0800_00D5);
        write16(&mut img, 0xD4, 0x4804);
        write16(&mut img, 0xD6, 0x4780);
        write16(&mut img, 0xD8, 0x4804);
        write16(&mut img, 0xDA, 0x4700);
        write32(&mut img, 0xE8, 0x0800_0F85);
        write32(&mut img, 0xEC, 0x0800_00C1);

        let mut out = [0u32; 3];
        assert!(resolve_stock_bl_trampoline(&img, &mut out));
        assert_eq!(out, [0x2000_2580, 0x0800_0F85, 0x0800_00C1]);
    }

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_resolve_stock_bl_trampoline_rejects_unrecognized_and_unsafe_msp() {
        let mut img = [0u8; 0x100];
        let mut out = [0u32; 3];
        img[0x00..0x04].copy_from_slice(&0x2000_2580u32.to_le_bytes());
        img[0x04..0x08].copy_from_slice(&0x0800_0081u32.to_le_bytes());
        img[0x80..0x82].copy_from_slice(&0x46C0u16.to_le_bytes());
        assert!(!resolve_stock_bl_trampoline(&img, &mut out));

        img[0x00..0x04].copy_from_slice(&0x2000_2F00u32.to_le_bytes());
        img[0x04..0x08].copy_from_slice(&0x0800_00D5u32.to_le_bytes());
        img[0xD4..0xD6].copy_from_slice(&0x4804u16.to_le_bytes());
        img[0xD6..0xD8].copy_from_slice(&0x4780u16.to_le_bytes());
        img[0xD8..0xDA].copy_from_slice(&0x4804u16.to_le_bytes());
        img[0xDA..0xDC].copy_from_slice(&0x4700u16.to_le_bytes());
        img[0xE8..0xEC].copy_from_slice(&0x0800_0F85u32.to_le_bytes());
        img[0xEC..0xF0].copy_from_slice(&0x0800_00C1u32.to_le_bytes());
        assert!(!resolve_stock_bl_trampoline(&img, &mut out));

        img[0x00..0x04].copy_from_slice(&0x2000_2E00u32.to_le_bytes());
        assert!(resolve_stock_bl_trampoline(&img, &mut out));
    }

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_uart_brr_to_baud_snap() {
        for &(brr, expected) in &[
            (5000u32, 9600u32),
            (2500, 19200),
            (1250, 38400),
            (833, 57600),
            (834, 57600),
            (417, 115200),
            (416, 115200),
            (208, 230400),
        ] {
            let raw = 48_000_000 / brr;
            let snapped = ((raw + 600) / 1200) * 1200;
            assert_eq!(snapped, expected);
        }
    }

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_draw_vid_pid_matches_top_strip_and_blank_when_disconnected() {
        let mut with_vid = Canvas::new();
        draw_dfu_screen_inner(&mut with_vid, "7.02.02", [0u8; 8], 0x36B7_0001, false);
        let mut isolated = Canvas::new();
        draw_vid_pid(&mut isolated, 0x36B7_0001);
        assert_eq!(with_vid.buffer[1][73..113], isolated.buffer[1][73..113]);
        assert!(isolated.buffer[1][73..113].iter().any(|&b| b != 0));
        assert!(isolated.buffer[2][73..113].iter().all(|&b| b == 0));

        // Disconnected boot screen (`vid_pid = 0`) leaves Page 1 [73..113] blank
        let mut idle = Canvas::new();
        draw_dfu_screen_inner(&mut idle, "7.02.02", [0u8; 8], 0, false);
        assert!(idle.buffer[1][73..113].iter().all(|&b| b == 0));
    }

    #[cfg(feature = "dfu_hooks")]
    #[test]
    fn test_uart_detection_and_top_label_state_machine() {
        // Pure model of DFU SysTick ISR blocks `4b` & `6c-top`:
        struct Sim {
            uart_timer: u8,
            prev_cndtr: u16,
        }
        impl Sim {
            fn step(
                &mut self,
                usb_active: bool,
                usart1_en: bool,
                ue: bool,
                cur_cndtr: u16,
                bl_state: u8,
                brr: u16,
            ) -> u32 {
                if usb_active {
                    self.uart_timer = 0;
                } else {
                    if self.uart_timer > 0 {
                        self.uart_timer -= 1;
                    }
                    if usart1_en && ue {
                        let rx_new = (self.prev_cndtr.wrapping_sub(cur_cndtr)) & 0x7FF;
                        if rx_new >= 2 {
                            self.prev_cndtr = cur_cndtr;
                            self.uart_timer = 150;
                        } else if bl_state == 2 {
                            self.uart_timer = 150;
                        }
                    }
                }
                if usb_active {
                    1
                } else if self.uart_timer > 0 && usart1_en && ue && brr >= 49 {
                    let raw = 48_000_000 / u32::from(brr);
                    let baud = ((raw + 600) / 1200) * 1200;
                    if baud >= 1200 { baud } else { 0 }
                } else {
                    0
                }
            }
        }

        let mut sim = Sim {
            uart_timer: 0,
            prev_cndtr: 2048,
        };
        // Pre-USART1 init & USB disconnected: blank (`0`)
        assert_eq!(sim.step(false, false, false, 0, 0, 0), 0);
        // Bootloader initializes USART1 @ 115200 (BRR=417) and DMA CNDTR=2048: still blank
        assert_eq!(sim.step(false, true, true, 2048, 0, 417), 0);
        // USB connect switches to VID:PID (`1`); USB disconnect returns to blank (`0`), never false UART
        assert_eq!(sim.step(true, true, true, 2048, 0, 417), 1);
        assert_eq!(sim.step(false, true, true, 2048, 0, 417), 0);
        // Single floating-pin glitch byte (rx_new == 1) is ignored
        assert_eq!(sim.step(false, true, true, 2047, 0, 417), 0);
        // Valid packet >=2 DMA bytes latches 115200bps (`115200`) and updates baseline CNDTR
        assert_eq!(sim.step(false, true, true, 2040, 0, 417), 115200);
        assert_eq!(sim.prev_cndtr, 2040);
        // USB plug-in immediately clears UART timer and prioritizes VID:PID
        assert_eq!(sim.step(true, true, true, 2040, 0, 417), 1);
        assert_eq!(sim.uart_timer, 0);
        // Re-trigger UART and verify 5 s (150 tick) idle decay returns to blank (`0`)
        assert_eq!(sim.step(false, true, true, 2036, 0, 417), 115200);
        for _ in 0..149 {
            assert_eq!(sim.step(false, true, true, 2036, 0, 417), 115200);
        }
        assert_eq!(sim.step(false, true, true, 2036, 0, 417), 0);
    }

    #[test]
    fn test_is_ptt_side2_chord_requires_both_keys() {
        assert!(!is_ptt_side2_chord(0));
        assert!(!is_ptt_side2_chord(1 << 18));
        assert!(!is_ptt_side2_chord(1 << 1));
        assert!(is_ptt_side2_chord((1 << 18) | (1 << 1)));
        assert!(is_ptt_side2_chord((1 << 18) | (1 << 1) | 0x20));
    }
}
