//! Lean, Native-Matching Animated UI Primitives for External Settings.
//!
//! Supports smooth kinetic pixel-scrolling (`KineticScroll`), oscillating selection
//! arrows, native toggle checkboxes, and clean status-bar clipping without `__aeabi_memclr`.

use kern_sdk::{
    COLOR_CLEAR, COLOR_FILL, KRX_STATUS_CUSTOM_TITLE, KRX_STATUS_DIVIDER, KRX_STATUS_VISIBLE,
    KrxApiTable, KrxStatusBar, format_u16_into, icon,
};

pub const VISIBLE_ROWS: u8 = 4;
pub const ROW_START_Y: i16 = 9;
pub const ROW_STEP_Y: i16 = 13;

/// Smooth pixel-level kinetic scroller matching resident `gfx::gui::animation::KineticScroller`.
#[derive(Clone, Copy)]
pub struct KineticScroll {
    pub cur_y: i16,
    pub target_y: i16,
}

impl KineticScroll {
    pub const fn new() -> Self {
        Self { cur_y: 0, target_y: 0 }
    }

    #[inline(always)]
    pub fn jump_to_row(&mut self, top_row: u8) {
        let y = (top_row as i16) * ROW_STEP_Y;
        self.cur_y = y;
        self.target_y = y;
    }

    #[inline(always)]
    pub fn set_target_row(&mut self, top_row: u8, animate: bool) {
        let y = (top_row as i16) * ROW_STEP_Y;
        self.target_y = y;
        if !animate {
            self.cur_y = y;
        }
    }

    /// Advance one animation frame; returns `true` while still moving.
    #[inline(always)]
    pub fn tick(&mut self) -> bool {
        let diff = self.target_y - self.cur_y;
        if diff == 0 {
            return false;
        }
        let step = if diff > 0 { (diff + 1) >> 1 } else { (diff - 1) >> 1 };
        self.cur_y += step;
        true
    }
}

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
        let n = title.len().min(16);
        let mut i = 0usize;
        while i < n {
            *ptr.add(12 + i) = *title.get_unchecked(i);
            i += 1;
        }
        api.set_status_bar(&*(ptr as *const KrxStatusBar));
    }
}

/// Draw a single scrolled row at pixel `y` with optional icon, value, oscillating arrow, or checkbox.
#[inline(never)]
pub fn draw_scrolled_row(
    api: &KrxApiTable,
    y: i16,
    label: &[u8],
    value: &[u8],
    icon_id: u8,
    selected: bool,
    editing: bool,
    right_icon: u8,
    tick: u8,
) {
    if y <= -ROW_STEP_Y || y >= 64 {
        return;
    }
    let flags = u8::from(selected) | (u8::from(editing) << 1) | 4;
    api.draw_menu_item(y, label, value, icon_id, flags);
    if right_icon != 0 {
        let fg = if selected && !editing { COLOR_CLEAR } else { COLOR_FILL };
        let mut rx: i16 = 112;
        if selected && right_icon == icon::ARROW_RIGHT {
            let phase = ((tick >> 1) & 3) as i16;
            rx += if phase == 3 { 1 } else { phase };
        }
        api.draw_icon(rx, y + 2, right_icon, fg);
    }
}

/// Clean any scrolled row bleed into the top status bar band and draw smooth scrollbar.
#[inline(never)]
pub fn finish_list_frame(api: &KrxApiTable, scroll_px: i16, total: u8) {
    api.fill_rect(0, 0, 128, 8, COLOR_CLEAR);
    if total > VISIBLE_ROWS {
        let pos = scroll_px.max(0) as u16;
        let tot = (total as u16) * (ROW_STEP_Y as u16);
        let vis = (VISIBLE_ROWS as u16) * (ROW_STEP_Y as u16);
        api.draw_scrollbar(125, 9, 52, pos, tot, vis);
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
