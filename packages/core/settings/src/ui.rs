//! Lean, Native-Matching UI Primitives for External Settings.
//!
//! Uses OS-native `draw_menu_item`, `draw_scrollbar`, and `set_status_bar` syscalls
//! without pulling in `__aeabi_memclr`.

use kern_sdk::{
    KRX_STATUS_CUSTOM_TITLE, KRX_STATUS_DIVIDER, KRX_STATUS_VISIBLE, KrxApiTable, KrxStatusBar,
    format_u16_into,
};

pub const VISIBLE_ROWS: u8 = 4;
pub const ROW_START_Y: i16 = 9;
pub const ROW_STEP_Y: i16 = 13;

#[inline(always)]
pub const fn scroll_top(sel: u8) -> u8 {
    if sel >= VISIBLE_ROWS {
        sel - (VISIBLE_ROWS - 1)
    } else {
        0
    }
}

/// Set the OS status bar title without calling `memset`/`__aeabi_memclr`.
#[inline(never)]
pub fn set_title(api: &KrxApiTable, title: &[u8]) {
    let mut words = [0u32; 16];
    let ptr = words.as_mut_ptr() as *mut u8;
    unsafe {
        *ptr = KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER | KRX_STATUS_CUSTOM_TITLE;
        // `title` field starts at byte offset 12 inside `KrxStatusBar`.
        let n = title.len().min(16);
        let mut i = 0usize;
        while i < n {
            *ptr.add(12 + i) = *title.get_unchecked(i);
            i += 1;
        }
        api.set_status_bar(&*(ptr as *const KrxStatusBar));
    }
}

/// Draw a single Settings menu row with optional icon, right-aligned value, and capsule.
#[inline(never)]
pub fn draw_row(
    api: &KrxApiTable,
    vis_idx: u8,
    label: &[u8],
    value: &[u8],
    icon_id: u8,
    selected: bool,
    editing: bool,
) {
    let y = ROW_START_Y + (vis_idx as i16) * ROW_STEP_Y;
    let flags = u8::from(selected) | (u8::from(editing) << 1) | 4;
    api.draw_menu_item(y, label, value, icon_id, flags);
}

#[inline(never)]
pub fn draw_list_scrollbar(api: &KrxApiTable, scroll: u8, total: u8) {
    if total > VISIBLE_ROWS {
        api.draw_scrollbar(125, 9, 52, scroll as u16, total as u16, VISIBLE_ROWS as u16);
    }
}

/// Format a byte count cleanly as `"999B"` or `"4.1K"` into `buf`.
#[inline(never)]
pub fn format_bytes<'a>(size: u32, buf: &'a mut [u8; 16]) -> &'a [u8] {
    if size < 1024 {
        format_u16_into(size as u16, 0, b"B", buf)
    } else {
        let kb = (size >> 10) as u16;
        let tenths = (((size & 1023) * 10) >> 10) as u16;
        format_u16_into(kb * 10 + tenths, 1, b"K", buf)
    }
}
