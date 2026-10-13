//! OS UI Compositor
//! Automatically composites application graphics workspace, status bar overlays, and system modal dialogs.

use gfx::Canvas;
use gfx::gui::modules::menu::{
    MenuView, OptionSheet, SheetResult, handle_sheet_event_with_repeater,
};
use gfx::gui::modules::popup::{PopupModel, PopupView};
use gfx::gui::modules::text_input::{TextInputModel, TextInputView};
use gfx::gui::repeater::KeyRepeater;
use gfx::gui::view::View;
use kernel::app::ModalResult;
use kernel::event::{Event, KeyCode, KeyState};

use super::keypad_lock::KeypadLockManager;
use crate::services::FirmwareContext;

#[repr(u8)]
pub enum ActiveModal {
    None = 0,
    Popup(PopupModel) = 1,
    TextInput(TextInputModel) = 2,
    Sheet(OptionSheet, KeyRepeater) = 3,
}

pub struct ModalCompositor {
    pub tag: u8,
    pub modal: ActiveModal,
    pub popup_view: PopupView,
    pub text_view: TextInputView,
}

impl ModalCompositor {
    pub const fn new() -> Self {
        Self {
            tag: 0,
            modal: ActiveModal::None,
            popup_view: PopupView::new(),
            text_view: TextInputView::new(),
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        !matches!(self.modal, ActiveModal::None)
    }

    #[inline(always)]
    pub fn close(&mut self) {
        self.modal = ActiveModal::None;
    }

    pub fn show_popup(&mut self, tag: u8, title: &'static str, message: &'static str) {
        self.tag = tag;
        self.modal = ActiveModal::Popup(PopupModel::new(title, message));
    }

    pub fn show_popup_str(&mut self, tag: u8, title: &'static str, message: &str) {
        let mut model = PopupModel::new(title, "");
        let _ = model.custom_msg.push_str(message);
        self.tag = tag;
        self.modal = ActiveModal::Popup(model);
    }

    pub fn show_confirm(
        &mut self,
        tag: u8,
        title: &'static str,
        message: &'static str,
        confirm: &'static str,
        cancel: &'static str,
    ) {
        self.tag = tag;
        self.modal =
            ActiveModal::Popup(PopupModel::new(title, message).with_badges(confirm, cancel));
    }

    pub fn prompt_text(
        &mut self,
        tag: u8,
        title: &'static str,
        initial: &str,
        max_len: usize,
        digits_only: bool,
    ) {
        let mut model = TextInputModel::new(max_len);
        model.title = title;
        model.is_passcode = false;
        model.digits_only = digits_only;
        model.show_header = true;
        if digits_only {
            model.charset = gfx::gui::modules::text_input::Charset::Numbers;
        }
        let _ = model.buffer.push_str(initial);
        self.tag = tag;
        self.modal = ActiveModal::TextInput(model);
    }

    pub fn prompt_passcode(
        &mut self,
        tag: u8,
        title: &'static str,
        max_len: usize,
        digits_only: bool,
        expose_len: bool,
    ) {
        let mut model = TextInputModel::new(max_len);
        model.title = title;
        model.is_passcode = true;
        model.digits_only = digits_only;
        model.expose_len = expose_len;
        model.charset = if digits_only {
            gfx::gui::modules::text_input::Charset::Numbers
        } else {
            gfx::gui::modules::text_input::Charset::Upper
        };
        model.show_header = true;
        self.tag = tag;
        self.modal = ActiveModal::TextInput(model);
    }

    pub fn show_sheet(&mut self, tag: u8, sheet: OptionSheet) {
        self.tag = tag;
        self.modal = ActiveModal::Sheet(sheet, KeyRepeater::new(8, 2));
    }

    pub fn handle_event(&mut self, event: &Event) -> Option<ModalResult> {
        let tag = self.tag;
        let res = match &mut self.modal {
            ActiveModal::None => return None,
            ActiveModal::Popup(model) => {
                if self.popup_view.on_event(model, event) {
                    return None;
                }
                if let Event::Key {
                    code,
                    state: KeyState::Pressed,
                } = event
                {
                    match code {
                        KeyCode::Menu => ModalResult::Confirmed(tag),
                        KeyCode::Exit => ModalResult::Dismissed(tag),
                        _ => return None,
                    }
                } else {
                    return None;
                }
            }
            ActiveModal::TextInput(model) => {
                self.text_view.on_event(model, event);
                if model.is_done {
                    ModalResult::TextInputDone {
                        tag,
                        text: core::mem::take(&mut model.buffer),
                    }
                } else if model.is_canceled {
                    ModalResult::Canceled(tag)
                } else {
                    return None;
                }
            }
            ActiveModal::Sheet(sheet, repeater) => {
                match handle_sheet_event_with_repeater(repeater, sheet, event) {
                    SheetResult::Confirmed(idx) => ModalResult::SheetSelected { tag, index: idx },
                    SheetResult::Canceled => ModalResult::Canceled(tag),
                    _ => return None,
                }
            }
        };
        self.modal = ActiveModal::None;
        Some(res)
    }

    #[inline(never)]
    pub fn render_overlay(&self, canvas: &mut Canvas) {
        match &self.modal {
            ActiveModal::None => {}
            ActiveModal::Popup(model) => {
                self.popup_view.render(canvas, model);
            }
            ActiveModal::TextInput(model) => {
                self.text_view.render(canvas, model);
            }
            ActiveModal::Sheet(sheet, _) => {
                MenuView::render_sheet(canvas, sheet);
            }
        }
    }
}

pub static mut ACTIVE_MODAL: ModalCompositor = ModalCompositor::new();

pub struct OsCompositor;

impl OsCompositor {
    #[inline(never)]
    pub fn render<'a>(
        active_app_id: usize,
        canvas: &mut Canvas,
        ctx: &'a FirmwareContext<'a, '_>,
        lock_mgr: &KeypadLockManager,
    ) {
        canvas.clear();

        let mut sb_cfg = kernel::app::StatusBarConfig::default();
        crate::AppRegistry::render(active_app_id, canvas, ctx, &mut sb_cfg);
        if sb_cfg.visible {
            ctx.draw_status_bar_cfg(canvas, &sb_cfg);
        }

        // Render OS modal overlay on top of screen if active
        unsafe {
            let modal_ref = &*core::ptr::addr_of!(ACTIVE_MODAL);
            if modal_ref.is_active() {
                modal_ref.render_overlay(canvas);
                if matches!(modal_ref.modal, ActiveModal::TextInput(_)) {
                    ctx.draw_status_bar_right_mode(canvas, true);
                }
            }
        }

        // Render Keypad Lock Toast Modal overlay on top of screen if active
        lock_mgr.render_overlay(canvas);
    }
}
