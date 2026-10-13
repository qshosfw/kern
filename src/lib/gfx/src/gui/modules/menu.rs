//! Modular Declarative Menu GUI
//! High-contrast hover/edit interactions, checkboxes, and navigation arrows.
//! 100% Safe Rust with clean separation between mutable event handling and immutable rendering.

use crate::fonts::mumatrix_regular::FONT as MUMATRIX;
use crate::fonts::tomthumb::FONT as TOMTHUMB;
use crate::gui::view::View;
use crate::icons;
use crate::text;
use crate::{Canvas, Color};
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
        let mut buf = [0u8; 10];
        s.push_str(crate::text::format_u32(val, &mut buf));
        s
    }

    pub fn from_hex32(val: u32) -> Self {
        let mut s = Self {
            bytes: [0; 18],
            len: 8,
        };
        let mut buf = [0u8; 8];
        s.bytes[..8].copy_from_slice(crate::text::format_hex32(val, &mut buf).as_bytes());
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
        let mut buf = [0u8; 18];
        let slice = crate::text::format_int_fixed(val, decimals, prefix, suffix, &mut buf);
        self.push_str(slice);
    }
}

#[derive(Clone, Copy)]
pub enum SheetOptions {
    Static(&'static [&'static str]),
    Dynamic(&'static [ShortString]),
}

impl SheetOptions {
    #[inline(always)]
    pub fn len(&self) -> usize {
        match self {
            SheetOptions::Static(s) => s.len(),
            SheetOptions::Dynamic(d) => d.len(),
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline(always)]
    pub fn get(&self, idx: usize) -> &str {
        match self {
            SheetOptions::Static(s) => {
                if idx < s.len() {
                    s[idx]
                } else {
                    ""
                }
            }
            SheetOptions::Dynamic(d) => {
                if idx < d.len() {
                    d[idx].as_str()
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
    pub selected: usize,
    pub hover: usize,
    pub show_radio: bool,
    pub is_pressed: bool,
}

impl OptionSheet {
    pub const fn new(
        title: &'static str,
        options: &'static [&'static str],
        selected: usize,
    ) -> Self {
        let hover = if selected < options.len() {
            selected
        } else {
            0
        };
        Self {
            title,
            options: SheetOptions::Static(options),
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
        options: &'static [&'static str],
    },
    Submenu,
    Info(&'static str),
    Value(ShortString),
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
}

impl MenuView {
    pub const fn new() -> Self {
        Self {
            repeater: crate::gui::repeater::KeyRepeater::new(8, 2), // 400ms initial hold delay, 100ms repeat rate @ 50ms tick
        }
    }

    pub fn on_event(&mut self, model: &mut MenuModelMut<'_>, event: &Event) -> bool {
        if model.items.is_empty() {
            return false;
        }

        if let Event::Encoder(delta) = event {
            if model.is_editing {
                let dir = if *delta > 0 {
                    ChangeDir::RightInc
                } else {
                    ChangeDir::LeftDec
                };
                apply_change_dir(&mut model.items[model.selected_idx], dir);
                return true;
            } else {
                let dir = if *delta > 0 {
                    ListNavDir::Down
                } else {
                    ListNavDir::Up
                };
                apply_list_nav_dir(model, dir);
                return true;
            }
        }

        let mut changed = false;

        if let Event::Tick = event {
            // Drive navigation / edit auto-repeat from tick
            if let Some(repeat_code) = self.repeater.tick() {
                if model.is_editing {
                    if let Some(dir) = get_edit_change_dir(model.nav_layout, repeat_code) {
                        apply_change_dir(&mut model.items[model.selected_idx], dir);
                        changed = true;
                    }
                } else if let Some(dir) = get_list_nav_dir(model.nav_layout, repeat_code) {
                    apply_list_nav_dir(model, dir);
                    changed = true;
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
                MenuItemType::Action
                    | MenuItemType::Submenu
                    | MenuItemType::Info(_)
                    | MenuItemType::Value(_)
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
                    apply_list_nav_dir(model, dir);
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

            if *state == KeyState::Held {
                if matches!(code, KeyCode::Menu | KeyCode::Key5) {
                    return true;
                }
            }
        }
        false
    }
}

fn apply_change_dir(item: &mut MenuItem, dir: ChangeDir) {
    match dir {
        ChangeDir::RightInc => match &mut item.item_type {
            MenuItemType::Range { val, max, step, .. } => {
                if *val < *max {
                    *val = (*val + *step).min(*max);
                }
            }
            _ => {}
        },
        ChangeDir::LeftDec => match &mut item.item_type {
            MenuItemType::Range { val, min, step, .. } => {
                if *val > *min {
                    *val = (*val - *step).max(*min);
                }
            }
            _ => {}
        },
    }
}

fn apply_list_nav_dir(model: &mut MenuModelMut<'_>, dir: ListNavDir) {
    let len = model.items.len();
    if len > 0 {
        match dir {
            ListNavDir::Up => {
                if model.selected_idx == 0 {
                    model.selected_idx = len - 1;
                } else {
                    model.selected_idx -= 1;
                }
            }
            ListNavDir::Down => {
                model.selected_idx += 1;
                if model.selected_idx >= len {
                    model.selected_idx = 0;
                }
            }
        }
        model.is_pressed = false;
    }
}

fn render_menu_row(
    canvas: &mut Canvas,
    item: &MenuItem,
    flags: u8,
    row_y: i16,
    icon: Option<&[u8; 8]>,
) {
    let is_selected = (flags & 1) != 0;
    let mut tc = Color::Fill;
    if is_selected {
        let ry = row_y as u8;
        if (flags & 6) != 0 {
            canvas.fill_rect(0, ry, 124, 13, Color::Fill);
            canvas.draw_pixel(0, ry, Color::Clear);
            canvas.draw_pixel(123, ry, Color::Clear);
            canvas.draw_pixel(0, ry + 12, Color::Clear);
            canvas.draw_pixel(123, ry + 12, Color::Clear);
            tc = Color::Clear;
        } else {
            canvas.draw_rect(0, ry, 124, 13, Color::Fill);
            canvas.draw_pixel(0, ry, Color::Clear);
            canvas.draw_pixel(123, ry, Color::Clear);
            canvas.draw_pixel(0, ry + 12, Color::Clear);
            canvas.draw_pixel(123, ry + 12, Color::Clear);
        }
    }

    let text_x = if let Some(bmp) = icon {
        icons::draw_icon_8(canvas, 3, row_y + 2, bmp, tc);
        16
    } else {
        3
    };

    let mut buf = [0u8; 16];
    let mut val_str = "";
    let mut arrows = 0u8;
    let mut max_title_x = 110i16;

    match &item.item_type {
        MenuItemType::Info(s) => val_str = s,
        MenuItemType::Value(v) => val_str = v.as_str(),
        MenuItemType::Choice { selected, options } => {
            let sel = *selected as usize;
            val_str = if sel < options.len() {
                options[sel]
            } else {
                "?"
            };
            if sel > 0 {
                arrows |= 1;
            }
            if sel + 1 < options.len() {
                arrows |= 2;
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
                arrows |= 1;
            }
            if *val < *max {
                arrows |= 2;
            }
            val_str = text::format_range_val(*val as i32, *min as i32, unit, &mut buf);
        }
        MenuItemType::Toggle(on) => {
            icons::draw_icon_8(
                canvas,
                111,
                row_y + 2,
                if *on {
                    &icons::CHECKBOX_ON
                } else {
                    &icons::CHECKBOX_OFF
                },
                tc,
            );
            max_title_x = 105;
        }
        MenuItemType::Submenu => {
            icons::draw_icon_8(canvas, 114, row_y + 3, &icons::ARROW_RIGHT, tc);
            max_title_x = 105;
        }
        _ => {}
    }

    let sw = if val_str.is_empty() {
        0
    } else {
        text::str_width(val_str, &TOMTHUMB) as i16
    };
    let title_max = if sw > 0 {
        (114 - sw).max(30)
    } else {
        max_title_x
    };
    text::draw_str_max_x(
        canvas,
        text_x,
        row_y + 9,
        item.title,
        &MUMATRIX,
        tc,
        title_max,
    );

    if sw > 0 {
        let text_x = if (flags & 3) == 3 {
            if (arrows & 2) != 0 {
                icons::draw_icon_8(canvas, 116, row_y + 3, &icons::ARROW_RIGHT, tc);
            }
            if (arrows & 1) != 0 {
                icons::draw_icon_8(
                    canvas,
                    (106 - sw).max(40),
                    row_y + 3,
                    &icons::ARROW_LEFT,
                    tc,
                );
            }
            114
        } else {
            118
        };
        text::draw_str(canvas, text_x - sw, row_y + 8, val_str, &TOMTHUMB, tc);
    }
}

impl<'a> View<MenuModel<'a>> for MenuView {
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

        // 2. Exact 13px Row List Menu (4 rows visible)
        let visible_count = 4;
        let start_idx = model.selected_idx & !3;
        let end_idx = (start_idx + visible_count).min(total_items);

        for (row_slot, idx) in (start_idx..end_idx).enumerate() {
            let row_y = 8 + (row_slot as i16 * 13);
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
            render_menu_row(canvas, &model.items[idx], flags, row_y, icon);
        }

        // 3. Scrollbar
        if total_items > visible_count {
            crate::widgets::draw_scrollbar(
                canvas,
                126,
                8,
                56,
                model.selected_idx,
                total_items,
                visible_count,
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
        if sheet.options.is_empty() {
            return;
        }

        let sheet_y = 16i16;
        let sheet_h = 48u8;

        crate::widgets::draw_sheet_frame(canvas, sheet.title, sheet_y, sheet_h, None);

        let visible_count = 3usize;
        let total = sheet.options.len();
        let page = sheet.hover / visible_count;
        let start_idx = page * visible_count;
        let end_idx = (start_idx + visible_count).min(total);

        let row_start_y = sheet_y + 11;
        let fill_w = if total > visible_count { 118 } else { 122 };
        let fill_x = if total > visible_count { 2 } else { 3 };

        for (slot, idx) in (start_idx..end_idx).enumerate() {
            let row_y = row_start_y + (slot as i16 * 12);
            let is_hovered = idx == sheet.hover;
            let is_selected = idx == sheet.selected;

            let mut tc = Color::Fill;
            if is_hovered {
                let ry = row_y as u8;
                canvas.fill_rect(fill_x, ry, fill_w, 11, Color::Fill);
                canvas.draw_pixel(fill_x, ry, Color::Clear);
                canvas.draw_pixel(fill_x + fill_w - 1, ry, Color::Clear);
                canvas.draw_pixel(fill_x, ry + 10, Color::Clear);
                canvas.draw_pixel(fill_x + fill_w - 1, ry + 10, Color::Clear);
                tc = Color::Clear;
            }

            let sprite = if sheet.show_radio {
                Some(if is_selected {
                    &icons::RADIO_ON
                } else {
                    &icons::RADIO_OFF
                })
            } else if is_hovered {
                Some(&icons::ARROW_RIGHT)
            } else {
                None
            };
            if let Some(s) = sprite {
                icons::draw_icon_8(canvas, 5, row_y + 2, s, tc);
            }
            let tx = if sheet.show_radio { 16 } else { 15 };
            text::draw_str(canvas, tx, row_y + 8, sheet.options.get(idx), &MUMATRIX, tc);
        }

        if total > visible_count {
            let track_y = (sheet_y + 10) as u8;
            let track_h = 36u8;
            crate::widgets::draw_scrollbar(
                canvas,
                124,
                track_y,
                track_h,
                sheet.hover,
                total,
                visible_count,
            );
        }
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
        (hover + 1) % total
    } else {
        (hover + total - 1) % total
    }
}

pub fn handle_sheet_event_with_repeater(
    repeater: &mut crate::gui::repeater::KeyRepeater,
    sheet: &mut OptionSheet,
    event: &Event,
) -> SheetResult {
    if sheet.options.is_empty() {
        return SheetResult::None;
    }
    let total = sheet.options.len();

    if let Event::Tick = event {
        if let Some(code) = repeater.tick() {
            if let Some(down) = sheet_nav_dir(code) {
                sheet.hover = step_hover(sheet.hover, total, down);
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
                KeyState::Released => {
                    if sheet.is_pressed && matches!(code, KeyCode::Menu | KeyCode::Key5) {
                        sheet.is_pressed = false;
                        sheet.selected = sheet.hover;
                        return SheetResult::Confirmed(sheet.hover);
                    }
                }
                _ => {}
            }
        }
        Event::Encoder(delta) => {
            sheet.hover = step_hover(sheet.hover, total, *delta > 0);
            return SheetResult::Changed;
        }
        _ => {}
    }
    SheetResult::None
}
