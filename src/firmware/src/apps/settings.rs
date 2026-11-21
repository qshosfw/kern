//! System Configuration & Settings Application
//!
//! When `resident_settings` is enabled (or in host unit tests), includes the full
//! multi-page resident settings tree. In target release builds without `resident_settings`,
//! delegates to `/apps/settings.krx` (`packages/core/settings`) and provides a compact
//! ROM Safe-Mode Settings & Storage Recovery fallback if `/apps/settings.krx` is absent.

#[cfg(any(test, feature = "resident_settings"))]
pub mod app;
#[cfg(all(any(test, feature = "resident_settings"), feature = "filesystem"))]
pub mod app_prefs;
#[cfg(any(test, feature = "resident_settings"))]
pub mod apps;
#[cfg(all(any(test, feature = "resident_settings"), feature = "filesystem"))]
pub mod asset_chooser;
#[cfg(any(test, feature = "resident_settings"))]
pub mod audio;
#[cfg(any(test, feature = "resident_settings"))]
pub mod battery;
#[cfg(any(test, feature = "resident_settings"))]
pub mod buttons;
#[cfg(any(test, feature = "resident_settings"))]
pub mod device;
#[cfg(any(test, feature = "resident_settings"))]
pub mod dialog;
#[cfg(any(test, feature = "resident_settings"))]
pub mod display;
#[cfg(any(test, feature = "resident_settings"))]
pub mod metric;
#[cfg(any(test, feature = "resident_settings"))]
pub mod nav;
#[cfg(any(test, feature = "resident_settings"))]
pub mod options;
#[cfg(any(test, feature = "resident_settings"))]
pub mod page;
#[cfg(all(any(test, feature = "resident_settings"), feature = "passcode"))]
pub mod security;
#[cfg(any(test, feature = "resident_settings"))]
pub mod system;

#[cfg(any(test, feature = "resident_settings"))]
pub use app::SettingsApp;
#[cfg(any(test, feature = "resident_settings"))]
pub use page::*;

#[cfg(not(any(test, feature = "resident_settings")))]
pub mod apps {
    #[inline(always)]
    fn resolve_icon_id(icon_id: u16) -> u8 {
        u8::try_from(icon_id).unwrap_or(::abi::icon::APPS)
    }

    #[inline(never)]
    pub fn package_icon_8(_title: &str, icon_id: u16) -> &'static [u8; 8] {
        gfx::icons::system_icon(resolve_icon_id(icon_id)).unwrap_or(gfx::icons::ICON_APPS)
    }
}

#[cfg(not(any(test, feature = "resident_settings")))]
pub mod dialog {
    pub const TAG_POPUP_INFO: u8 = 7;
}

#[cfg(not(any(test, feature = "resident_settings")))]
pub mod app {
    use crate::SysContext;
    use gfx::{Canvas, Color};
    use kernel::app::{App, EventResult, ModalResult};
    use kernel::event::{Event, KeyCode, KeyState};

    const TAG_MISSING_SETTINGS: u8 = 19;
    const TAG_RESET_SETTINGS: u8 = 20;
    const TAG_REBOOT_FW: u8 = 21;

    /// Compact ROM Missing-Settings Dialog & Safe-Mode Recovery Fallback.
    ///
    /// When `/apps/settings.krx` is installed, `Orchestrator` and `Launcher` launch it directly.
    /// This lightweight ROM fallback is entered only when `/apps/settings.krx` is missing:
    /// it immediately prompts the user to install `settings.krx` via Dock or RigWire, and offers
    /// an optional Safe Recovery screen (`Reset Settings`, `Restart Radio`).
    #[repr(C)]
    pub struct SettingsApp {
        pub(crate) title: &'static str,
        pub(crate) sel: u8,
        pub(crate) recovery_mode: bool,
    }

    utils::static_bytes!(
        static RECOVERY_LABELS = b"Reset Settings\0Restart Radio\0";
    );

    #[inline(always)]
    pub fn is_action_module_active() -> bool {
        false
    }

    impl SettingsApp {
        pub unsafe fn init_at(ptr: *mut Self) {
            unsafe {
                (*ptr).title = "Settings";
                (*ptr).sel = 0;
                (*ptr).recovery_mode = false;
            }
        }

        #[inline(always)]
        pub fn is_animating(&self) -> bool {
            false
        }
    }

    impl App<SysContext<'_, '_>, Canvas> for SettingsApp {
        #[inline(never)]
        fn on_start(&mut self, ctx: &mut SysContext) {
            self.title = "Settings";
            self.sel = 0;
            self.recovery_mode = false;
            ctx.show_confirm(
                TAG_MISSING_SETTINGS,
                "Settings Missing",
                "Install settings.krx via Dock or RigWire.\nOpen Safe Recovery?",
                "Recovery",
                "Close",
            );
        }

        #[inline(never)]
        fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
            if !self.recovery_mode {
                return EventResult::Ignored;
            }
            let code = match *event {
                Event::Key {
                    code,
                    state: KeyState::Pressed,
                } => code,
                Event::Encoder(d) if d != 0 => {
                    if d < 0 {
                        KeyCode::Up
                    } else {
                        KeyCode::Down
                    }
                }
                _ => return EventResult::Ignored,
            };

            match code {
                KeyCode::Up | KeyCode::Key2 | KeyCode::Down | KeyCode::Key8 => {
                    self.sel ^= 1;
                    EventResult::Consumed
                }
                KeyCode::Menu | KeyCode::Key5 => {
                    if self.sel == 0 {
                        ctx.show_hold_confirm(
                            TAG_RESET_SETTINGS,
                            "Reset Settings?",
                            "Restore factory defaults?",
                            "Hold",
                            "Cancel",
                            1200,
                        );
                    } else {
                        ctx.show_hold_confirm(
                            TAG_REBOOT_FW,
                            "Restart Radio?",
                            "Save & restart now?",
                            "Hold",
                            "Cancel",
                            800,
                        );
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Ignored,
            }
        }

        fn on_modal_result(
            &mut self,
            ctx: &mut SysContext,
            result: &ModalResult<'_>,
        ) -> EventResult {
            match *result {
                ModalResult::Confirmed(TAG_MISSING_SETTINGS) => {
                    self.recovery_mode = true;
                    self.title = "Safe Recovery";
                    self.sel = 0;
                    EventResult::Consumed
                }
                ModalResult::Canceled(TAG_MISSING_SETTINGS) => {
                    ctx.post_event(crate::apps::Shortcut::CloseApp.into());
                    EventResult::Consumed
                }
                ModalResult::Confirmed(TAG_RESET_SETTINGS) => {
                    unsafe {
                        crate::config::MasterConfig::init_in_place(ctx.config);
                    }
                    ctx.config.sanitize();
                    ctx.config.hw_dirty = 0x3F;
                    #[cfg(feature = "filesystem")]
                    let _ = ctx.config.save_all(ctx.storage);
                    ctx.show_popup(0, "Safe Recovery", "Defaults restored.");
                    EventResult::Consumed
                }
                ModalResult::Confirmed(TAG_REBOOT_FW) => {
                    let _ = ctx.flush_config();
                    kernel::power::PowerControl::reboot();
                }
                _ => EventResult::Ignored,
            }
        }

        #[inline(never)]
        fn render(&self, canvas: &mut Canvas, _ctx: &SysContext) {
            if !self.recovery_mode {
                return;
            }
            for idx in 0..2usize {
                let y = (11 + idx * 14) as u8;
                let fg = if idx == self.sel as usize {
                    gfx::widgets::draw_selection_capsule(
                        canvas,
                        [2, y, 124, 13],
                        gfx::widgets::CAPSULE_FILL_ROUND,
                    )
                } else {
                    Color::Fill
                };
                let label = utils::format::nth_cstr(&RECOVERY_LABELS, idx as u8);
                gfx::text::draw_str(
                    canvas,
                    6,
                    y as i16 + 9,
                    label,
                    &gfx::fonts::mumatrix_regular::FONT,
                    fg,
                );
            }
            gfx::text::draw_str_align(
                canvas,
                64,
                56,
                "Install /apps/settings.krx",
                &gfx::fonts::tomthumb::FONT,
                Color::Fill,
                gfx::text::Align::Center,
            );
        }

        fn on_exit(&mut self, ctx: &mut SysContext) {
            if ctx.config.is_dirty() {
                let _ = ctx.flush_config();
            }
        }
    }
}

#[cfg(not(any(test, feature = "resident_settings")))]
pub use app::SettingsApp;
