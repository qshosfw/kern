//! Lean ABI-Native UI Wrappers for External Settings.
//!
//! Delegates capsule drawing, icons, checkboxes, oscillating chevrons, OptionSheets,
//! and scrollbars directly to OS syscalls with zero large stack buffers.

use kern_sdk::{
    COLOR_CLEAR, KRX_STATUS_CUSTOM_TITLE, KRX_STATUS_DIVIDER, KRX_STATUS_VISIBLE,
    KRX_WIDGET_MENU_ITEM, KrxApiTable, KrxStatusBar, KrxWidget, Op,
};

pub const VISIBLE_ROWS: u8 = 4;
pub const ROW_START_Y: i16 = 9;
pub const ROW_STEP_Y: i16 = 13;

/// `KRX_WIDGET_MENU_ITEM` flag bits matching `syscall/ui.rs`.
pub const FLAG_CHEVRON: u8 = 1 << 1;
pub const FLAG_OUTLINE: u8 = 1 << 2;
pub const FLAG_TOGGLE: u8 = 1 << 3;
pub const FLAG_TOGGLE_ON: u8 = 1 << 4;
pub const FLAG_ARROW_LEFT: u8 = 1 << 5;
pub const FLAG_ARROW_RIGHT: u8 = 1 << 6;

/// Stateful windowed kinetic scroller matching resident `gfx::gui::modules::menu::MenuView`.
/// Keeps `top_row` stable while the cursor moves inside the visible 4-row window.
#[derive(Clone, Copy)]
pub struct KineticScroll {
    pub cur_y: i16,
    pub target_y: i16,
    pub top_row: u8,
}

impl KineticScroll {
    pub const fn new() -> Self {
        Self { cur_y: 0, target_y: 0, top_row: 0 }
    }

    #[inline(never)]
    pub fn adjust(&mut self, sel: u8, total: u8, animate: bool) {
        let mut top = self.top_row;
        if sel < top {
            top = sel;
        } else if sel >= top + VISIBLE_ROWS {
            top = sel - (VISIBLE_ROWS - 1);
        }
        let max_top = total.saturating_sub(VISIBLE_ROWS);
        self.top_row = top.min(max_top);
        let y = (self.top_row as i16) * ROW_STEP_Y;
        self.target_y = y;
        if !animate {
            self.cur_y = y;
        }
    }

    #[inline(always)]
    pub fn jump(&mut self, sel: u8, total: u8, saved_top: u8) {
        self.top_row = saved_top;
        self.adjust(sel, total, false);
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

/// Set the OS status bar title using caller-supplied SRAM scratch (`>= 64 B`, 0 bytes of stack).
#[inline(never)]
pub fn set_title(api: &KrxApiTable, scratch: &mut [u8; 128], title: &[u8]) {
    let ptr = scratch.as_mut_ptr() as *mut u32;
    unsafe {
        let mut w = 0usize;
        while w < 16 {
            core::ptr::write_volatile(ptr.add(w), 0);
            w += 1;
        }
        let bptr = ptr as *mut u8;
        *bptr = KRX_STATUS_VISIBLE | KRX_STATUS_DIVIDER | KRX_STATUS_CUSTOM_TITLE;
        let n = title.len().min(16);
        let mut i = 0usize;
        while i < n {
            *bptr.add(12 + i) = *title.get_unchecked(i);
            i += 1;
        }
        api.set_status_bar(&*(bptr as *const KrxStatusBar));
    }
}

/// Draw a single scrolled row directly via `KRX_WIDGET_MENU_ITEM` ABI widget.
/// `icon_flags` packs `icon_id` in low byte and `flags` in high byte to minimize AAPCS spills.
#[inline(never)]
pub fn draw_row(
    api: &KrxApiTable,
    y: i16,
    label: &[u8],
    value: &[u8],
    icon_flags: u16,
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
        a: (icon_flags >> 8) as u8,
        b: icon_flags as u8,
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
