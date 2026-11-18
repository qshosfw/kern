//! System Domain Settings & Submenu Declarations

#[cfg(any(
    feature = "filesystem",
    feature = "settings_calib",
    feature = "settings_debug"
))]
use super::page::MetricId;
use super::page::{ActionId, ChoiceId, PageId, SettingDef, SettingField};
#[cfg(feature = "settings_info")]
use crate::SysContext;
#[cfg(feature = "settings_info")]
use gfx::fonts::tomthumb::FONT as TOMTHUMB;
#[cfg(feature = "settings_info")]
use gfx::text::{Align, draw_str_align};
#[cfg(feature = "settings_info")]
use gfx::{Canvas, Color};

#[cfg(feature = "overclock_72mhz")]
pub static CPU_CLOCK_OPTIONS: &str = "16 MHz\024 MHz\032 MHz\048 MHz\072 MHz\0";
#[cfg(not(feature = "overclock_72mhz"))]
pub static CPU_CLOCK_OPTIONS: &str = "16 MHz\024 MHz\032 MHz\048 MHz\0";

#[cfg(feature = "uart")]
pub static UART_BAUD_OPTIONS: &str = "115200\038400\0";

/// Index in `PageId::System` where declarative `FLAG_SYSTEM_MENU` modules are inserted.
#[cfg(feature = "krx_native")]
#[inline(always)]
pub(crate) const fn system_modules_insert_idx() -> usize {
    1 + if cfg!(feature = "uart") { 1 } else { 0 }
        + if cfg!(feature = "settings_info") {
            1
        } else {
            0
        }
}

/// Index in `PageId::System` where declarative `FLAG_SYSTEM_ACTION` buttons are inserted (before Restart).
#[cfg(feature = "krx_native")]
#[inline(always)]
pub(crate) const fn system_actions_insert_idx() -> usize {
    SYSTEM_ROOT.len().saturating_sub(1)
}

pub static SYSTEM_ROOT: &[SettingDef] = &[
    // Hardware & Bus Tuning
    SettingDef::choice("Clock", ChoiceId::CpuClock, SettingField::CPU_CLOCK),
    #[cfg(feature = "uart")]
    SettingDef::choice("UART Baud", ChoiceId::UartBaud, SettingField::UART_BAUD),
    // Identity, Storage & Diagnostics
    #[cfg(feature = "settings_info")]
    SettingDef::submenu("Info", PageId::SystemInfo),
    #[cfg(feature = "filesystem")]
    SettingDef::submenu("Storage", PageId::SystemStorage),
    #[cfg(feature = "settings_calib")]
    SettingDef::submenu("Calibration", PageId::SystemCalibration),
    #[cfg(feature = "settings_debug")]
    SettingDef::submenu("Debug", PageId::SystemDebug),
    // Maintenance & Power Control
    SettingDef::action("Reset Settings", ActionId::ResetSettings),
    SettingDef::action("Restart", ActionId::RebootFirmware),
];

#[cfg(feature = "settings_info")]
pub const SYSTEM_INFO_VISIBLE_ROWS: usize = 7;

#[cfg(feature = "settings_info")]
const BASE_INFO_ROWS: usize = 18;

#[cfg(feature = "settings_info")]
const FW_BADGE_STR: &str = concat!("v", env!("CARGO_PKG_VERSION"), " ", env!("KERN_GIT_HASH"));

#[cfg(feature = "settings_info")]
pub const FCC_NOTICE: &str = "\
    This device complies with\0\
    Part 15 of the FCC Rules.\0\
    Operation is subject to two\0\
    conditions: (1) must not\0\
    cause harmful interference,\0\
    (2) must accept any received.";

/// NUL-delimited static headings, codes, field keys, and FCC notice (indices 0..=17).
#[cfg(feature = "settings_info")]
const INFO_STRINGS: &[u8] = b"\
    SYSTEM\0\
    HARDWARE ID\0\
    REGULATORY\0\
    47 CFR PART 15B / 97\0\
    CE  RED 2014/53/EU\0\
    MCU\0\
    BOOT\0\
    BUILD\0\
    UPTIME\0\
    S/N\0\
    MAC\0\
    QS-1\0\
    This device complies with\0\
    Part 15 of the FCC Rules.\0\
    Operation is subject to two\0\
    conditions: (1) must not\0\
    cause harmful interference,\0\
    (2) must accept any received.\0";

/// 15x11 LSB-top column-major interlaced FCC logo monogram.
#[cfg(feature = "settings_info")]
const FCC_MARK: [u16; 15] = [
    0x7FF, 0x7FF, 0x01B, 0x01B, 0x003, 0x000, 0x1FC, 0x3FE, 0x707, 0x603, 0x6FB, 0x7FF, 0x78F,
    0x78F, 0x306,
];

/// 14x10 LSB-top column-major European CE conformity mark.
#[cfg(feature = "settings_info")]
const CE_MARK: [u16; 14] = [
    0x0FC, 0x1FE, 0x303, 0x303, 0x303, 0x303, 0x000, 0x000, 0x1FE, 0x3FF, 0x333, 0x333, 0x333,
    0x303,
];

#[cfg(feature = "settings_info")]
#[inline(always)]
const fn pack_xywh(x: u8, y: u8, w: u8, h: u8) -> u32 {
    (x as u32) | ((y as u32) << 8) | ((w as u32) << 16) | ((h as u32) << 24)
}

#[cfg(feature = "settings_info")]
#[inline(never)]
fn rect(canvas: &mut Canvas, xywh: u32) {
    canvas.fill_rect(
        xywh as u8,
        (xywh >> 8) as u8,
        (xywh >> 16) as u8,
        (xywh >> 24) as u8,
        Color::Fill,
    );
}

#[cfg(feature = "settings_info")]
#[inline(always)]
const fn pack_xyc(x: u8, y: u8, cfg: u8) -> u32 {
    (x as u32) | ((y as u32) << 8) | ((cfg as u32) << 16)
}

/// Single shared TomThumb draw primitive (all arguments fit in `r0..r3` with zero stack spills).
#[cfg(feature = "settings_info")]
#[inline(never)]
fn tt(canvas: &mut Canvas, xyc: u32, s: &str) {
    let color = if ((xyc >> 16) & 1) != 0 {
        Color::Clear
    } else {
        Color::Fill
    };
    let align = match (xyc >> 17) & 3 {
        1 => Align::Center,
        2 => Align::Right,
        _ => Align::Left,
    };
    draw_str_align(
        canvas,
        xyc as u8 as i16,
        (xyc >> 8) as u8 as i16,
        s,
        &TOMTHUMB,
        color,
        align,
    );
}

/// Returns the `idx`-th NUL-delimited slice from `INFO_STRINGS` without bounds panics.
#[cfg(feature = "settings_info")]
#[inline(never)]
fn info_str(idx: usize) -> &'static str {
    let mut cur = 0usize;
    let mut i = 0usize;
    let len = INFO_STRINGS.len();
    while i < len {
        let start = i;
        while i < len && INFO_STRINGS[i] != 0 {
            i += 1;
        }
        if cur == idx {
            return unsafe { core::str::from_utf8_unchecked(INFO_STRINGS.get_unchecked(start..i)) };
        }
        cur += 1;
        i += 1;
    }
    ""
}

/// Draws a 11-px column at `x` clipped cleanly to ST7565 Pages `1..=7` (`Y=8..63`).
#[cfg(feature = "settings_info")]
#[inline(always)]
fn blit_col11(canvas: &mut Canvas, x: usize, col: u16, by: isize, p0: usize, sh: u32) {
    let mut v = if by < 8 {
        (col as u32) >> sh
    } else {
        (col as u32) << sh
    };
    let mut p = p0;
    while v != 0 && p < 8 {
        canvas.buffer[p][x] |= v as u8;
        v >>= 8;
        p += 1;
    }
}

/// Draws FCC (15x11), CE (14x10), and vertical dividers clipped cleanly to `Y=8..63`.
#[cfg(feature = "settings_info")]
#[inline(never)]
fn draw_reg_marks(canvas: &mut Canvas, by: isize) {
    let (p0, sh) = if by < 8 {
        (1usize, (8 - by) as u32)
    } else {
        ((by as usize) >> 3, (by & 7) as u32)
    };
    blit_col11(canvas, 19, 0x7FF, by, p0, sh);
    blit_col11(canvas, 105, 0x7FF, by, p0, sh);
    for (i, &c) in FCC_MARK.iter().enumerate() {
        blit_col11(canvas, 2 + i, c, by, p0, sh);
    }
    for (i, &c) in CE_MARK.iter().enumerate() {
        blit_col11(canvas, 108 + i, c, by, p0, sh);
    }
}

/// Returns total scrollable rows on the System Info screen (`+1` when factory QS-1 data is present).
#[cfg(feature = "settings_info")]
#[inline(never)]
pub fn system_info_row_count() -> usize {
    if matches!(
        bsp::uid::factory_cert_status(),
        bsp::uid::FactoryCertStatus::Qs1Ascii { .. }
    ) {
        BASE_INFO_ROWS + 1
    } else {
        BASE_INFO_ROWS
    }
}

/// Renders the scrollable TomThumb System Info & Regulatory page (`Y=8..63`).
#[cfg(feature = "settings_info")]
#[inline(always)]
pub fn render_system_info_page(canvas: &mut Canvas, ctx: &SysContext, scroll_row: usize) {
    render_system_info_with(canvas, ctx, scroll_row, bsp::uid::factory_cert_status());
}

/// Inner renderer parameterized on `FactoryCertStatus` for deterministic unit testing.
#[cfg(feature = "settings_info")]
#[inline(never)]
pub fn render_system_info_with(
    canvas: &mut Canvas,
    ctx: &SysContext,
    scroll_row: usize,
    qs1: bsp::uid::FactoryCertStatus,
) {
    canvas.clear();

    let has_qs1 = matches!(qs1, bsp::uid::FactoryCertStatus::Qs1Ascii { .. });
    let total_rows = BASE_INFO_ROWS + usize::from(has_qs1);
    let max_scroll = total_rows - SYSTEM_INFO_VISIBLE_ROWS;
    let scroll = scroll_row.min(max_scroll);

    let badge_top = 14 + ((10 + usize::from(has_qs1)) as isize - scroll as isize) * 8 - 4;
    if badge_top < 64 && badge_top + 11 > 8 {
        draw_reg_marks(canvas, badge_top);
    }

    let mut fmt_buf = [0u8; 20];
    let mut sn: [u8; 15];
    let mut vis = 0usize;
    while vis < SYSTEM_INFO_VISIBLE_ROWS {
        let row = scroll + vis;
        if row >= total_rows {
            break;
        }
        let y = 14 + (vis as u8) * 8;
        let mut left_x = 2u8;
        let mut left = "";
        let mut right = "";
        let mut cfg = 0u8;

        if has_qs1 && row == 9 {
            left = info_str(11);
            right = qs1.as_str();
        } else {
            let norm = if has_qs1 && row > 9 { row - 1 } else { row };
            match norm {
                0 => {
                    rect(canvas, pack_xywh(2, 8, 21, 7));
                    tt(canvas, pack_xyc(4, 13, 1), "KERN");
                    tt(canvas, pack_xyc(25, 13, 0), FW_BADGE_STR);
                    tt(canvas, pack_xyc(122, 13, 4), ctx.config.system.model_str());
                    rect(canvas, pack_xywh(2, 15, 121, 1));
                }
                1 | 6 | 9 => {
                    rect(canvas, pack_xywh(2, y - 5, 2, 5));
                    rect(canvas, pack_xywh(2, y + 1, 121, 1));
                    left_x = 6;
                    left = info_str(if norm == 1 {
                        0
                    } else if norm == 6 {
                        1
                    } else {
                        2
                    });
                }
                10 | 11 => {
                    left_x = 62;
                    cfg = 2;
                    left = info_str(norm - 7);
                }
                12..=17 => {
                    rect(canvas, pack_xywh(2, y - 5, 1, 6));
                    left_x = 4;
                    left = info_str(norm);
                }
                _ => {
                    let k = match norm {
                        2 => {
                            fmt_buf[..9].copy_from_slice(b"PY32F071 ");
                            let kb_len = utils::format::format_u32(
                                u32::from(py32_hal::uid::SystemInfo::flash_size_kb()),
                                unsafe { &mut *(fmt_buf.as_mut_ptr().add(9) as *mut [u8; 8]) },
                            )
                            .len();
                            fmt_buf[9 + kb_len] = b'K';
                            right = unsafe {
                                core::str::from_utf8_unchecked(fmt_buf.get_unchecked(..10 + kb_len))
                            };
                            5
                        }
                        3 => {
                            right = bsp::uid::bootloader_version_str();
                            6
                        }
                        4 => {
                            right = env!("KERN_BUILD_DATE");
                            7
                        }
                        5 => {
                            let up_buf = unsafe { &mut *(fmt_buf.as_mut_ptr() as *mut [u8; 8]) };
                            right = utils::format::format_uptime_hms(ctx.power.uptime_secs, up_buf);
                            8
                        }
                        7 => {
                            sn = bsp::uid::get_crockford_serial();
                            right = unsafe { core::str::from_utf8_unchecked(&sn[..14]) };
                            9
                        }
                        _ => {
                            let mac = bsp::uid::read_mac_address();
                            let mac_buf = unsafe { &mut *(fmt_buf.as_mut_ptr() as *mut [u8; 17]) };
                            right = utils::format::format_mac_address(&mac, mac_buf);
                            10
                        }
                    };
                    left = info_str(k);
                }
            }
        }
        if !left.is_empty() {
            tt(canvas, pack_xyc(left_x, y, cfg), left);
        }
        if !right.is_empty() {
            tt(canvas, pack_xyc(122, y, 4), right);
        }
        vis += 1;
    }

    gfx::widgets::draw_scrollbar(
        canvas,
        125,
        9,
        53,
        scroll,
        total_rows,
        SYSTEM_INFO_VISIBLE_ROWS,
    );
}

#[cfg(feature = "filesystem")]
pub static SYSTEM_STORAGE: &[SettingDef] = &[
    SettingDef::metric("Used", MetricId::StorageUsed),
    SettingDef::metric("Free", MetricId::StorageFree),
    SettingDef::metric("Files", MetricId::StorageFiles),
    SettingDef::metric("Wear Cycles", MetricId::StorageWear),
    SettingDef::metric("Deleted", MetricId::StorageDeleted),
    SettingDef::action("Format Storage", ActionId::FormatKeepCalib),
    SettingDef::action("Erase All Data", ActionId::FormatWipeAll),
];

#[cfg(feature = "settings_calib")]
pub static SYSTEM_CALIBRATION: [SettingDef; 6] = [
    SettingDef::metric("Calib Sig", MetricId::CalibSig),
    SettingDef::metric("XTAL Trim", MetricId::CalibXtal),
    SettingDef::metric("Batt Ref", MetricId::CalibBatt),
    SettingDef::metric("LNA Gain", MetricId::CalibLna),
    SettingDef::metric("Mixer Gain", MetricId::CalibMix),
    SettingDef::metric("Volume Gain", MetricId::CalibVol),
];

#[cfg(feature = "settings_debug")]
pub static SYSTEM_DEBUG: [SettingDef; 8] = [
    SettingDef::metric("Stack Margin", MetricId::DebugStackMargin),
    SettingDef::metric("Peak Stack", MetricId::DebugStackPeak),
    SettingDef::metric("Crashes", MetricId::DebugCrashes),
    SettingDef::metric("SWD Port", MetricId::DebugSwd),
    SettingDef::metric("MCU RDP", MetricId::DebugRdp),
    SettingDef::metric("Dev Key", MetricId::DebugDevKey),
    SettingDef::metric("MK Hash", MetricId::DebugMkHash),
    SettingDef::action("Clear Logs", ActionId::ClearLogs),
];
