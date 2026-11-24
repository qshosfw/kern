//! Declarative modular Application system and generic dependency-injected system context.

use crate::event::Event;
use crate::ui::StatusGlyph;

/// Static metadata describing an App for the OS registry and compositor.
#[derive(Debug, Clone, Copy)]
pub struct AppMetadata {
    pub name: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventResult {
    Ignored,
    Consumed,
    Error(u8),
    RecoverableFault(u8),
}

/// Status Bar Configuration requested by an application for the OS compositor.
#[derive(Debug, Clone, Copy)]
pub struct StatusBarConfig<'a> {
    pub visible: bool,
    pub partial: bool,
    pub title: Option<&'a str>,
    pub center_text: Option<&'a str>,
    pub signal_level: Option<u8>,
    pub left_badge: Option<&'a str>,
    pub left_icons: &'a [StatusGlyph],
    pub center_icons: &'a [StatusGlyph],
    pub show_divider: bool,
}

impl<'a> Default for StatusBarConfig<'a> {
    fn default() -> Self {
        Self {
            visible: true,
            partial: false,
            title: None,
            center_text: None,
            signal_level: None,
            left_badge: None,
            left_icons: &[],
            center_icons: &[],
            show_divider: true,
        }
    }
}

impl<'a> StatusBarConfig<'a> {
    pub const fn new(title: &'a str) -> Self {
        Self {
            visible: true,
            partial: false,
            title: Some(title),
            center_text: None,
            signal_level: None,
            left_badge: None,
            left_icons: &[],
            center_icons: &[],
            show_divider: true,
        }
    }

    pub const fn partial() -> Self {
        Self {
            visible: true,
            partial: true,
            title: None,
            center_text: None,
            signal_level: None,
            left_badge: None,
            left_icons: &[],
            center_icons: &[],
            show_divider: false,
        }
    }

    pub const fn hidden() -> Self {
        Self {
            visible: false,
            partial: false,
            title: None,
            center_text: None,
            signal_level: None,
            left_badge: None,
            left_icons: &[],
            center_icons: &[],
            show_divider: false,
        }
    }

    pub const fn with_partial(mut self, partial: bool) -> Self {
        self.partial = partial;
        if partial {
            self.show_divider = false;
        }
        self
    }

    pub const fn with_center(mut self, center: &'a str) -> Self {
        self.center_text = Some(center);
        self
    }

    pub const fn with_signal(mut self, level: u8) -> Self {
        self.signal_level = Some(level);
        self
    }

    pub const fn with_badge(mut self, badge: &'a str) -> Self {
        self.left_badge = Some(badge);
        self
    }

    pub const fn with_left_icons(mut self, icons: &'a [StatusGlyph]) -> Self {
        self.left_icons = icons;
        self
    }

    pub const fn with_center_icons(mut self, icons: &'a [StatusGlyph]) -> Self {
        self.center_icons = icons;
        self
    }

    pub const fn with_divider(mut self, show_divider: bool) -> Self {
        self.show_divider = show_divider;
        self
    }

    pub const fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

/// Result returned from an OS modal dialog or input prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalResult<'a> {
    Dismissed(u8),
    Confirmed(u8),
    TextInputDone { tag: u8, text: &'a str },
    NumberInputDone { tag: u8, value: i32 },
    SheetSelected { tag: u8, index: usize },
    Canceled(u8),
}

/// Declarative Application interface for microkernel GUI and foreground modules.
pub trait App<C, Fb> {
    /// Called by the OS when launched into the foreground.
    fn on_start(&mut self, ctx: &mut C);

    /// Core event handler receiving ownership of events and localized context.
    fn on_event(&mut self, ctx: &mut C, event: &Event) -> EventResult;

    /// Called when an OS modal (popup, text prompt, or option sheet) produces a result.
    fn on_modal_result(&mut self, _ctx: &mut C, _result: &ModalResult<'_>) -> EventResult {
        EventResult::Ignored
    }

    /// Called during the OS render loop.
    fn render(&self, canvas: &mut Fb, ctx: &C);

    /// Called when backgrounded or closed.
    fn on_exit(&mut self, ctx: &mut C);
}
