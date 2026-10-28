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
    Sheet(OptionSheet, KeyRepeater, gfx::gui::animation::SlideUpAnim) = 3,
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
    pub fn is_animating(&self) -> bool {
        match &self.modal {
            ActiveModal::Sheet(_, _, anim) => anim.is_animating(),
            ActiveModal::Popup(model) => !model.anim.is_complete(),
            _ => false,
        }
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

    fn init_text_prompt(
        &mut self,
        tag: u8,
        title: &'static str,
        max_len: usize,
        digits_only: bool,
        is_passcode: bool,
    ) -> &mut TextInputModel {
        self.tag = tag;
        let mut model = TextInputModel::new(max_len);
        model.title = title;
        model.is_passcode = is_passcode;
        model.digits_only = digits_only;
        model.show_header = true;
        if digits_only {
            model.charset = gfx::gui::modules::text_input::Charset::Numbers;
        }
        self.modal = ActiveModal::TextInput(model);
        match &mut self.modal {
            ActiveModal::TextInput(m) => m,
            _ => unreachable!(),
        }
    }

    pub fn prompt_text(
        &mut self,
        tag: u8,
        title: &'static str,
        initial: &str,
        max_len: usize,
        digits_only: bool,
    ) {
        let m = self.init_text_prompt(tag, title, max_len, digits_only, false);
        let _ = m.buffer.push_str(initial);
    }

    pub fn prompt_passcode(
        &mut self,
        tag: u8,
        title: &'static str,
        max_len: usize,
        digits_only: bool,
        expose_len: bool,
    ) {
        let m = self.init_text_prompt(tag, title, max_len, digits_only, true);
        m.expose_len = expose_len;
    }

    pub fn show_sheet(&mut self, tag: u8, sheet: OptionSheet) {
        self.tag = tag;
        let is_animated = crate::master_config_ref().display.ui_animations != 0;
        let mut anim = gfx::gui::animation::SlideUpAnim::new(48, 8);
        if is_animated {
            anim.open();
        } else {
            anim.jump_open();
        }
        self.modal = ActiveModal::Sheet(sheet, KeyRepeater::new(8, 2), anim);
    }

    pub fn handle_event(&mut self, event: &Event) -> Option<ModalResult<'_>> {
        let tag = self.tag;
        match &mut self.modal {
            ActiveModal::None => None,
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
                        KeyCode::Menu => Some(ModalResult::Confirmed(tag)),
                        KeyCode::Exit => Some(ModalResult::Dismissed(tag)),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            ActiveModal::TextInput(model) => {
                self.text_view.on_event(model, event);
                if model.is_done {
                    Some(ModalResult::TextInputDone {
                        tag,
                        text: model.buffer.as_str(),
                    })
                } else if model.is_canceled {
                    Some(ModalResult::Canceled(tag))
                } else {
                    None
                }
            }
            ActiveModal::Sheet(sheet, repeater, anim) => {
                if let Event::Tick = event {
                    anim.tick();
                }
                match handle_sheet_event_with_repeater(repeater, sheet, event) {
                    SheetResult::Confirmed(idx) => {
                        Some(ModalResult::SheetSelected { tag, index: idx })
                    }
                    SheetResult::Canceled => Some(ModalResult::Canceled(tag)),
                    _ => None,
                }
            }
        }
    }

    #[inline(always)]
    pub fn dismiss(&mut self) {
        self.modal = ActiveModal::None;
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
            ActiveModal::Sheet(sheet, _, anim) => {
                let off = anim.offset();
                if off > 0 {
                    MenuView::render_sheet_slide_up(canvas, sheet, off as i8);
                } else {
                    MenuView::render_sheet(canvas, sheet);
                }
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
