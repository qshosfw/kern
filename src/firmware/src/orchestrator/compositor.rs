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

#[repr(C)]
pub struct ModalCompositor {
    pub tag: u8,
    pub last_hold_ms: u32,
    pub modal: ActiveModal,
    pub toast: ToastModel,
    pub toast_view: ToastView,
    pub popup_view: PopupView,
    #[cfg(feature = "text_input")]
    pub text_view: TextInputView,
}

impl ModalCompositor {
    pub const fn new() -> Self {
        Self {
            tag: 0,
            last_hold_ms: 0,
            modal: ActiveModal::None,
            toast: ToastModel::new(),
            toast_view: ToastView::new(),
            popup_view: PopupView::new(),
            #[cfg(feature = "text_input")]
            text_view: TextInputView::new(),
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        !matches!(self.modal, ActiveModal::None) || self.toast.is_active
    }

    #[inline(never)]
    pub fn is_animating(&self) -> bool {
        if self.toast.is_active {
            return true;
        }
        match &self.modal {
            ActiveModal::Sheet(_, _, anim) => anim.is_animating(),
            ActiveModal::Popup(model) => !model.anim.is_complete() || model.is_hold_animating(),
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
        self.last_hold_ms = 0;
        self.modal = ActiveModal::Popup(PopupModel::new());
        if let ActiveModal::Popup(model) = &mut self.modal {
            if !confirm.is_empty() {
                model.confirm_label = Some(confirm);
            }
            if !cancel.is_empty() {
                model.cancel_label = Some(cancel);
            }
            let _ = model.custom_title.push_str(title);
            let _ = model.custom_msg.push_str(message);
        }
    }

    #[inline(never)]
    pub fn set_hold_ms(&mut self, hold_ms: u16) {
        crate::orchestrator::clear_sunk_keys();
        if let ActiveModal::Popup(model) = &mut self.modal {
            model.hold.required_ms = hold_ms;
        }
    }

    #[inline(never)]
    pub fn show_hold_confirm(
        &mut self,
        tag: u8,
        title: &str,
        message: &str,
        confirm: &'static str,
        cancel: &'static str,
        hold_ms: u16,
    ) {
        self.show_confirm(tag, title, message, confirm, cancel);
        self.set_hold_ms(hold_ms);
    }

    #[inline(never)]
    pub fn show_popup(&mut self, tag: u8, title: &str, message: &str) {
        self.show_confirm(tag, title, message, "", "");
    }

    #[cfg(feature = "text_input")]
    #[inline(never)]
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
        self.modal = ActiveModal::TextInput(TextInputModel::new(max_len));
        let ActiveModal::TextInput(model) = &mut self.modal else {
            unreachable!()
        };
        let _ = model.custom_title.push_str(title);
        model.is_passcode = is_passcode;
        model.digits_only = digits_only;
        model.show_header = true;
        if digits_only {
            model.charset = gfx::gui::modules::text_input::Charset::Numbers;
        }
        model
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
                    if matches!(event, Event::Tick) && model.is_hold_animating() {
                        let dt = super::tick_dt_ms(&mut self.last_hold_ms, 250);
                        if model.advance_hold(dt) {
                            return Some(ModalResult::Confirmed(tag));
                        }
                    } else if !model.is_hold_animating() {
                        self.last_hold_ms = 0;
                    }
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
pub const TAG_SYSTEM_ALERT: u8 = 0xFD;

const MAX_SYSTEM_ALERTS: usize = 2;
const MAX_ALERT_LINE_PX: i16 = 116;

#[derive(Clone)]
pub struct SystemAlert {
    pub title: heapless::String<24>,
    pub message: heapless::String<120>,
}

impl SystemAlert {
    pub const fn empty() -> Self {
        Self {
            title: heapless::String::new(),
            message: heapless::String::new(),
        }
    }
}

struct AlertQueue {
    slots: [SystemAlert; MAX_SYSTEM_ALERTS],
    head: usize,
    len: usize,
}

impl AlertQueue {
    const fn new() -> Self {
        Self {
            slots: [const { SystemAlert::empty() }; MAX_SYSTEM_ALERTS],
            head: 0,
            len: 0,
        }
    }

    fn push_alert(&mut self, title: &str, message: &str) {
        let mut target = None;
        for i in 0..self.len {
            let idx = (self.head + i) & (MAX_SYSTEM_ALERTS - 1);
            if self.slots[idx].title.as_str() == title {
                target = Some(idx);
                break;
            }
        }
        let idx = match target {
            Some(i) => i,
            None if self.len == MAX_SYSTEM_ALERTS => {
                let i = self.head;
                self.head = (self.head + 1) & (MAX_SYSTEM_ALERTS - 1);
                i
            }
            None => {
                let i = (self.head + self.len) & (MAX_SYSTEM_ALERTS - 1);
                self.len += 1;
                i
            }
        };
        let slot = &mut self.slots[idx];
        slot.title.clear();
        let _ = slot.title.push_str(title);
        wrap_alert_text(message, &mut slot.message);
    }

    fn peek(&self) -> Option<&SystemAlert> {
        if self.len == 0 {
            None
        } else {
            Some(&self.slots[self.head])
        }
    }

    fn discard_head(&mut self) {
        if self.len > 0 {
            self.head = (self.head + 1) & (MAX_SYSTEM_ALERTS - 1);
            self.len -= 1;
        }
    }

    #[cfg(test)]
    fn pop(&mut self) -> Option<SystemAlert> {
        let alert = self.peek().cloned()?;
        self.discard_head();
        Some(alert)
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

static mut SYSTEM_ALERTS: AlertQueue = AlertQueue::new();

/// Word-wraps `raw` for TomThumb rendering inside the 116 px bottom-sheet viewport.
#[inline(never)]
pub fn wrap_alert_text(raw: &str, out: &mut heapless::String<120>) {
    out.clear();
    let space_px = gfx::text::str_width(" ", &gfx::fonts::tomthumb::FONT);
    let b = raw.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    let mut line_px: i16 = 0;

    while i < n {
        if b[i] == b'\n' {
            if out.push('\n').is_err() {
                return;
            }
            line_px = 0;
            i += 1;
            continue;
        }
        if b[i] == b' ' {
            i += 1;
            continue;
        }
        let s = i;
        while i < n && b[i] != b' ' && b[i] != b'\n' {
            i += 1;
        }
        let word = &raw[s..i];
        let word_px = gfx::text::str_width(word, &gfx::fonts::tomthumb::FONT);
        if line_px > 0 {
            let sep = if line_px + space_px + word_px > MAX_ALERT_LINE_PX {
                line_px = 0;
                '\n'
            } else {
                line_px += space_px;
                ' '
            };
            if out.push(sep).is_err() {
                return;
            }
        }
        if out.push_str(word).is_err() {
            return;
        }
        line_px += word_px;
    }
}

/// Queues a persistent notification for presentation once the Launcher is active.
#[inline(never)]
pub fn queue_system_alert(title: &str, message: &str) {
    unsafe {
        (*core::ptr::addr_of_mut!(SYSTEM_ALERTS)).push_alert(title, message);
    }
}

#[cfg(test)]
pub fn pop_system_alert() -> Option<SystemAlert> {
    unsafe { (*core::ptr::addr_of_mut!(SYSTEM_ALERTS)).pop() }
}

#[cfg(test)]
pub fn clear_system_alerts() {
    unsafe {
        (*core::ptr::addr_of_mut!(SYSTEM_ALERTS)).clear();
    }
}

/// Presents the next queued system notification as a persistent modal bottom sheet.
pub fn present_next_system_alert(ctx: &mut SysContext) -> bool {
    let modal_active = unsafe { (*core::ptr::addr_of!(ACTIVE_MODAL)).is_active() };
    if modal_active {
        return false;
    }
    let q = unsafe { &mut *core::ptr::addr_of_mut!(SYSTEM_ALERTS) };
    let Some(alert) = q.peek() else {
        return false;
    };
    ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Prompt);
    crate::orchestrator::sink_exit_key();
    unsafe {
        (*core::ptr::addr_of_mut!(ACTIVE_MODAL)).show_confirm(
            TAG_SYSTEM_ALERT,
            alert.title.as_str(),
            alert.message.as_str(),
            "OK",
            "",
        );
    }
    q.discard_head();
    true
}

/// Displays an immediate non-blocking Low Battery alert modal on top of active screen.
pub fn show_low_battery_popup() {
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
        #[cfg(feature = "text_input")]
        let is_text_input = has_modal && matches!(modal_ref.modal, ActiveModal::TextInput(_));
        #[cfg(not(feature = "text_input"))]
        let is_text_input = false;

        if sb_cfg.visible && !is_text_input {
            if !sb_cfg.partial && sb_cfg.title.is_none() {
                if let Some(meta) = crate::AppRegistry::descriptor(active_app_id) {
                    sb_cfg.title = Some(meta.name);
                }
            }
            ctx.draw_status_bar_cfg(canvas, &sb_cfg);
        }

        // Render OS modal overlay on top of screen if active
        if has_modal {
            modal_ref.render_overlay(canvas);
            #[cfg(feature = "text_input")]
            if is_text_input {
                ctx.draw_status_bar_right_mode(canvas, true, true);
            }
        } else if lock_mgr.should_show_f_drawer() && ctx.config.keys.f_drawer != 0 {
            render_f_shortcut_drawer(canvas, &ctx.config.keys, active_app_id);
        }

        // Render Keypad Lock Toast Modal overlay on top of screen if active
        lock_mgr.render_overlay(canvas);
    }
}

/// Slide-up `[F]` key shortcut quick-glance HUD: 2x3 grid of `[key]` inverted badges + 8x8 icons.
#[inline(never)]
fn render_f_shortcut_drawer(
    canvas: &mut Canvas,
    keys: &crate::config::keys::KeyConfig,
    active_app_id: usize,
) {
    use crate::apps::shortcut::{
        F_DRAWER_START_SLOT, KEY_SHORTCUT_COUNT, TARGET_NONE, active_context, describe_into,
        drawer_badge, resolve_slot,
    };

    const DRAWER_H: u8 = 40;
    gfx::widgets::draw_sheet_slide_up(canvas, "F Shortcuts", DRAWER_H, None, 0);

    let cat = crate::apps::launcher::catalog::get_catalog();
    let cur_ctx = active_context(active_app_id);
    let mut shown = 0usize;
    let base_y = (64 - DRAWER_H) as i16 + 10;
    let mut label = gfx::gui::modules::menu::ShortString::empty();

    for slot in F_DRAWER_START_SLOT..KEY_SHORTCUT_COUNT {
        let target_id = resolve_slot(keys, slot, cur_ctx);
        if target_id == TARGET_NONE {
            continue;
        }
        if shown >= 6 {
            break;
        }
        let col = shown & 1;
        let row = shown >> 1;
        let cell_x = if col == 0 { 4i16 } else { 65i16 };
        let cell_y = base_y + (row as i16 * 10);

        let box_w =
            gfx::widgets::draw_key_badge_box(canvas, cell_x, cell_y + 1, drawer_badge(slot), false);
        let icon = describe_into(target_id, cat, false, &mut label);
        let icon_x = cell_x + box_w as i16 + 2;
        gfx::icons::draw_icon_8(canvas, icon_x, cell_y, icon, gfx::Color::Fill);
        gfx::text::draw_str_max_x(
            canvas,
            icon_x + 9,
            cell_y + 6,
            label.as_str(),
            &gfx::fonts::tomthumb::FONT,
            gfx::Color::Fill,
            cell_x + 59,
        );
        shown += 1;
    }

    if shown == 0 {
        gfx::text::draw_str(
            canvas,
            18,
            base_y + 16,
            "No F-key shortcuts",
            &gfx::fonts::tomthumb::FONT,
            gfx::Color::Fill,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_alert_text_fits_within_bottom_sheet_width() {
        let mut out = heapless::String::<120>::new();
        wrap_alert_text(
            "A device passcode protects 3 encrypted files, and Passcode unlock is disabled in this firmware.",
            &mut out,
        );
        assert!(gfx::text::count_lines(out.as_str()) >= 2);
        for line in gfx::text::lines(out.as_str()) {
            let px = gfx::text::str_width(line, &gfx::fonts::tomthumb::FONT);
            assert!(
                px <= MAX_ALERT_LINE_PX,
                "line overflowed ({px} px): {line:?}"
            );
        }
    }

    #[test]
    fn system_alert_queue_dedups_and_preserves_order() {
        clear_system_alerts();
        queue_system_alert("Storage Protected", "First copy");
        queue_system_alert("Encrypted Storage", "Second alert");
        queue_system_alert("Storage Protected", "Updated first copy");

        let a1 = pop_system_alert().unwrap();
        assert_eq!(a1.title.as_str(), "Storage Protected");
        assert_eq!(a1.message.as_str(), "Updated first copy");

        let a2 = pop_system_alert().unwrap();
        assert_eq!(a2.title.as_str(), "Encrypted Storage");
        assert!(pop_system_alert().is_none());

        // Pushing beyond capacity evicts oldest slot cleanly
        queue_system_alert("First", "1");
        queue_system_alert("Second", "2");
        queue_system_alert("Third", "3");
        assert_eq!(pop_system_alert().unwrap().title.as_str(), "Second");
        assert_eq!(pop_system_alert().unwrap().title.as_str(), "Third");
        assert!(pop_system_alert().is_none());
    }

    #[test]
    fn modal_single_badge_mode_allows_ok_only() {
        let mut compositor = ModalCompositor::new();
        compositor.show_confirm(
            TAG_SYSTEM_ALERT,
            "Storage Protected",
            "Message body",
            "OK",
            "",
        );
        match &compositor.modal {
            ActiveModal::Popup(model) => {
                assert_eq!(model.confirm_label, Some("OK"));
                assert_eq!(model.cancel_label, None);
            }
            _ => panic!("expected popup modal"),
        }
    }

    #[test]
    fn hold_confirm_requires_sustained_hold_and_resets_on_release() {
        let mut compositor = ModalCompositor::new();
        compositor.show_hold_confirm(
            42,
            "Format Storage?",
            "Erase all files?",
            "Hold",
            "Cancel",
            1000,
        );
        assert!(
            compositor
                .handle_event(&Event::Key {
                    code: KeyCode::Menu,
                    state: KeyState::Pressed,
                })
                .is_none()
        );
        for _ in 0..5 {
            assert!(compositor.handle_event(&Event::Tick).is_none());
        }
        assert!(
            compositor
                .handle_event(&Event::Key {
                    code: KeyCode::Menu,
                    state: KeyState::Released,
                })
                .is_none()
        );
        // Smooth 3x decay rewinds progress back to 0 across subsequent ticks
        for _ in 0..3 {
            assert!(compositor.handle_event(&Event::Tick).is_none());
        }
        assert!(
            compositor
                .handle_event(&Event::Key {
                    code: KeyCode::Menu,
                    state: KeyState::Held,
                })
                .is_none()
        );
        let mut confirmed = false;
        for _ in 0..20 {
            if matches!(
                compositor.handle_event(&Event::Tick),
                Some(ModalResult::Confirmed(42))
            ) {
                confirmed = true;
                break;
            }
        }
        assert!(confirmed, "sustained Menu hold must confirm hold modal");
    }
}
