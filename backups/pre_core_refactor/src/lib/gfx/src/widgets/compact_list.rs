//! Compact scrolling list & table widgets for dense UI displays.
//!
//! Provides zero-allocation, fixed-geometry scrolling lists with automatic
//! windowing, selection highlight bars, right-edge scrollbars, and multi-column
//! table headers and row layout helpers.

use super::dialog::draw_scrollbar;
use crate::text;
use crate::{Canvas, Color, GfxFont};

/// Configuration for compact scrolling list viewports.
#[derive(Debug, Clone, Copy)]
pub struct CompactListConfig {
    pub x: u8,
    pub y: u8,
    pub w: u8,
    pub h: u8,
    pub row_height: u8,
    pub show_scrollbar: bool,
    pub rounded_selection: bool,
}

impl Default for CompactListConfig {
    fn default() -> Self {
        Self {
            x: 2,
            y: 22,
            w: 124,
            h: 32,
            row_height: 8,
            show_scrollbar: true,
            rounded_selection: false,
        }
    }
}

impl CompactListConfig {
    /// Number of fully visible rows in the viewport
    #[inline]
    pub const fn visible_rows(&self) -> usize {
        let count = self.h / self.row_height;
        if count == 0 { 1 } else { count as usize }
    }

    /// Automatically adjust scroll_top to ensure selected_idx is in the visible window
    #[inline]
    pub fn ensure_visible(
        &self,
        selected_idx: usize,
        current_scroll_top: usize,
        total_items: usize,
    ) -> usize {
        if total_items == 0 {
            return 0;
        }
        let vis = self.visible_rows();
        let sel = selected_idx.min(total_items - 1);
        if sel < current_scroll_top {
            sel
        } else if sel >= current_scroll_top + vis {
            sel + 1 - vis
        } else {
            current_scroll_top
        }
    }
}

/// Generic compact scrolling list renderer with item callback.
///
/// Handles selection highlighting, row clipping, scroll windowing, and
/// right-edge scrollbar rendering.
pub fn draw_compact_list<F>(
    canvas: &mut Canvas,
    config: &CompactListConfig,
    total_items: usize,
    selected_idx: usize,
    scroll_top: usize,
    mut render_row: F,
) where
    F: FnMut(&mut Canvas, usize, i16, i16, u8, u8, bool),
{
    if total_items == 0 {
        return;
    }

    let vis_rows = config.visible_rows();
    let effective_scroll_top = config.ensure_visible(selected_idx, scroll_top, total_items);
    let row_w = if total_items > vis_rows && config.show_scrollbar {
        config.w.saturating_sub(4)
    } else {
        config.w
    };

    for i in 0..vis_rows {
        let item_idx = effective_scroll_top + i;
        if item_idx >= total_items {
            break;
        }

        let item_y = config.y as i16 + (i as i16 * config.row_height as i16);
        let is_selected = item_idx == selected_idx;

        if is_selected {
            canvas.fill_rect(
                config.x,
                item_y as u8,
                row_w,
                config.row_height,
                Color::Fill,
            );
            if config.rounded_selection {
                canvas.clear_corners(config.x, item_y as u8, row_w, config.row_height);
            }
        }

        render_row(
            canvas,
            item_idx,
            config.x as i16,
            item_y,
            row_w,
            config.row_height,
            is_selected,
        );
    }

    if total_items > vis_rows && config.show_scrollbar {
        let sb_x = config.x + config.w.saturating_sub(3);
        draw_scrollbar(
            canvas,
            sb_x,
            config.y,
            config.h,
            selected_idx,
            total_items,
            vis_rows,
        );
    }
}

/// Column definition for table headers and rows
#[derive(Debug, Clone, Copy)]
pub struct TableColumn<'a> {
    pub title: &'a str,
    pub x: i16,
    pub max_width: Option<i16>,
}

/// Draws a crisp table header with optional bottom separator line
pub fn draw_table_header(
    canvas: &mut Canvas,
    y: i16,
    columns: &[TableColumn<'_>],
    font: &GfxFont,
    draw_divider: bool,
) {
    for col in columns {
        if let Some(max_w) = col.max_width {
            text::draw_str_max_x(canvas, col.x, y, col.title, font, Color::Fill, max_w);
        } else {
            text::draw_str(canvas, col.x, y, col.title, font, Color::Fill);
        }
    }
    if draw_divider {
        canvas.draw_horizontal_line(2, (y + 2) as u8, 124, Color::DarkGray);
    }
}

/// Draws columns of text within a table row
pub fn draw_table_row_columns(
    canvas: &mut Canvas,
    y: i16,
    columns: &[(&str, i16, Option<i16>)],
    font: &GfxFont,
    color: Color,
) {
    for &(txt, x, max_w) in columns {
        if let Some(mw) = max_w {
            text::draw_str_max_x(canvas, x, y, txt, font, color, mw);
        } else {
            text::draw_str(canvas, x, y, txt, font, color);
        }
    }
}

/// Smooth kinetic compact scrolling list renderer with pixel-level vertical offset.
///
/// Handles selection highlighting, row clipping within the viewport `[config.y, config.y + config.h]`,
/// and right-edge scrollbar rendering.
pub fn draw_compact_list_smooth<F>(
    canvas: &mut Canvas,
    config: &CompactListConfig,
    total_items: usize,
    selected_idx: usize,
    pixel_scroll_y: i16,
    mut render_row: F,
) where
    F: FnMut(&mut Canvas, usize, i16, i16, u8, u8, bool),
{
    if total_items == 0 {
        return;
    }

    let vis_rows = config.visible_rows();
    let row_h = config.row_height as i16;
    let row_w = if total_items > vis_rows && config.show_scrollbar {
        config.w.saturating_sub(4)
    } else {
        config.w
    };

    let viewport_top = config.y as i16;
    let viewport_bottom = (config.y + config.h) as i16;

    let start_row = (pixel_scroll_y / row_h).max(0) as usize;
    let end_row = ((pixel_scroll_y + config.h as i16 + row_h - 1) / row_h) as usize;
    let end_row = end_row.min(total_items);

    for item_idx in start_row..end_row {
        let row_top = viewport_top + (item_idx as i16 * row_h) - pixel_scroll_y;
        let row_bottom = row_top + row_h;

        if row_bottom <= viewport_top || row_top >= viewport_bottom {
            continue;
        }

        let is_selected = item_idx == selected_idx;
        if is_selected {
            let sel_y = row_top.max(viewport_top) as u8;
            let sel_h = (row_bottom.min(viewport_bottom) - row_top.max(viewport_top)) as u8;
            if sel_h > 0 {
                canvas.fill_rect(config.x, sel_y, row_w, sel_h, Color::Fill);
                if config.rounded_selection {
                    canvas.clear_corners(config.x, sel_y, row_w, sel_h);
                }
            }
        }

        render_row(
            canvas,
            item_idx,
            config.x as i16,
            row_top,
            row_w,
            config.row_height,
            is_selected,
        );
    }

    if total_items > vis_rows && config.show_scrollbar {
        let track_x = config.x + config.w - 3;
        draw_scrollbar(
            canvas,
            track_x,
            config.y,
            config.h,
            selected_idx,
            total_items,
            vis_rows,
        );
    }
}
