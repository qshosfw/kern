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
    use gfx::Canvas;
    use kernel::app::{App, EventResult, ModalResult};
    use kernel::event::Event;

    const TAG_MISSING_SETTINGS: u8 = 19;

    /// Minimal ROM fallback when `/apps/settings.krx` is not installed.
    #[repr(C)]
    pub struct SettingsApp {
        pub(crate) title: &'static str,
    }

    #[inline(always)]
    pub fn is_action_module_active() -> bool {
        false
    }

    impl SettingsApp {
        pub unsafe fn init_at(ptr: *mut Self) {
            unsafe {
                (*ptr).title = "Settings";
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
            ctx.show_confirm(
                TAG_MISSING_SETTINGS,
                "Settings Missing",
                "Install /apps/settings.krx\nvia kern wire provision.",
                "OK",
                "",
            );
        }

        fn on_event(&mut self, _ctx: &mut SysContext, _event: &Event) -> EventResult {
            EventResult::Ignored
        }

        fn on_modal_result(
            &mut self,
            ctx: &mut SysContext,
            result: &ModalResult<'_>,
        ) -> EventResult {
            if matches!(
                *result,
                ModalResult::Confirmed(TAG_MISSING_SETTINGS)
                    | ModalResult::Canceled(TAG_MISSING_SETTINGS)
            ) {
                ctx.post_event(crate::apps::Shortcut::CloseApp.into());
                return EventResult::Consumed;
            }
            EventResult::Ignored
        }

        fn render(&self, _canvas: &mut Canvas, _ctx: &SysContext) {}

        fn on_exit(&mut self, _ctx: &mut SysContext) {}
    }
}

#[cfg(not(any(test, feature = "resident_settings")))]
pub use app::SettingsApp;
