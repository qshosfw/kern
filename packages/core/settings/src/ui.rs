//! Lean ABI-Native UI Wrappers for External Settings.
//!
//! Delegates capsule drawing, icons, checkboxes, oscillating chevrons, and scrollbars
//! directly to OS `KRX_WIDGET_MENU_ITEM` / `KRX_WIDGET_SCROLLBAR` syscalls — zero
//! double-implemented UI rendering.

use kern_sdk::{
    COLOR_CLEAR, KRX_STATUS_CUSTOM_TITLE, KRX_STATUS_DIVIDER, KRX_STATUS_VISIBLE,
    KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxStatusBar, KrxWidget, Op, format_u16_into,
};

pub const VISIBLE_ROWS: u8 = 4;
pub const ROW_START_Y: i16 = 9;
pub const ROW_STEP_Y: i16 = 13;

/// `KRX_WIDGET_MENU_ITEM` flag bits matching `syscall/ui.rs`.
pub const FLAG_SEL: u8 = 1 << 0;
pub const FLAG_CHEVRON: u8 = 1 << 1;
pub const FLAG_OUTLINE: u8 = 1 << 2;
pub const FLAG_TOGGLE: u8 = 1 << 3;
pub const FLAG_TOGGLE_ON: u8 = 1 << 4;

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

    #[inline(always)]
    pub fn tick(&mut self) -> bool {
        let diff = self.target_y - self.cur_y;
        if diff == 0 {
            return false;
        }
        self.cur_y += if diff > 0 { (diff + 1) >> 1 } else { (diff - 1) >> 1 };
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

/// Draw a single scrolled row directly via `KRX_WIDGET_MENU_ITEM` ABI widget.
#[inline(never)]
pub fn draw_row(
    api: &KrxApiTable,
    y: i16,
    label: &[u8],
    value: &[u8],
    icon_id: u8,
    flags: u8,
    anim_tick: u16,
) {
    if y <= -ROW_STEP_Y || y >= 64 {
        return;
    }
    let desc = KrxWidget {
        p0: label.as_ptr(),
        p1: value.as_ptr(),
        x: 2,
        y,
        len0: label.len() as u16,
        len1: value.len() as u16,
        extra: anim_tick,
        w: 120,
        h: 13,
        a: flags,
        b: icon_id,
        _pad: [0; 2],
    };
    let _ = api.call_op(
        Op::UiWidget,
        KRX_WIDGET_MENU_ITEM as usize,
        &raw const desc as usize,
        0,
    );
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
