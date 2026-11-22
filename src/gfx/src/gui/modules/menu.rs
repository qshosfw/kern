//! Modular Declarative Menu GUI
//! High-contrast hover/edit interactions, checkboxes, and navigation arrows.
//! 100% Safe Rust with clean separation between mutable event handling and immutable rendering.

use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::animation::{KineticScroller, ListScrollMode, MarqueeScroller};
use crate::gui::view::View;
use crate::icons;
use crate::text;
use crate::{Canvas, Color};
use core::cell::Cell;
use kernel::event::{Event, KeyCode, KeyState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShortString {
    pub bytes: [u8; 18],
    pub len: u8,
}

impl ShortString {
    pub const fn empty() -> Self {
        Self {
            bytes: [0; 18],
            len: 0,
        }
    }

    pub fn set_str(&mut self, s: &str) {
        let copy_len = s.len().min(18);
        self.bytes[..copy_len].copy_from_slice(&s.as_bytes()[..copy_len]);
        self.len = copy_len as u8;
    }

    pub fn from_str(s: &str) -> Self {
        let mut out = Self::empty();
        out.set_str(s);
        out
    }

    pub fn as_str(&self) -> &str {
        unsafe { core::str::from_utf8_unchecked(&self.bytes[..self.len as usize]) }
    }

    pub fn from_u32(val: u32) -> Self {
        let mut s = Self::empty();
        let slice = crate::text::format_u32(val, &mut s.bytes);
        s.len = slice.len() as u8;
        s
    }

    pub fn from_hex32(val: u32) -> Self {
        let mut s = Self {
            bytes: [0; 18],
            len: 8,
        };
        let arr = unsafe { &mut *(s.bytes.as_mut_ptr() as *mut [u8; 8]) };
        let _ = crate::text::format_hex32(val, arr);
        s
    }

    pub fn push_str(&mut self, s: &str) {
        let rem = 18usize.saturating_sub(self.len as usize);
        let n = s.len().min(rem);
        self.bytes[self.len as usize..self.len as usize + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n as u8;
    }

    pub fn push_byte(&mut self, b: u8) {
        if (self.len as usize) < 18 {
            self.bytes[self.len as usize] = b;
            self.len += 1;
        }
    }

    pub fn push_int_fixed(&mut self, val: i32, decimals: u8, prefix: &str, suffix: &str) {
        let rem = 18usize.saturating_sub(self.len as usize);
        if rem > 0 {
            let slice = crate::text::format_int_fixed(
                val,
                decimals,
                prefix,
                suffix,
                &mut self.bytes[self.len as usize..],
            );
            self.len += slice.len() as u8;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SheetOptions {
    Dynamic(&'static [ShortString]),
    Packed(&'static str),
}

impl SheetOptions {
    #[inline(never)]
    pub fn len(&self) -> usize {
        match self {
            SheetOptions::Dynamic(d) => d.len(),
            SheetOptions::Packed(p) => {
                let mut count = 0;
                for &b in p.as_bytes() {
                    if b == 0 {
                        count += 1;
                    }
                }
                count
            }
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline(never)]
    pub fn get(&self, idx: usize) -> &str {
        match self {
            SheetOptions::Dynamic(d) => {
                if idx < d.len() {
                    d[idx].as_str()
                } else {
                    ""
                }
            }
            SheetOptions::Packed(p) => {
                if idx <= u8::MAX as usize {
                    utils::format::nth_cstr(p.as_bytes(), idx as u8)
                } else {
                    ""
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
pub struct OptionSheet {
    pub title: &'static str,
    pub options: SheetOptions,
    pub icons: Option<&'static [&'static [u8; 8]]>,
    pub selected: usize,
    pub hover: usize,
    pub show_radio: bool,
    pub is_pressed: bool,
}

impl OptionSheet {
    pub fn new(title: &'static str, options: SheetOptions, selected: usize) -> Self {
        let hover = if selected < options.len() {
            selected
        } else {
            0
        };
        Self {
            title,
            options,
            icons: None,
            selected,
            hover,
            show_radio: false,
            is_pressed: false,
        }
    }

    pub fn new_dynamic(
        title: &'static str,
        labels: &'static [ShortString],
        selected: usize,
    ) -> Self {
        let hover = if selected < labels.len() { selected } else { 0 };
        Self {
            title,
            options: SheetOptions::Dynamic(labels),
            icons: None,
            selected,
            hover,
            show_radio: false,
            is_pressed: false,
        }
    }

    pub const fn with_radio(mut self, show_radio: bool) -> Self {
        self.show_radio = show_radio;
        self
    }

    pub const fn with_icons(mut self, icons: &'static [&'static [u8; 8]]) -> Self {
        self.icons = Some(icons);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetResult {
    None,
    Changed,
    Confirmed(usize),
    Canceled,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MenuItemType {
    Label,
    Action,
    Toggle(bool),
    Range {
        val: i16,
        min: i16,
        max: i16,
        step: i16,
        unit: &'static str,
    },
    Choice {
        selected: u16,
        options: SheetOptions,
    },
    Submenu,
    Value(ShortString),
    Shortcut(ShortString),
}

#[derive(Debug, Clone, Copy)]
pub struct MenuItem {
    pub title: &'static str,
    pub item_type: MenuItemType,
}

impl MenuItem {
    pub const fn new(title: &'static str, item_type: MenuItemType) -> Self {
        Self { title, item_type }
    }
}

/// Immutable Menu Model for safe UI rendering
pub struct MenuModel<'a> {
    pub title: &'static str,
    pub items: &'a [MenuItem],
    pub selected_idx: usize,
    pub icons: Option<&'a [&'static [u8; 8]]>,

    pub is_editing: bool,
    pub is_pressed: bool,
}

/// Mutable Menu Model for event processing and item state mutation
pub struct MenuModelMut<'a> {
    pub items: &'a mut [MenuItem],
    pub selected_idx: usize,
    /// 0 = K1 (L/R horizontal D-pad), 1 = K5 (U/D vertical D-pad)
    pub nav_layout: u8,

    pub is_editing: bool,
    pub is_pressed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListNavDir {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeDir {
    /// Towards `>` arrow: increments index (+1) or value (+step)
    RightInc,
    /// Towards `<` arrow: decrements index (-1) or value (-step)
    LeftDec,
}

/// Map a physical key to a list scroll direction.
///
#[derive(Clone, Copy, PartialEq, Eq)]
enum KeyNavStep {
    Primary(bool), // true = down/forward, false = up/backward
    Keypad(bool),  // true = right/down, false = left/up
}

#[inline(always)]
fn classify_nav_key(code: KeyCode) -> Option<KeyNavStep> {
    match code {
        KeyCode::Up | KeyCode::Key2 => Some(KeyNavStep::Primary(false)),
        KeyCode::Down | KeyCode::Key8 => Some(KeyNavStep::Primary(true)),
        KeyCode::Key4 => Some(KeyNavStep::Keypad(false)),
        KeyCode::Key6 => Some(KeyNavStep::Keypad(true)),
        _ => None,
    }
}

/// Map a physical key to a vertical list navigation direction.
pub fn get_list_nav_dir(nav_layout: u8, code: KeyCode) -> Option<ListNavDir> {
    match classify_nav_key(code)? {
        KeyNavStep::Primary(false) => Some(ListNavDir::Up),
        KeyNavStep::Primary(true) => Some(ListNavDir::Down),
        KeyNavStep::Keypad(forward) if nav_layout == 0 => Some(if forward {
            ListNavDir::Down
        } else {
            ListNavDir::Up
        }),
        _ => None,
    }
}

/// Map a physical key to an edit change direction (for Range/Choice values).
pub fn get_edit_change_dir(nav_layout: u8, code: KeyCode) -> Option<ChangeDir> {
    match classify_nav_key(code)? {
        KeyNavStep::Keypad(forward) => Some(if forward {
            ChangeDir::RightInc
        } else {
            ChangeDir::LeftDec
        }),
        KeyNavStep::Primary(forward) => {
            let inc = if nav_layout == 0 { forward } else { !forward };
            Some(if inc {
                ChangeDir::RightInc
            } else {
                ChangeDir::LeftDec
            })
        }
    }
}

pub struct MenuView {
    pub repeater: crate::gui::repeater::KeyRepeater,
    pub scroller: KineticScroller,
    pub marquee: MarqueeScroller,
    marquee_overflow: Cell<u8>,
    pub mode: ListScrollMode,
    pub scroll_top: usize,
    pub animated: bool,
}

impl MenuView {
    pub const fn new() -> Self {
        Self {
            repeater: crate::gui::repeater::KeyRepeater::new(8, 2), // 400ms initial hold delay, 100ms repeat rate @ 50ms tick
            scroller: KineticScroller::new(2),
            marquee: MarqueeScroller::idle(),
            marquee_overflow: Cell::new(0),
            mode: ListScrollMode::Windowed,
            scroll_top: 0,
            animated: true,
        }
    }

    pub fn reset_scroll(&mut self) {
        self.scroll_top = 0;
        self.scroller.jump(0);
        self.marquee.set_overflow(0);
        self.marquee_overflow.set(0);
    }

    pub fn adjust_scroll(&mut self, selected_idx: usize, total_items: usize) {
        if total_items == 0 {
            self.scroll_top = 0;
            self.scroller.jump(0);
            return;
        }
        let vis = 4usize;
        match self.mode {
            ListScrollMode::Windowed => {
                if selected_idx < self.scroll_top {
                    self.scroll_top = selected_idx;
                } else if selected_idx >= self.scroll_top + vis {
                    self.scroll_top = selected_idx + 1 - vis;
                }
            }
            ListScrollMode::Paged => {
                self.scroll_top = (selected_idx / vis) * vis;
            }
            ListScrollMode::Thirds => {
                let margin = (vis / 3).max(1);
                if selected_idx < self.scroll_top + margin {
                    self.scroll_top = selected_idx.saturating_sub(margin);
                } else if selected_idx + margin >= self.scroll_top + vis {
                    self.scroll_top = (selected_idx + margin + 1).saturating_sub(vis);
                }
            }
        }
        let max_scroll = total_items.saturating_sub(vis);
        self.scroll_top = self.scroll_top.min(max_scroll);
        let target_y = (self.scroll_top as i16) * 13;
        if self.animated {
            self.scroller.set_target(target_y);
        } else {
            self.scroller.jump(target_y);
        }
    }

    #[inline(never)]
    fn move_selection(
        &mut self,
        model: &mut MenuModelMut<'_>,
        dir: ListNavDir,
        wrap: bool,
        pause_ticks: u16,
    ) -> bool {
        let prev_idx = model.selected_idx;
        let wrapped = apply_list_nav_dir(model, dir, wrap);
        if model.selected_idx != prev_idx {
            self.marquee.set_overflow(0);
            self.marquee_overflow.set(0);
            self.adjust_scroll(model.selected_idx, model.items.len());
            if wrapped {
                self.scroller.jump((self.scroll_top as i16) * 13);
                self.repeater.pause(pause_ticks);
            }
            true
        } else {
            false
        }
    }

    #[inline(never)]
    pub fn on_event(&mut self, model: &mut MenuModelMut<'_>, event: &Event) -> bool {
        if model.items.is_empty() {
            return false;
        }

        if let Event::Encoder(delta) = event {
            if *delta == 0 {
                return false;
            }
            let steps = delta.unsigned_abs().min(8);
            if model.is_editing {
                let dir = if *delta > 0 {
                    ChangeDir::RightInc
                } else {
                    ChangeDir::LeftDec
                };
                for _ in 0..steps {
                    apply_change_dir(&mut model.items[model.selected_idx], dir);
                }
                return true;
            } else {
                let dir = if *delta > 0 {
                    ListNavDir::Down
                } else {
                    ListNavDir::Up
                };
                for _ in 0..steps {
                    self.move_selection(model, dir, true, 5);
                }
                return true;
            }
        }

        if let Event::Tick = event {
            let mut changed = self.scroller.tick();

            if self.animated {
                self.marquee.set_overflow(self.marquee_overflow.get());
                if self.marquee.tick() {
                    changed = true;
                }
            } else if self.marquee.is_active() {
                self.marquee.set_overflow(0);
            }

            // Drive navigation / edit auto-repeat from tick
            if let Some(repeat_code) = self.repeater.tick() {
                if model.is_editing {
                    if let Some(dir) = get_edit_change_dir(model.nav_layout, repeat_code) {
                        apply_change_dir(&mut model.items[model.selected_idx], dir);
                        changed = true;
                    }
                } else if let Some(dir) = get_list_nav_dir(model.nav_layout, repeat_code) {
                    // On small lists (all items visible on screen), clamp to boundaries during repeat hold
                    if self.move_selection(model, dir, model.items.len() > 4, 6) {
                        changed = true;
                    }
                }
            }

            return changed;
        }

        if let Event::Key { code, state } = event {
            // Update repeater tracking for this key
            self.repeater.handle_key(*code, *state);

            let is_released = *state == KeyState::Released;
            let is_pressed = *state == KeyState::Pressed;

            let item = &mut model.items[model.selected_idx];
            let is_action = matches!(
                item.item_type,
                MenuItemType::Action | MenuItemType::Submenu | MenuItemType::Value(_)
            );

            if is_released {
                if model.is_pressed {
                    model.is_pressed = false;
                    if is_action && (*code == KeyCode::Menu || *code == KeyCode::Key5) {
                        return true;
                    }
                }
                // Always consume the release of navigation keys so they never leak into global shortcuts
                if classify_nav_key(*code).is_some() {
                    return true;
                }
                return false;
            }

            if model.is_editing {
                if is_pressed {
                    if let Some(dir) = get_edit_change_dir(model.nav_layout, *code) {
                        apply_change_dir(item, dir);
                        return true;
                    } else if matches!(code, KeyCode::Menu | KeyCode::Exit | KeyCode::Key5) {
                        self.repeater.reset();
                        model.is_editing = false;
                        return true;
                    } else {
                        return true;
                    }
                }
                return true;
            }

            if is_pressed {
                if let Some(dir) = get_list_nav_dir(model.nav_layout, *code) {
                    self.move_selection(model, dir, true, 5);
                    return true;
                }
                // Non-nav key handling (Menu, Exit, etc.)
                match code {
                    KeyCode::Menu | KeyCode::Key5 => {
                        if matches!(item.item_type, MenuItemType::Range { .. }) {
                            self.repeater.reset();
                            model.is_editing = true;
                            return true;
                        } else if let MenuItemType::Toggle(ref mut on) = item.item_type {
                            *on = !*on;
                            return true;
                        } else {
                            model.is_pressed = true;
                            return true;
                        }
                    }
                    KeyCode::Exit => {
                        self.repeater.reset();
                        model.is_pressed = false;
                        return false;
                    }
                    _ => {}
                }
            }

            if *state == KeyState::Held && matches!(code, KeyCode::Menu | KeyCode::Key5) {
                return true;
            }
        }
        false
    }
}

impl Default for MenuView {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_change_dir(item: &mut MenuItem, dir: ChangeDir) {
    match dir {
        ChangeDir::RightInc => {
            if let MenuItemType::Range { val, max, step, .. } = &mut item.item_type
                && *val < *max
            {
                *val = (*val + *step).min(*max);
            }
        }
        ChangeDir::LeftDec => {
            if let MenuItemType::Range { val, min, step, .. } = &mut item.item_type
                && *val > *min
            {
                *val = (*val - *step).max(*min);
            }
        }
    }
}

/// Returns true if navigation wrapped around boundaries (from 0 -> len-1 or len-1 -> 0).
fn apply_list_nav_dir(model: &mut MenuModelMut<'_>, dir: ListNavDir, wrap: bool) -> bool {
    let len = model.items.len();
    if len > 0 {
        model.is_pressed = false;
        match dir {
            ListNavDir::Up => {
                if model.selected_idx > 0 {
                    model.selected_idx -= 1;
                } else if wrap && len > 1 {
                    model.selected_idx = len - 1;
                    return true;
                }
            }
            ListNavDir::Down => {
                if model.selected_idx + 1 < len {
                    model.selected_idx += 1;
                } else if wrap && len > 1 {
                    model.selected_idx = 0;
                    return true;
                }
            }
        }
    }
    false
}

#[inline(never)]
fn render_menu_row(
    canvas: &mut Canvas,
    item: &MenuItem,
    flags: u8,
    row_y: i16,
    icon: Option<&[u8; 8]>,
    marquee_x: i16,
    marquee_enabled: bool,
) -> u8 {
    use crate::widgets::*;
    let is_selected = (flags & 1) != 0;
    let is_editing = (flags & 2) != 0;
    let is_pressed = (flags & 4) != 0;
    let mut row_flags = 0u8;
    if is_selected {
        row_flags |= ROW_SELECTED;
        if !(is_editing || is_pressed) {
            row_flags |= ROW_OUTLINE;
        }
    }

    let mut buf_uninit = core::mem::MaybeUninit::<[u8; 16]>::uninit();
    let mut title = item.title;
    let mut value = "";
    let mut localize_value = false;

    match &item.item_type {
        MenuItemType::Shortcut(action_label) => {
            row_flags |= ROW_SHORTCUT;
            value = action_label.as_str();
        }
        MenuItemType::Value(v) => {
            value = v.as_str();
            localize_value = true;
        }
        MenuItemType::Choice { selected, options } => {
            let sel = *selected as usize;
            value = options.get(sel);
            localize_value = true;
            if sel > 0 {
                row_flags |= ROW_ARROW_LEFT;
            }
            if sel + 1 < options.len() {
                row_flags |= ROW_ARROW_RIGHT;
            }
        }
        MenuItemType::Range {
            val,
            min,
            max,
            unit,
            ..
        } => {
            if *val > *min {
                row_flags |= ROW_ARROW_LEFT;
            }
            if *val < *max {
                row_flags |= ROW_ARROW_RIGHT;
            }
            let buf = unsafe { &mut *buf_uninit.as_mut_ptr() };
            value = text::format_range_val(*val as i32, *min as i32, unit, buf);
        }
        MenuItemType::Toggle(on) => {
            row_flags |= ROW_TOGGLE;
            if *on {
                row_flags |= ROW_TOGGLE_ON;
            }
        }
        MenuItemType::Submenu => {
            row_flags |= ROW_CHEVRON;
        }
        _ => {}
    }

    let mut spec = ListRowSpec::new(0, row_y, 124, 13, title);
    spec.flags = row_flags;
    spec.value = value;
    spec.icon = icon;
    spec.marquee_x = marquee_x;
    spec.marquee_enabled = marquee_enabled;
    spec.localize_value = localize_value;
    spec.clip_top = 8;
    if is_selected && is_editing {
        // Edit mode draws outline capsule + directional arrows around value.
        spec.flags |= ROW_OUTLINE;
    }
    let _ = &mut title;
    draw_list_row(canvas, &spec)
}

impl<'a> View<MenuModel<'a>> for MenuView {
    #[inline(never)]
    fn render(&self, canvas: &mut Canvas, model: &MenuModel<'a>) {
        canvas.clear();

        let total_items = model.items.len();
        if total_items == 0 {
            text::draw_str_align(
                canvas,
                64,
                32,
                "(empty)",
                &TOMTHUMB,
                Color::Fill,
                text::Align::Center,
            );
            return;
        }

        // 2. Exact 13px Row List Menu with Smooth Kinetic Windowing
        let visible_count = 4;
        let scroll_y = self.scroller.current();
        let sy = scroll_y.max(0) as u32;
        let start_idx = ((sy * 5042) >> 16) as usize;
        let end_idx = ((((sy + 64) * 5042) >> 16) as usize).min(total_items);

        for idx in start_idx..end_idx {
            let row_y = 8 + (idx as i16 * 13) - scroll_y;
            if row_y + 13 <= 8 || row_y >= 64 {
                continue;
            }

            let mut flags = 0u8;
            if idx == model.selected_idx {
                flags |= 1;
                if model.is_editing {
                    flags |= 2;
                }
                if model.is_pressed {
                    flags |= 4;
                }
            }
            let icon = model.icons.and_then(|list| list.get(idx).copied());
            let ov = render_menu_row(
                canvas,
                &model.items[idx],
                flags,
                row_y,
                icon,
                if idx == model.selected_idx {
                    self.marquee.offset()
                } else {
                    0
                },
                self.animated,
            );
            if idx == model.selected_idx {
                self.marquee_overflow.set(ov);
            }
        }

        // Clean any row bleed into status bar area (0..8)
        canvas.fill_rect(0, 0, 128, 8, Color::Clear);

        // 3. Scrollbar
        if total_items > visible_count {
            crate::widgets::draw_scrollbar(
                canvas,
                126,
                8,
                56,
                scroll_y.max(0) as usize,
                total_items * 13,
                visible_count * 13,
            );
        }
    }

    fn on_event(&mut self, _model: &mut MenuModel<'a>, _event: &Event) -> bool {
        false
    }
}

impl MenuView {
    #[inline(always)]
    pub fn handle_sheet_event(&mut self, sheet: &mut OptionSheet, event: &Event) -> SheetResult {
        handle_sheet_event_with_repeater(&mut self.repeater, sheet, event)
    }

    pub fn render_sheet(canvas: &mut Canvas, sheet: &OptionSheet) {
        Self::render_sheet_slide_up(canvas, sheet, 0);
    }

    #[inline(never)]
    fn render_sheet_row(
        canvas: &mut Canvas,
        sheet: &OptionSheet,
        idx: usize,
        row_y_scroll: i16,
        marquee_x: i16,
        marquee_enabled: bool,
    ) -> u8 {
        use crate::widgets::{ListRowSpec, ROW_SELECTED, draw_list_row};
        let has_scroll = row_y_scroll < 0;
        let row_y = if has_scroll {
            !row_y_scroll
        } else {
            row_y_scroll
        };
        let is_hovered = idx == sheet.hover;
        let is_selected = idx == sheet.selected;
        let (fill_x, fill_w) = if has_scroll { (2, 118) } else { (3, 122) };
        let row_icon = sheet.icons.and_then(|arr| arr.get(idx).copied());
        let prefix_icon = if sheet.show_radio {
            Some(if is_selected {
                icons::RADIO_ON
            } else {
                icons::RADIO_OFF
            })
        } else if is_hovered && row_icon.is_none() {
            Some(icons::ARROW_RIGHT)
        } else {
            None
        };
        let mut spec = ListRowSpec::new(fill_x, row_y, fill_w, 11, sheet.options.get(idx));
        if is_hovered {
            spec.flags = ROW_SELECTED;
        }
        spec.prefix_icon = prefix_icon;
        spec.icon = row_icon;
        spec.marquee_x = marquee_x;
        spec.marquee_enabled = marquee_enabled;
        draw_list_row(canvas, &spec)
    }

    #[inline(always)]
    pub fn render_sheet_slide_up(canvas: &mut Canvas, sheet: &OptionSheet, y_offset: i8) {
        let _ = Self::render_sheet_slide_up_marquee(canvas, sheet, y_offset, 0, false);
    }

    /// Render a slide-up option sheet with reading-speed marquee on the hovered row.
    /// Returns the hovered row's horizontal overflow in pixels (`0` when it fits).
    #[inline(never)]
    pub fn render_sheet_slide_up_marquee(
        canvas: &mut Canvas,
        sheet: &OptionSheet,
        y_offset: i8,
        marquee_x: i16,
        marquee_enabled: bool,
    ) -> u8 {
        let total = sheet.options.len();
        if total == 0 {
            return 0;
        }

        let sheet_h = 48u8;
        let sheet_y = crate::widgets::draw_sheet_slide_up(canvas, sheet.title, sheet_h, y_offset);
        if sheet_y >= 64 {
            return 0;
        }

        let visible_count = 3usize;
        let (page_u16, _) = utils::math::div3_u16(sheet.hover as u16);
        let start_idx = page_u16 as usize * visible_count;
        let end_idx = (start_idx + visible_count).min(total);
        let has_scroll = total > visible_count;
        let mut hover_overflow = 0u8;

        let mut row_y = sheet_y + 11;
        for idx in start_idx..end_idx {
            if row_y < 64 && row_y + 11 > sheet_y + 10 {
                let packed = if has_scroll { !row_y } else { row_y };
                let ov = Self::render_sheet_row(
                    canvas,
                    sheet,
                    idx,
                    packed,
                    if idx == sheet.hover { marquee_x } else { 0 },
                    marquee_enabled,
                );
                if idx == sheet.hover {
                    hover_overflow = ov;
                }
            }
            row_y += 12;
        }

        if has_scroll && sheet_y + 10 < 60 {
            let track_y = (sheet_y + 10) as u8;
            let track_h = (64 - sheet_y - 12).clamp(8, 36) as u8;
            crate::widgets::draw_scrollbar(
                canvas,
                124,
                track_y,
                track_h,
                start_idx,
                total,
                visible_count,
            );
        }
        hover_overflow
    }
}

#[inline(always)]
pub fn sheet_nav_dir(code: KeyCode) -> Option<bool> {
    match code {
        KeyCode::Key6 => Some(true),
        KeyCode::Key4 => Some(false),
        _ => match classify_nav_key(code)? {
            KeyNavStep::Primary(fwd) | KeyNavStep::Keypad(fwd) => Some(fwd),
        },
    }
}

#[inline(always)]
fn step_hover(hover: usize, total: usize, forward: bool) -> usize {
    if forward {
        if hover + 1 < total { hover + 1 } else { 0 }
    } else if hover > 0 {
        hover - 1
    } else if total > 0 {
        total - 1
    } else {
        0
    }
}

pub fn handle_sheet_event_with_repeater(
    repeater: &mut crate::gui::repeater::KeyRepeater,
    sheet: &mut OptionSheet,
    event: &Event,
) -> SheetResult {
    let total = sheet.options.len();
    if total == 0 {
        return SheetResult::None;
    }

    if let Event::Tick = event {
        if let Some(code) = repeater.tick()
            && let Some(down) = sheet_nav_dir(code)
        {
            let prev = sheet.hover;
            if down {
                if prev + 1 < total {
                    sheet.hover = prev + 1;
                } else if total > 4 {
                    sheet.hover = 0;
                    repeater.pause(6);
                }
            } else if prev > 0 {
                sheet.hover = prev - 1;
            } else if total > 4 {
                sheet.hover = total - 1;
                repeater.pause(6);
            }
            if sheet.hover != prev {
                return SheetResult::Changed;
            }
        }
        return SheetResult::None;
    }

    match event {
        Event::Key { code, state } => {
            repeater.handle_key(*code, *state);
            match state {
                KeyState::Pressed => {
                    if let Some(down) = sheet_nav_dir(*code) {
                        sheet.hover = step_hover(sheet.hover, total, down);
                        return SheetResult::Changed;
                    }
                    match code {
                        KeyCode::Menu | KeyCode::Key5 => {
                            sheet.is_pressed = true;
                            return SheetResult::Changed;
                        }
                        KeyCode::Exit | KeyCode::Key0 => {
                            return SheetResult::Canceled;
                        }
                        _ => {}
                    }
                }
                KeyState::Released
                    if sheet.is_pressed && matches!(code, KeyCode::Menu | KeyCode::Key5) =>
                {
                    sheet.is_pressed = false;
                    sheet.selected = sheet.hover;
                    return SheetResult::Confirmed(sheet.hover);
                }
                _ => {}
            }
        }
        Event::Encoder(delta) if *delta != 0 => {
            let forward = *delta > 0;
            for _ in 0..delta.unsigned_abs().min(8) {
                sheet.hover = step_hover(sheet.hover, total, forward);
            }
            return SheetResult::Changed;
        }
        _ => {}
    }
    SheetResult::None
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_ITEMS_8: [MenuItem; 8] = [
        MenuItem::new("Item 0", MenuItemType::Label),
        MenuItem::new("Item 1", MenuItemType::Label),
        MenuItem::new("Item 2", MenuItemType::Label),
        MenuItem::new("Item 3", MenuItemType::Label),
        MenuItem::new("Item 4", MenuItemType::Label),
        MenuItem::new("Item 5", MenuItemType::Label),
        MenuItem::new("Item 6", MenuItemType::Label),
        MenuItem::new("Item 7", MenuItemType::Label),
    ];

    #[test]
    fn test_menu_view_initial_state() {
        let view = MenuView::new();
        assert_eq!(view.mode, ListScrollMode::Windowed);
        assert_eq!(view.scroll_top, 0);
        assert_eq!(view.scroller.current(), 0);
        assert_eq!(view.scroller.target(), 0);
        assert!(!view.scroller.is_animating());
    }

    #[test]
    fn test_menu_view_windowed_navigation_and_animation() {
        let mut view = MenuView::new();
        let mut items = TEST_ITEMS_8;
        let mut model = MenuModelMut {
            items: &mut items,
            selected_idx: 0,
            nav_layout: 1, // K5 U/D keys
            is_editing: false,
            is_pressed: false,
        };

        // Moving down within first 4 visible slots (0 -> 1 -> 2 -> 3)
        for expected_idx in 1..=3 {
            let res = view.on_event(
                &mut model,
                &Event::Key {
                    code: KeyCode::Down,
                    state: KeyState::Pressed,
                },
            );
            assert!(res);
            assert_eq!(model.selected_idx, expected_idx);
            assert_eq!(
                view.scroll_top, 0,
                "scroll_top should remain 0 within first 4 slots"
            );
            assert_eq!(
                view.scroller.target(),
                0,
                "No animation within visible window"
            );
            assert!(!view.scroller.is_animating());
        }

        // Now at slot 3 (index 3). Pushing down should move to index 4 and scroll by 1 row!
        let res = view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert!(res);
        assert_eq!(model.selected_idx, 4);
        assert_eq!(view.scroll_top, 1, "scroll_top should edge-push by 1 row");
        assert_eq!(
            view.scroller.target(),
            13,
            "Scroller target should be 1 row = 13px"
        );
        assert!(
            view.scroller.is_animating(),
            "Should begin kinetic animation"
        );

        // Tick scroller to completion
        while view.on_event(&mut model, &Event::Tick) {}
        assert_eq!(view.scroller.current(), 13);
        assert!(!view.scroller.is_animating());

        // Moving back up from 4 to 3: should stay at scroll_top 1 (cursor moves to slot 2)
        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Up,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 3);
        assert_eq!(view.scroll_top, 1);
        assert_eq!(view.scroller.target(), 13);

        // Moving up to 0 pushes top edge and animates back to 0
        while model.selected_idx > 0 {
            view.on_event(
                &mut model,
                &Event::Key {
                    code: KeyCode::Up,
                    state: KeyState::Pressed,
                },
            );
        }
        assert_eq!(view.scroll_top, 0);
        assert_eq!(view.scroller.target(), 0);
    }

    #[test]
    fn test_menu_view_wrap_around_jumps_scroller() {
        let mut view = MenuView::new();
        let mut items = TEST_ITEMS_8;
        let mut model = MenuModelMut {
            items: &mut items,
            selected_idx: 0,
            nav_layout: 1,
            is_editing: false,
            is_pressed: false,
        };

        // Up at index 0 wraps to 7 (last item)
        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Up,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 7);
        assert_eq!(view.scroll_top, 4, "Windowed bottom scroll_top: 8 - 4 = 4");
        // On wrap-around, it should immediately jump without crawling across all items
        assert_eq!(view.scroller.current(), 4 * 13);
        assert_eq!(view.scroller.target(), 4 * 13);
        assert!(!view.scroller.is_animating());

        // Down at index 7 wraps back to 0
        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 0);
        assert_eq!(view.scroll_top, 0);
        assert_eq!(view.scroller.current(), 0);
        assert_eq!(view.scroller.target(), 0);
        assert!(!view.scroller.is_animating());
    }

    #[test]
    fn test_menu_view_paged_mode() {
        let mut view = MenuView::new();
        view.mode = ListScrollMode::Paged;
        let mut items = TEST_ITEMS_8;
        let mut model = MenuModelMut {
            items: &mut items,
            selected_idx: 0,
            nav_layout: 1,
            is_editing: false,
            is_pressed: false,
        };

        // Move 0 -> 1 -> 2 -> 3
        for _ in 0..3 {
            view.on_event(
                &mut model,
                &Event::Key {
                    code: KeyCode::Down,
                    state: KeyState::Pressed,
                },
            );
        }
        assert_eq!(view.scroll_top, 0);

        // Move 3 -> 4: in paged mode, page flips from 0 to 4 (4 * 13 = 52px)
        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 4);
        assert_eq!(view.scroll_top, 4);
        assert_eq!(view.scroller.target(), 52);
    }

    #[test]
    fn test_menu_view_small_list_repeat_clamps() {
        let mut view = MenuView::new();
        let mut items = [
            MenuItem::new("Item 0", MenuItemType::Label),
            MenuItem::new("Item 1", MenuItemType::Label),
            MenuItem::new("Item 2", MenuItemType::Label),
        ];
        let mut model = MenuModelMut {
            items: &mut items,
            selected_idx: 0,
            nav_layout: 1,
            is_editing: false,
            is_pressed: false,
        };

        // 1. Single-press tap wraps around
        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Up,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 2, "Single press Up at 0 wraps to 2");

        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 0, "Single press Down at 2 wraps to 0");

        // 2. Hold-down repeat clamps to ends without runaway loop
        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 1);

        // Feed ticks to trigger repeater hold
        for _ in 0..10 {
            view.on_event(&mut model, &Event::Tick);
        }
        // Selection must clamp at 2 (the last item) and NOT wrap around to 0
        assert_eq!(
            model.selected_idx, 2,
            "Repeat hold Down must clamp at last item on small list"
        );

        // Now hold Up
        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Up,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 1);

        for _ in 0..10 {
            view.on_event(&mut model, &Event::Tick);
        }
        // Selection must clamp at 0 and NOT wrap around to 2
        assert_eq!(
            model.selected_idx, 0,
            "Repeat hold Up must clamp at 0 on small list"
        );
    }

    #[test]
    fn test_option_sheet_small_repeat_clamps() {
        let mut sheet = OptionSheet::new("Beep", SheetOptions::Packed("Off\0Tone\0Voice\0"), 0);
        let mut rep = crate::gui::KeyRepeater::new(4, 2);

        // 1. Single tap wraps
        let res = handle_sheet_event_with_repeater(
            &mut rep,
            &mut sheet,
            &Event::Key {
                code: KeyCode::Up,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(res, SheetResult::Changed);
        assert_eq!(sheet.hover, 2, "Single press Up wraps to 2");

        let res = handle_sheet_event_with_repeater(
            &mut rep,
            &mut sheet,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(res, SheetResult::Changed);
        assert_eq!(sheet.hover, 0, "Single press Down wraps to 0");

        // 2. Hold Down repeat clamps at 2
        handle_sheet_event_with_repeater(
            &mut rep,
            &mut sheet,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(sheet.hover, 1);

        for _ in 0..10 {
            handle_sheet_event_with_repeater(&mut rep, &mut sheet, &Event::Tick);
        }
        assert_eq!(
            sheet.hover, 2,
            "OptionSheet repeat hold Down must clamp at 2"
        );

        // 3. Hold Up repeat clamps at 0
        handle_sheet_event_with_repeater(
            &mut rep,
            &mut sheet,
            &Event::Key {
                code: KeyCode::Up,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(sheet.hover, 1);

        for _ in 0..10 {
            handle_sheet_event_with_repeater(&mut rep, &mut sheet, &Event::Tick);
        }
        assert_eq!(sheet.hover, 0, "OptionSheet repeat hold Up must clamp at 0");
    }

    #[test]
    fn shortcut_menu_row_and_icon_sheet_render_cleanly() {
        let mut canvas = Canvas::new();
        let label = ShortString::from_str("Tasks");
        let item = MenuItem::new("M", MenuItemType::Shortcut(label));
        render_menu_row(
            &mut canvas,
            &item,
            0b11,
            14,
            Some(icons::ICON_FOLDER),
            0,
            false,
        );
        assert!(canvas.buffer.iter().flatten().any(|&b| b != 0));

        static TEST_ICONS: [&[u8; 8]; 2] = [icons::ICON_FOLDER, icons::ICON_VFO];
        let sheet = OptionSheet::new("Bind", SheetOptions::Packed("Tasks\0VFO\0"), 0)
            .with_radio(true)
            .with_icons(&TEST_ICONS);
        canvas.clear();
        MenuView::render_sheet(&mut canvas, &sheet);
        assert!(canvas.buffer.iter().flatten().any(|&b| b != 0));
    }

    #[test]
    fn focused_long_menu_label_scrolls_and_resets_on_nav() {
        let mut view = MenuView::new();
        let mut items = [
            MenuItem::new("Very Long Localized Menu Entry Name", MenuItemType::Submenu),
            MenuItem::new("Short", MenuItemType::Label),
        ];
        let mut canvas = Canvas::new();
        let ro_model = MenuModel {
            title: "Test",
            items: &items,
            selected_idx: 0,
            icons: None,
            is_editing: false,
            is_pressed: false,
        };
        view.render(&mut canvas, &ro_model);
        assert!(view.marquee_overflow.get() > 0);

        let mut rw_model = MenuModelMut {
            items: &mut items,
            selected_idx: 0,
            nav_layout: 1,
            is_editing: false,
            is_pressed: false,
        };
        let mut moved = false;
        for _ in 0..40 {
            if view.on_event(&mut rw_model, &Event::Tick) {
                moved = true;
            }
        }
        assert!(moved);
        assert!(view.marquee.offset() > 0);

        view.on_event(
            &mut rw_model,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(rw_model.selected_idx, 1);
        assert_eq!(view.marquee.offset(), 0);
        assert!(!view.marquee.is_active());
    }
}
