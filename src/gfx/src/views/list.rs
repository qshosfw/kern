//! Declarative Scrollable List View (`ListView`, `ListModel`, `ListItem`).

use super::View;
use super::animation::{KineticScroller, ListScrollMode, MarqueeScroller};
use super::repeater::KeyRepeater;
use super::sheet::SheetOptions;
use crate::elements::{
    ListRowSpec, ROW_ARROW_LEFT, ROW_ARROW_RIGHT, ROW_CHEVRON, ROW_OUTLINE, ROW_SELECTED,
    ROW_SHORTCUT, ROW_TOGGLE, ROW_TOGGLE_ON, draw_list_row, draw_scrollbar,
};
use crate::fonts::tomthumb::FONT as TOMTHUMB;
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
        self.len = utils::format::copy_trunc_utf8(&mut self.bytes, 0, s);
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
        self.len = utils::format::copy_trunc_utf8(&mut self.bytes, self.len as usize, s);
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
pub enum ListItemKind {
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
pub struct ListItem {
    pub title: &'static str,
    pub kind: ListItemKind,
}

impl ListItem {
    pub const fn new(title: &'static str, kind: ListItemKind) -> Self {
        Self { title, kind }
    }
}

pub struct ListModel<'a> {
    pub title: &'static str,
    pub items: &'a [ListItem],
    pub selected_idx: usize,
    pub icons: Option<&'a [&'static [u8; 8]]>,
    pub is_editing: bool,
    pub is_pressed: bool,
}

pub struct ListModelMut<'a> {
    pub items: &'a mut [ListItem],
    pub selected_idx: usize,
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
    RightInc,
    LeftDec,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyNavStep {
    Primary(bool),
    Keypad(bool),
}

#[inline(always)]
pub(crate) fn classify_nav_key(code: KeyCode) -> Option<KeyNavStep> {
    match code {
        KeyCode::Up | KeyCode::Key2 => Some(KeyNavStep::Primary(false)),
        KeyCode::Down | KeyCode::Key8 => Some(KeyNavStep::Primary(true)),
        KeyCode::Key4 => Some(KeyNavStep::Keypad(false)),
        KeyCode::Key6 => Some(KeyNavStep::Keypad(true)),
        _ => None,
    }
}

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

pub struct ListView {
    pub repeater: KeyRepeater,
    pub scroller: KineticScroller,
    pub marquee: MarqueeScroller,
    marquee_overflow: Cell<u8>,
    pub mode: ListScrollMode,
    pub scroll_top: usize,
    pub animated: bool,
}

impl ListView {
    pub const fn new() -> Self {
        Self {
            repeater: KeyRepeater::new(8, 2),
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

    fn move_selection(
        &mut self,
        model: &mut ListModelMut<'_>,
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

    pub fn on_event(&mut self, model: &mut ListModelMut<'_>, event: &Event) -> bool {
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
            } else {
                let dir = if *delta > 0 {
                    ListNavDir::Down
                } else {
                    ListNavDir::Up
                };
                for _ in 0..steps {
                    self.move_selection(model, dir, true, 5);
                }
            }
            return true;
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
            if let Some(repeat_code) = self.repeater.tick() {
                if model.is_editing {
                    if let Some(dir) = get_edit_change_dir(model.nav_layout, repeat_code) {
                        apply_change_dir(&mut model.items[model.selected_idx], dir);
                        changed = true;
                    }
                } else if let Some(dir) = get_list_nav_dir(model.nav_layout, repeat_code)
                    && self.move_selection(model, dir, model.items.len() > 4, 6)
                {
                    changed = true;
                }
            }
            return changed;
        }

        if let Event::Key { code, state } = event {
            self.repeater.handle_key(*code, *state);
            let is_released = *state == KeyState::Released;
            let is_pressed = *state == KeyState::Pressed;
            let item = &mut model.items[model.selected_idx];
            let is_action = matches!(
                item.kind,
                ListItemKind::Action | ListItemKind::Submenu | ListItemKind::Value(_)
            );

            if is_released {
                if model.is_pressed {
                    model.is_pressed = false;
                    if is_action && (*code == KeyCode::Menu || *code == KeyCode::Key5) {
                        return true;
                    }
                }
                return classify_nav_key(*code).is_some();
            }

            if model.is_editing {
                if is_pressed {
                    if let Some(dir) = get_edit_change_dir(model.nav_layout, *code) {
                        apply_change_dir(item, dir);
                    } else if matches!(code, KeyCode::Menu | KeyCode::Exit | KeyCode::Key5) {
                        self.repeater.reset();
                        model.is_editing = false;
                    }
                }
                return true;
            }

            if is_pressed {
                if let Some(dir) = get_list_nav_dir(model.nav_layout, *code) {
                    self.move_selection(model, dir, true, 5);
                    return true;
                }
                match code {
                    KeyCode::Menu | KeyCode::Key5 => {
                        if matches!(item.kind, ListItemKind::Range { .. }) {
                            self.repeater.reset();
                            model.is_editing = true;
                        } else if let ListItemKind::Toggle(ref mut on) = item.kind {
                            *on = !*on;
                        } else {
                            model.is_pressed = true;
                        }
                        return true;
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

impl Default for ListView {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_change_dir(item: &mut ListItem, dir: ChangeDir) {
    match dir {
        ChangeDir::RightInc => {
            if let ListItemKind::Range { val, max, step, .. } = &mut item.kind
                && *val < *max
            {
                *val = (*val + *step).min(*max);
            }
        }
        ChangeDir::LeftDec => {
            if let ListItemKind::Range { val, min, step, .. } = &mut item.kind
                && *val > *min
            {
                *val = (*val - *step).max(*min);
            }
        }
    }
}

fn apply_list_nav_dir(model: &mut ListModelMut<'_>, dir: ListNavDir, wrap: bool) -> bool {
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

pub(crate) fn render_list_item_row(
    canvas: &mut Canvas,
    item: &ListItem,
    flags: u8,
    row_y: i16,
    icon: Option<&[u8; 8]>,
    marquee_x: i16,
    marquee_enabled: bool,
) -> u8 {
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
    let mut value = "";
    let mut localize_value = false;

    match &item.kind {
        ListItemKind::Shortcut(action_label) => {
            row_flags |= ROW_SHORTCUT;
            value = action_label.as_str();
        }
        ListItemKind::Value(v) => {
            value = v.as_str();
            localize_value = true;
        }
        ListItemKind::Choice { selected, options } => {
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
        ListItemKind::Range {
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
        ListItemKind::Toggle(on) => {
            row_flags |= ROW_TOGGLE;
            if *on {
                row_flags |= ROW_TOGGLE_ON;
            }
        }
        ListItemKind::Submenu => {
            row_flags |= ROW_CHEVRON;
        }
        _ => {}
    }

    let mut spec = ListRowSpec::new(0, row_y, 124, 13, item.title);
    spec.flags = row_flags;
    spec.value = value;
    spec.icon = icon;
    spec.marquee_x = marquee_x;
    spec.marquee_enabled = marquee_enabled;
    spec.localize_value = localize_value;
    spec.clip_top = 8;
    if is_selected && is_editing {
        spec.flags |= ROW_OUTLINE;
    }
    draw_list_row(canvas, &spec)
}

impl<'a> View<ListModel<'a>> for ListView {
    fn render(&self, canvas: &mut Canvas, model: &ListModel<'a>) {
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
            let ov = render_list_item_row(
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

        canvas.fill_rect(0, 0, 128, 8, Color::Clear);
        if total_items > visible_count {
            draw_scrollbar(
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

    fn on_event(&mut self, _model: &mut ListModel<'a>, _event: &Event) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_ITEMS_8: [ListItem; 8] = [
        ListItem::new("Item 0", ListItemKind::Label),
        ListItem::new("Item 1", ListItemKind::Label),
        ListItem::new("Item 2", ListItemKind::Label),
        ListItem::new("Item 3", ListItemKind::Label),
        ListItem::new("Item 4", ListItemKind::Label),
        ListItem::new("Item 5", ListItemKind::Label),
        ListItem::new("Item 6", ListItemKind::Label),
        ListItem::new("Item 7", ListItemKind::Label),
    ];

    #[test]
    fn list_view_windowed_and_wrap_around_navigation() {
        let mut view = ListView::new();
        let mut items = TEST_ITEMS_8;
        let mut model = ListModelMut {
            items: &mut items,
            selected_idx: 0,
            nav_layout: 1,
            is_editing: false,
            is_pressed: false,
        };

        for expected in 1..=3 {
            assert!(view.on_event(
                &mut model,
                &Event::Key {
                    code: KeyCode::Down,
                    state: KeyState::Pressed,
                },
            ));
            assert_eq!(model.selected_idx, expected);
            assert_eq!(view.scroll_top, 0);
        }

        assert!(view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        ));
        assert_eq!(model.selected_idx, 4);
        assert_eq!(view.scroll_top, 1);
        while view.on_event(&mut model, &Event::Tick) {}
        assert_eq!(view.scroller.current(), 13);

        model.selected_idx = 0;
        view.reset_scroll();
        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Up,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 7);
        assert_eq!(view.scroller.current(), 52);
    }

    #[test]
    fn list_view_small_list_repeat_clamps_and_marquee_resets() {
        let mut view = ListView::new();
        let mut items = [
            ListItem::new("Very Long Localized Menu Entry Name", ListItemKind::Submenu),
            ListItem::new("Item 1", ListItemKind::Label),
            ListItem::new("Item 2", ListItemKind::Label),
        ];
        let mut canvas = Canvas::new();
        view.render(
            &mut canvas,
            &ListModel {
                title: "Test",
                items: &items,
                selected_idx: 0,
                icons: None,
                is_editing: false,
                is_pressed: false,
            },
        );
        assert!(view.marquee_overflow.get() > 0);

        let mut model = ListModelMut {
            items: &mut items,
            selected_idx: 0,
            nav_layout: 1,
            is_editing: false,
            is_pressed: false,
        };
        for _ in 0..40 {
            view.on_event(&mut model, &Event::Tick);
        }
        assert!(view.marquee.offset() > 0);

        view.on_event(
            &mut model,
            &Event::Key {
                code: KeyCode::Down,
                state: KeyState::Pressed,
            },
        );
        assert_eq!(model.selected_idx, 1);
        assert_eq!(view.marquee.offset(), 0);
        for _ in 0..10 {
            view.on_event(&mut model, &Event::Tick);
        }
        assert_eq!(model.selected_idx, 2);
    }
}
