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

    const TAG_RESET_SETTINGS: u8 = 20;
    const TAG_REBOOT_FW: u8 = 21;

    /// Compact ROM Settings Launcher & Safe-Mode Recovery Fallback (`16 B` in `OS_APP_STORAGE`).
    /// Launches `/apps/settings.krx` when available, or provides essential display/audio/reset
    /// controls when storage is unmounted or `/apps/settings.krx` is not installed.
    #[repr(C)]
    pub struct SettingsApp {
        pub(crate) title: &'static str,
        pub(crate) sel: u8,
        pub(crate) editing: bool,
    }

    const FALLBACK_PROPS: [u8; 4] = [
        ::abi::prop::CONTRAST,
        ::abi::prop::BACKLIGHT_MAX,
        ::abi::prop::VOLUME,
        ::abi::prop::KEY_BEEP,
    ];

    utils::static_bytes!(
        static FALLBACK_LABELS = b"Contrast\0Brightness\0AF Volume\0Key Beep\0Reset Settings\0Restart Radio\0";
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
                (*ptr).editing = false;
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
            self.editing = false;
            #[cfg(all(feature = "filesystem", feature = "krx_native"))]
            let _ = crate::runtime::launch_krx_package(ctx, "/apps/settings.krx", 0);
        }

        #[inline(never)]
        fn on_event(&mut self, ctx: &mut SysContext, event: &Event) -> EventResult {
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

            if self.editing {
                let prop_id = FALLBACK_PROPS[(self.sel as usize).min(3)];
                match code {
                    KeyCode::Up | KeyCode::Key2 | KeyCode::Down | KeyCode::Key8 => {
                        let up = matches!(code, KeyCode::Up | KeyCode::Key2);
                        if let Some((min, max, step)) = ::abi::prop::lookup_bounds(prop_id) {
                            let cur = ctx.config.prop_get(prop_id);
                            let next = if up {
                                cur.saturating_add(step).min(max)
                            } else {
                                cur.saturating_sub(step).max(min)
                            };
                            let _ = ctx.config.prop_set(prop_id, next);
                        }
                        EventResult::Consumed
                    }
                    KeyCode::Menu | KeyCode::Key5 | KeyCode::Exit => {
                        self.editing = false;
                        let _ = ctx.flush_config();
                        EventResult::Consumed
                    }
                    _ => EventResult::Consumed,
                }
            } else {
                match code {
                    KeyCode::Up | KeyCode::Key2 => {
                        self.sel = if self.sel > 0 { self.sel - 1 } else { 5 };
                        EventResult::Consumed
                    }
                    KeyCode::Down | KeyCode::Key8 => {
                        self.sel = if self.sel < 5 { self.sel + 1 } else { 0 };
                        EventResult::Consumed
                    }
                    KeyCode::Menu | KeyCode::Key5 => {
                        if self.sel < 4 {
                            self.editing = true;
                        } else if self.sel == 4 {
                            ctx.show_hold_confirm(
                                TAG_RESET_SETTINGS,
                                "Reset Settings?",
                                "Restore defaults?",
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
        }

        fn on_modal_result(
            &mut self,
            ctx: &mut SysContext,
            result: &ModalResult<'_>,
        ) -> EventResult {
            if let ModalResult::Confirmed(tag) = *result {
                if tag == TAG_RESET_SETTINGS {
                    unsafe {
                        crate::config::MasterConfig::init_in_place(ctx.config);
                    }
                    ctx.config.sanitize();
                    ctx.config.hw_dirty = 0x3F;
                    #[cfg(feature = "filesystem")]
                    let _ = ctx.config.save_all(ctx.storage);
                    return EventResult::Consumed;
                } else if tag == TAG_REBOOT_FW {
                    let _ = ctx.flush_config();
                    kernel::power::PowerControl::reboot();
                }
            }
            EventResult::Ignored
        }

        #[inline(never)]
        fn render(&self, canvas: &mut Canvas, ctx: &SysContext) {
            let start = if self.sel >= 4 {
                (self.sel - 3) as usize
            } else {
                0
            };
            let mut num_buf = [0u8; 5];
            for vis in 0..4usize {
                let idx = start + vis;
                if idx >= 6 {
                    break;
                }
                let y = (9 + vis * 13) as u8;
                let is_sel = idx == self.sel as usize;
                let fg = if is_sel {
                    gfx::widgets::draw_selection_capsule(
                        canvas,
                        [2, y, 120, 13],
                        if self.editing {
                            gfx::widgets::CAPSULE_ROUND
                        } else {
                            gfx::widgets::CAPSULE_FILL_ROUND
                        },
                    )
                } else {
                    Color::Fill
                };
                let label = utils::format::nth_cstr(&FALLBACK_LABELS, idx as u8);
                gfx::text::draw_str(
                    canvas,
                    6,
                    y as i16 + 9,
                    label,
                    &gfx::fonts::mumatrix_regular::FONT,
                    fg,
                );
                if idx < 4 {
                    let val = ctx.config.prop_get(FALLBACK_PROPS[idx]) as u32;
                    let val_str = gfx::text::format_u32(val, &mut num_buf);
                    gfx::text::draw_str_align(
                        canvas,
                        119,
                        y as i16 + 9,
                        val_str,
                        &gfx::fonts::tomthumb::FONT,
                        fg,
                        gfx::text::Align::Right,
                    );
                }
            }
            gfx::widgets::draw_scrollbar(canvas, 125, 9, 52, start, 6, 4);
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
