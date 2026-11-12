//! OS UI Compositor
//! Automatically composites application graphics workspace, status bar overlays, and system modal dialogs.

use gfx::Canvas;
use gfx::gui::modules::menu::{
    MenuView, OptionSheet, SheetResult, handle_sheet_event_with_repeater,
};
use gfx::gui::modules::popup::{PopupModel, PopupView};
#[cfg(feature = "text_input")]
use gfx::gui::modules::text_input::{TextInputModel, TextInputView};
use gfx::gui::modules::toast::{ToastModel, ToastView};
use gfx::gui::repeater::KeyRepeater;
use gfx::gui::view::View;
use kernel::app::ModalResult;
use kernel::event::{Event, KeyCode, KeyState};

use super::keypad_lock::KeypadLockManager;
use crate::services::SysContext;

#[repr(u8)]
pub enum ActiveModal {
    None = 0,
    Popup(PopupModel) = 1,
    #[cfg(feature = "text_input")]
    TextInput(TextInputModel) = 2,
    Sheet(OptionSheet, KeyRepeater, gfx::gui::animation::SlideUpAnim) = 3,
}

pub struct ModalCompositor {
    pub tag: u8,
    pub toast: ToastModel,
    pub toast_view: ToastView,
    pub popup_view: PopupView,
    #[cfg(feature = "text_input")]
    pub text_view: TextInputView,
    pub modal: ActiveModal,
}

impl ModalCompositor {
    pub const fn new() -> Self {
        Self {
            tag: 0,
            toast: ToastModel::new(),
            toast_view: ToastView::new(),
            popup_view: PopupView::new(),
            #[cfg(feature = "text_input")]
            text_view: TextInputView::new(),
            modal: ActiveModal::None,
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        !matches!(self.modal, ActiveModal::None) || self.toast.is_active
    }

    #[inline(always)]
    pub fn is_animating(&self) -> bool {
        if self.toast.is_active {
            return true;
        }
        match &self.modal {
            ActiveModal::Sheet(_, _, anim) => anim.is_animating(),
            ActiveModal::Popup(model) => !model.anim.is_complete(),
            _ => false,
        }
    }

    pub fn show_toast(&mut self, msg: &str, duration_ms: u16) {
        self.toast.show(msg, duration_ms);
    }

    pub fn tick(&mut self, dt_ms: u16) {
        self.toast.tick(dt_ms);
    }

    #[inline(always)]
    pub fn close(&mut self) {
        self.modal = ActiveModal::None;
    }

    #[inline(never)]
    pub fn show_confirm(
        &mut self,
        tag: u8,
        title: &str,
        message: &str,
        confirm: &'static str,
        cancel: &'static str,
    ) {
        crate::orchestrator::sink_enter_keys();
        self.tag = tag;
        self.modal = ActiveModal::Popup(PopupModel::new());
        if let ActiveModal::Popup(model) = &mut self.modal {
            if !confirm.is_empty() && !cancel.is_empty() {
                model.confirm_label = Some(confirm);
                model.cancel_label = Some(cancel);
            }
            let _ = model.custom_title.push_str(title);
            let _ = model.custom_msg.push_str(message);
        }
    }

    #[inline(always)]
    pub fn show_popup(&mut self, tag: u8, title: &str, message: &str) {
        self.show_confirm(tag, title, message, "", "");
    }

    #[cfg(feature = "text_input")]
    fn init_text_prompt(
        &mut self,
        tag: u8,
        title: &str,
        max_len: usize,
        digits_only: bool,
        is_passcode: bool,
    ) -> &mut TextInputModel {
        crate::orchestrator::sink_enter_keys();
        self.tag = tag;
        let mut model = TextInputModel::new(max_len);
        let _ = model.custom_title.push_str(title);
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

    #[cfg(feature = "text_input")]
    pub fn prompt_text(
        &mut self,
        tag: u8,
        title: &str,
        initial: &str,
        max_len: usize,
        digits_only: bool,
    ) {
        let m = self.init_text_prompt(tag, title, max_len, digits_only, false);
        let _ = m.buffer.push_str(initial);
    }

    #[cfg(feature = "text_input")]
    pub fn prompt_passcode(
        &mut self,
        tag: u8,
        title: &str,
        max_len: usize,
        digits_only: bool,
        expose_len: bool,
    ) {
        let m = self.init_text_prompt(tag, title, max_len, digits_only, true);
        m.expose_len = expose_len;
    }

    pub fn show_sheet(&mut self, tag: u8, sheet: OptionSheet) {
        crate::orchestrator::sink_enter_keys();
        self.tag = tag;
        let is_animated = crate::master_config_ref().display.animations_enabled();
        let mut anim = gfx::gui::animation::SlideUpAnim::new(48, 8);
        if is_animated {
            anim.open();
        } else {
            anim.jump_open();
        }
        self.modal = ActiveModal::Sheet(sheet, KeyRepeater::new(8, 2), anim);
    }

    #[inline(never)]
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
            #[cfg(feature = "text_input")]
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
            #[cfg(feature = "text_input")]
            ActiveModal::TextInput(model) => {
                self.text_view.render(canvas, model);
            }
            ActiveModal::Sheet(sheet, _, anim) => {
                MenuView::render_sheet_slide_up(canvas, sheet, anim.offset() as i8);
            }
        }
        if self.toast.is_active {
            self.toast_view.render(canvas, &self.toast);
        }
    }
}

pub static mut ACTIVE_MODAL: ModalCompositor = ModalCompositor::new();

pub const TAG_LOW_BATTERY_POPUP: u8 = 0xFE;

/// Displays an immediate non-blocking Low Battery alert modal on top of active screen.
pub fn show_low_battery_popup() {
    crate::orchestrator::sink_enter_keys();
    unsafe {
        (*core::ptr::addr_of_mut!(ACTIVE_MODAL)).show_popup(
            TAG_LOW_BATTERY_POPUP,
            "LOW BATTERY",
            "Please Charge",
        );
    }
}

pub struct OsCompositor;

impl OsCompositor {
    #[inline(never)]
    pub fn render<'a>(
        active_app_id: usize,
        canvas: &mut Canvas,
        ctx: &'a SysContext<'a, '_>,
        lock_mgr: &KeypadLockManager,
    ) {
        canvas.clear();

        let mut sb_cfg = kernel::app::StatusBarConfig::default();
        crate::AppRegistry::render(active_app_id, canvas, ctx, &mut sb_cfg);

        let modal_ref = unsafe { &*core::ptr::addr_of!(ACTIVE_MODAL) };
        let has_modal = modal_ref.is_active();

        if sb_cfg.visible {
            #[cfg(feature = "text_input")]
            let is_text_input = has_modal && matches!(modal_ref.modal, ActiveModal::TextInput(_));
            #[cfg(not(feature = "text_input"))]
            let is_text_input = false;

            if !is_text_input {
                if !sb_cfg.partial && sb_cfg.title.is_none() {
                    if let Some(meta) = crate::AppRegistry::descriptor(active_app_id) {
                        sb_cfg.title = Some(meta.name);
                    }
                }
                ctx.draw_status_bar_cfg(canvas, &sb_cfg);
            }
        }

        // Render OS modal overlay on top of screen if active
        if has_modal {
            modal_ref.render_overlay(canvas);
            #[cfg(feature = "text_input")]
            if matches!(modal_ref.modal, ActiveModal::TextInput(_)) {
                ctx.draw_status_bar_right_mode(canvas, true, true);
            }
        }

        // Render Keypad Lock Toast Modal overlay on top of screen if active
        lock_mgr.render_overlay(canvas);
    }
}
