//! OS UI Compositor
//! Automatically composites application graphics workspace, status bar overlays, and system modal dialogs.

use gfx::Canvas;
#[cfg(feature = "text_input")]
use gfx::views::{Charset, TextPromptState, TextPromptView};
use gfx::views::{
    DialogState, DialogView, KeyRepeater, MarqueeScroller, OptionSheet, SheetResult, SheetView,
    ShortString, ToastBanner, ToastState, TransitionProgress, View,
};
use kernel::app::ModalResult;
use kernel::event::{Event, KeyCode, KeyState};

use super::keypad_lock::KeypadLockManager;

#[repr(u8)]
pub enum ActiveModal {
    None = 0,
    Dialog(DialogState) = 1,
    #[cfg(feature = "text_input")]
    TextInput(TextPromptState) = 2,
    Sheet(
        core::cell::Cell<u8>,
        TransitionProgress,
        MarqueeScroller,
        KeyRepeater,
        OptionSheet,
    ) = 3,
}

#[repr(C)]
pub struct ModalCompositor {
    pub tag: u8,
    pub last_hold_ms: u32,
    pub modal: ActiveModal,
    pub toast: ToastState,
}

impl ModalCompositor {
    pub const fn new() -> Self {
        Self {
            tag: 0,
            last_hold_ms: 0,
            modal: ActiveModal::None,
            toast: ToastState::new(),
        }
    }

    #[inline(always)]
    pub fn is_active(&self) -> bool {
        !matches!(self.modal, ActiveModal::None) || self.toast.is_active
    }

    pub fn is_animating(&self) -> bool {
        match &self.modal {
            ActiveModal::Sheet(_, anim, marquee, _, _) => {
                !anim.is_complete()
                    || (crate::master_config_ref().display.animations_enabled()
                        && marquee.is_active())
            }
            ActiveModal::Dialog(model) => !model.anim.is_complete() || model.is_hold_animating(),
            _ => false,
        }
    }

    pub fn show_toast(&mut self, msg: &str, duration_ms: u16) {
        self.toast.show(msg, duration_ms);
    }

    /// Advance non-blocking toast expiry timer; returns `true` when an active toast expires.
    pub fn tick(&mut self, dt_ms: u16) -> bool {
        let was_active = self.toast.is_active;
        self.toast.tick(dt_ms);
        was_active && !self.toast.is_active
    }

    #[inline(always)]
    pub fn close(&mut self) {
        self.modal = ActiveModal::None;
    }

    unsafe fn reset_slot(&mut self, tag: u8, disc: u8) -> *mut u8 {
        crate::orchestrator::sink_enter_keys();
        self.tag = tag;
        self.last_hold_ms = 0;
        let ptr = (&mut self.modal as *mut ActiveModal).cast::<u8>();
        unsafe {
            core::ptr::write_bytes(ptr, 0, core::mem::size_of::<ActiveModal>());
            *ptr = disc;
        }
        ptr
    }

    pub fn show_confirm(
        &mut self,
        tag: u8,
        title: &str,
        message: &str,
        confirm: &'static str,
        cancel: &'static str,
    ) {
        unsafe { self.reset_slot(tag, 1) };
        if let ActiveModal::Dialog(model) = &mut self.modal {
            model.anim = TransitionProgress::new(4);
            model.repeater = KeyRepeater::new(8, 2);
            if !confirm.is_empty() {
                model.confirm_label = Some(confirm);
            }
            if !cancel.is_empty() {
                model.cancel_label = Some(cancel);
            }
            model.custom_title.set(title);
            wrap_alert_inline(message, &mut model.custom_msg);
        }
    }

    pub fn set_hold_ms(&mut self, hold_ms: u16) {
        crate::orchestrator::clear_sunk_keys();
        if let ActiveModal::Dialog(model) = &mut self.modal {
            model.hold.required_ms = hold_ms;
        }
    }

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
    ) -> &mut TextPromptState {
        unsafe { self.reset_slot(tag, 2) };
        let ActiveModal::TextInput(model) = &mut self.modal else {
            unreachable!()
        };
        model.is_k1 = true;
        model.max_len = max_len;
        model.hold_ticks = 24;
        model.custom_title.set(title);
        model.is_passcode = is_passcode;
        model.digits_only = digits_only;
        model.show_header = true;
        if digits_only {
            model.charset = Charset::Numbers;
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
        #[cfg(all(feature = "filesystem", not(test)))]
        if !digits_only {
            crate::services::lang::load_locale_keymap(crate::kern_fs(), &mut m.locale_keymap);
        }
    }

    #[cfg(feature = "text_input")]
    pub fn prompt_passcode(
        &mut self,
        tag: u8,
        title: &str,
        max_len: usize,
        digits_only: bool,
        expose_len: bool,
    ) -> &mut TextPromptState {
        let m = self.init_text_prompt(tag, title, max_len, digits_only, true);
        m.expose_len = expose_len;
        m
    }

    pub fn show_sheet(&mut self, tag: u8, sheet: OptionSheet) {
        unsafe { self.reset_slot(tag, 3) };
        if let ActiveModal::Sheet(_, anim, _, rep, slot_sheet) = &mut self.modal {
            let cfg = crate::master_config_ref();
            *anim = if cfg.display.animations_enabled() {
                TransitionProgress::new(4)
            } else {
                TransitionProgress::completed(4)
            };
            *rep = KeyRepeater::new(8, 2);
            rep.configure(
                cfg.keys.hold_delay_ms(),
                cfg.keys.repeat_rate_ms(),
                cfg.keys.fast_scroll != 0,
            );
            *slot_sheet = sheet;
        }
    }

    pub fn handle_event(&mut self, event: &Event) -> Option<ModalResult<'_>> {
        let tag = self.tag;
        match &mut self.modal {
            ActiveModal::None => None,
            ActiveModal::Dialog(model) => {
                if DialogView.on_event(model, event) {
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
                        KeyCode::Menu | KeyCode::Key5 => Some(ModalResult::Confirmed(tag)),
                        KeyCode::Exit => Some(ModalResult::Dismissed(tag)),
                        _ => None,
                    }
                } else {
                    None
                }
            }
            #[cfg(feature = "text_input")]
            ActiveModal::TextInput(model) => {
                TextPromptView::new().on_event(model, event);
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
            ActiveModal::Sheet(hover_ov, anim, marquee, repeater, sheet) => {
                let prev_hover = sheet.hover;
                if let Event::Tick = event {
                    anim.tick();
                    if crate::master_config_ref().display.animations_enabled() {
                        marquee.set_overflow(hover_ov.get());
                        marquee.tick();
                    } else if marquee.is_active() {
                        marquee.set_overflow(0);
                    }
                }
                let res = SheetView::handle_event(repeater, sheet, event);
                if sheet.hover != prev_hover {
                    marquee.set_overflow(0);
                    hover_ov.set(0);
                }
                match res {
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
            ActiveModal::Dialog(model) => {
                DialogView.render(canvas, model);
            }
            #[cfg(feature = "text_input")]
            ActiveModal::TextInput(model) => {
                TextPromptView::new().render(canvas, model);
            }
            ActiveModal::Sheet(hover_ov, anim, marquee, _, sheet) => {
                let animated = crate::master_config_ref().display.animations_enabled();
                let ov = SheetView::render_slide_up_marquee(
                    canvas,
                    sheet,
                    anim.slide_offset(48),
                    marquee.offset(),
                    animated,
                );
                hover_ov.set(ov);
            }
        }
        if self.toast.is_active {
            ToastBanner.render(canvas, &self.toast);
        }
    }
}

pub static mut ACTIVE_MODAL: ModalCompositor = ModalCompositor::new();

pub fn active_modal_mut() -> &'static mut ModalCompositor {
    unsafe { &mut *core::ptr::addr_of_mut!(ACTIVE_MODAL) }
}

pub const TAG_LOW_BATTERY_POPUP: u8 = 0xFE;
pub const TAG_SYSTEM_ALERT: u8 = 0xFD;

const MAX_ALERT_LINE_PX: i16 = 112;

#[derive(Clone, Copy)]
pub struct SystemAlert {
    pub title: utils::format::InlineStr<18>,
    pub message: utils::format::InlineStr<88>,
}

impl SystemAlert {
    pub const fn empty() -> Self {
        Self {
            title: utils::format::InlineStr::new(),
            message: utils::format::InlineStr::new(),
        }
    }
}

struct AlertQueue {
    has_alert: bool,
    slot: SystemAlert,
}

impl AlertQueue {
    const fn new() -> Self {
        Self {
            has_alert: false,
            slot: SystemAlert::empty(),
        }
    }

    fn push_alert(&mut self, title: &str, message: &str, overwrite: bool) {
        if self.has_alert && !overwrite {
            return;
        }
        self.has_alert = true;
        self.slot.title.set(title);
        wrap_alert_inline(message, &mut self.slot.message);
    }

    fn peek(&self) -> Option<&SystemAlert> {
        self.has_alert.then_some(&self.slot)
    }

    fn discard_head(&mut self) {
        self.has_alert = false;
    }

    #[cfg(test)]
    fn pop(&mut self) -> Option<SystemAlert> {
        let alert = self.peek().copied()?;
        self.discard_head();
        Some(alert)
    }

    #[cfg(test)]
    fn clear(&mut self) {
        self.has_alert = false;
    }
}

static mut SYSTEM_ALERTS: AlertQueue = AlertQueue::new();

fn wrap_alert_bytes(raw: &str, dst: &mut [u8]) -> usize {
    const SPACE_PX: i16 = 4; // TomThumb ' ' glyph x_advance
    let b = raw.as_bytes();
    let mut i = 0usize;
    let mut pos = 0usize;
    let mut line_px: i16 = 0;

    while i < b.len() {
        let ch = b[i];
        if ch == b'\n' || ch == b' ' {
            if ch == b'\n' {
                if pos >= dst.len() {
                    return pos;
                }
                dst[pos] = b'\n';
                pos += 1;
                line_px = 0;
            }
            i += 1;
            continue;
        }
        let s = i;
        while i < b.len() && b[i] != b' ' && b[i] != b'\n' {
            i += 1;
        }
        let word_bytes = &b[s..i];
        // SAFETY: `s` and `i` land on ASCII space/newline or slice bounds of valid UTF-8 `raw`.
        let word = unsafe { core::str::from_utf8_unchecked(word_bytes) };
        let word_px = gfx::text::str_width(word, &gfx::fonts::tomthumb::FONT);
        if line_px > 0 {
            if pos >= dst.len() {
                return pos;
            }
            let sep = if line_px + SPACE_PX + word_px > MAX_ALERT_LINE_PX {
                line_px = 0;
                b'\n'
            } else {
                line_px += SPACE_PX;
                b' '
            };
            dst[pos] = sep;
            pos += 1;
        }
        if pos + word_bytes.len() > dst.len() {
            return pos;
        }
        dst[pos..pos + word_bytes.len()].copy_from_slice(word_bytes);
        pos += word_bytes.len();
        line_px += word_px;
    }
    pos
}

/// Non-generic word-wrapper directly into `InlineStr<88>`.
fn wrap_alert_inline(raw: &str, out: &mut utils::format::InlineStr<88>) {
    out.len = wrap_alert_bytes(raw, &mut out.buf) as u8;
}

/// Word-wraps `raw` for TomThumb rendering inside the modal/sheet viewport.
#[cfg(test)]
pub fn wrap_alert_text<const N: usize>(raw: &str, out: &mut heapless::String<N>) {
    let vec = unsafe { out.as_mut_vec() };
    vec.clear();
    let _ = vec.resize_default(N);
    let len = wrap_alert_bytes(raw, vec.as_mut_slice());
    vec.truncate(len);
}

/// Queues a persistent notification for presentation once the Launcher is active.
pub fn queue_system_alert(title: &str, message: &str) {
    unsafe {
        (*core::ptr::addr_of_mut!(SYSTEM_ALERTS)).push_alert(title, message, true);
    }
}

/// Queues a low-priority onboarding/welcome notice only when no alert is already pending.
pub fn queue_system_alert_if_empty(title: &str, message: &str) {
    unsafe {
        (*core::ptr::addr_of_mut!(SYSTEM_ALERTS)).push_alert(title, message, false);
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

/// Presents the next queued system notification as a modal dialog.
pub fn present_next_system_alert() -> bool {
    let modal = active_modal_mut();
    if modal.is_active() {
        return false;
    }
    let q = unsafe { &mut *core::ptr::addr_of_mut!(SYSTEM_ALERTS) };
    let Some(alert) = q.peek() else {
        return false;
    };
    crate::services::play_dialog_sound(crate::services::melody::DialogSoundType::Prompt);
    crate::orchestrator::sink_exit_key();
    modal.show_confirm(
        TAG_SYSTEM_ALERT,
        alert.title.as_str(),
        alert.message.as_str(),
        "OK",
        "",
    );
    q.discard_head();
    true
}

/// Displays an immediate non-blocking Low Battery alert modal on top of active screen.
pub fn show_low_battery_popup() {
    active_modal_mut().show_confirm(
        TAG_LOW_BATTERY_POPUP,
        "Low Battery",
        "Charge radio soon.",
        "OK",
        "",
    );
}

pub struct OsCompositor;

impl OsCompositor {
    fn render_status_bar(
        active_app_id: usize,
        canvas: &mut Canvas,
        sb_cfg_in: kernel::app::StatusBarConfig<'_>,
    ) {
        let mut native_left_icons =
            [kernel::ui::StatusGlyph::Builtin(kernel::ui::Icon::Antenna); 4];
        let mut native_center_icons =
            [kernel::ui::StatusGlyph::Builtin(kernel::ui::Icon::Antenna); 4];
        let mut sb_cfg = sb_cfg_in;
        #[cfg(feature = "krx_native")]
        if active_app_id >= crate::apps::AppRegistry::count() {
            crate::runtime::native_runner::apply_slot_status_icons(
                &mut sb_cfg,
                &mut native_left_icons,
                &mut native_center_icons,
            );
        }
        if !sb_cfg.partial && sb_cfg.title.is_none() {
            if let Some(meta) = crate::AppRegistry::descriptor(active_app_id) {
                sb_cfg.title = Some(meta.name);
            }
        }
        crate::services::draw_status_bar_cfg(canvas, &sb_cfg);
    }

    pub fn render(active_app_id: usize, canvas: &mut Canvas, lock_mgr: &KeypadLockManager) {
        let modal_ref = active_modal_mut();
        let has_modal = modal_ref.is_active();
        let has_blocking_modal = !matches!(modal_ref.modal, ActiveModal::None);
        #[cfg(feature = "text_input")]
        let is_text_input = matches!(modal_ref.modal, ActiveModal::TextInput(_));
        #[cfg(not(feature = "text_input"))]
        let is_text_input = false;

        canvas.clear();

        let mut sb_cfg = kernel::app::StatusBarConfig::default();
        crate::AppRegistry::render(active_app_id, canvas, &mut sb_cfg);
        let sb_visible = sb_cfg.visible;

        if sb_visible && !is_text_input {
            Self::render_status_bar(active_app_id, canvas, sb_cfg);
        }

        // Render OS modal overlay on top of screen if active
        if has_modal {
            if has_blocking_modal && !is_text_input {
                canvas.dim_backdrop(if sb_visible { 9 } else { 0 });
            }
            modal_ref.render_overlay(canvas);
            #[cfg(feature = "text_input")]
            if is_text_input {
                crate::services::draw_status_bar_right_mode(canvas, true, true);
            }
        } else {
            let cfg = crate::services::config();
            if lock_mgr.should_show_f_drawer() && cfg.keys.f_drawer != 0 {
                let y_offset = if cfg.display.animations_enabled() {
                    lock_mgr.f_drawer_y_offset(40)
                } else {
                    0
                };
                render_f_shortcut_drawer(canvas, &cfg.keys, active_app_id, y_offset);
            }
        }

        // Render Keypad Lock Toast Modal overlay on top of screen if active
        lock_mgr.render_overlay(canvas);
    }
}

/// Slide-up `[F]` key shortcut quick-glance HUD: 2x3 grid of `[key]` inverted badges + 8x8 icons.
fn render_f_shortcut_drawer(
    canvas: &mut Canvas,
    keys: &crate::config::keys::KeyConfig,
    active_app_id: usize,
    y_offset: i8,
) {
    use crate::apps::shortcut::{
        F_DRAWER_START_SLOT, KEY_SHORTCUT_COUNT, TARGET_NONE, active_context, describe_into,
        drawer_badge, resolve_slot,
    };

    const DRAWER_H: u8 = 40;
    let sheet_y = gfx::elements::draw_sheet_slide_up(canvas, "F Shortcuts", DRAWER_H, y_offset);
    if sheet_y >= 64 {
        return;
    }

    let cat = crate::apps::launcher::catalog::get_catalog();
    let cur_ctx = active_context(active_app_id);
    let mut shown = 0usize;
    let base_y = sheet_y + 10;
    let mut label = ShortString::empty();

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
        if cell_y >= 64 {
            break;
        }

        let box_w = gfx::elements::draw_key_badge_box(
            canvas,
            cell_x,
            cell_y + 1,
            drawer_badge(slot),
            false,
        );
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
        let msg_y = base_y + 16;
        if msg_y < 64 {
            gfx::text::draw_str(
                canvas,
                18,
                msg_y,
                "No F-key shortcuts",
                &gfx::fonts::tomthumb::FONT,
                gfx::Color::Fill,
            );
        }
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
        queue_system_alert("Storage Protected", "Updated first copy");

        let a1 = pop_system_alert().unwrap();
        assert_eq!(a1.title.as_str(), "Storage Protected");
        assert_eq!(a1.message.as_str(), "Updated first copy");
        assert!(pop_system_alert().is_none());

        // Pushing beyond capacity evicts oldest slot cleanly
        queue_system_alert("First", "1");
        queue_system_alert("Second", "2");
        assert_eq!(pop_system_alert().unwrap().title.as_str(), "Second");
        assert!(pop_system_alert().is_none());

        // Low-priority welcome notice never overwrites a pending storage/calib warning
        queue_system_alert("Default Calib", "Factory calib missing.");
        queue_system_alert_if_empty("Welcome to KERN", "Sync apps in Kern Dock.");
        assert_eq!(pop_system_alert().unwrap().title.as_str(), "Default Calib");
    }

    #[test]
    fn show_confirm_auto_wraps_long_lines_and_accepts_key5() {
        let mut compositor = ModalCompositor::new();
        compositor.show_confirm(
            7,
            "Settings App",
            "Package checksum mismatch detected during launch verification.",
            "OK",
            "",
        );
        if let ActiveModal::Dialog(model) = &compositor.modal {
            assert!(gfx::text::count_lines(model.custom_msg.as_str()) >= 2);
            for line in gfx::text::lines(model.custom_msg.as_str()) {
                assert!(
                    gfx::text::str_width(line, &gfx::fonts::tomthumb::FONT) <= MAX_ALERT_LINE_PX
                );
            }
        } else {
            panic!("expected popup modal");
        }
        assert!(matches!(
            compositor.handle_event(&Event::Key {
                code: KeyCode::Key5,
                state: KeyState::Pressed,
            }),
            Some(ModalResult::Confirmed(7))
        ));
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
            ActiveModal::Dialog(model) => {
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
