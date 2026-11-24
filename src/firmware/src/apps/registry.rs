//! Application Registry and OS Application Dispatch.
//!
//! `LauncherApp` (`APP_ID_LAUNCHER`) is the sole resident ROM application;
//! `Radio` (`APP_ID_VFO`) and `Settings` (`APP_ID_SETTINGS`) are external `.krx` packages
//! launched through the dynamic slot alongside third-party apps.

use crate::apps::LauncherApp;
use crate::orchestrator::{proc_book_mut, proc_book_ref};
use crate::services::SysContext;

use ::abi::CapabilitySet;
use kernel::app::{App, AppMetadata, EventResult, ModalResult, StatusBarConfig};
use kernel::event::Event;

/// Dynamic application slot record for installable `.krx` bundles.
#[derive(Clone, Copy)]
pub struct DynamicAppRecord {
    pub metadata: AppMetadata,
    pub required_capabilities: CapabilitySet,
}

pub const APP_ID_LAUNCHER: usize = 0;
pub const APP_ID_VFO: usize = 1;
pub const APP_ID_SETTINGS: usize = 2;
const RESERVED_APP_SLOTS: usize = 3;

#[unsafe(link_section = ".uninit")]
static mut LAUNCHER_STORAGE: core::mem::MaybeUninit<LauncherApp> =
    core::mem::MaybeUninit::uninit();

#[inline(always)]
fn launcher_ptr() -> *mut LauncherApp {
    core::ptr::addr_of_mut!(LAUNCHER_STORAGE) as *mut LauncherApp
}

pub fn init_all_apps() {
    AppRegistry::init_app(APP_ID_LAUNCHER);
}

pub struct AppRegistry;

impl AppRegistry {
    #[inline(always)]
    pub const fn count() -> usize {
        RESERVED_APP_SLOTS
    }

    #[inline(always)]
    pub fn app_count() -> usize {
        RESERVED_APP_SLOTS + usize::from(Self::is_dynamic_loaded())
    }

    #[inline(always)]
    pub fn is_dynamic_loaded() -> bool {
        proc_book_ref().dynamic_app.is_some()
    }

    #[inline(never)]
    pub fn init_app(app_id: usize) {
        if app_id == APP_ID_LAUNCHER {
            unsafe {
                let p = launcher_ptr();
                core::ptr::write_bytes(p as *mut u8, 0, core::mem::size_of::<LauncherApp>());
                LauncherApp::init_at(p);
            }
        }
    }

    #[inline(always)]
    pub fn app_instance(app_id: usize) -> Option<*mut ()> {
        (app_id == APP_ID_LAUNCHER).then(|| launcher_ptr() as *mut ())
    }

    #[inline(always)]
    pub fn app_size(app_id: usize) -> usize {
        if app_id == APP_ID_LAUNCHER {
            core::mem::size_of::<LauncherApp>()
        } else {
            0
        }
    }

    pub fn descriptor(app_id: usize) -> Option<AppMetadata> {
        match app_id {
            APP_ID_LAUNCHER => Some(AppMetadata { name: "Home" }),
            APP_ID_VFO => Some(AppMetadata { name: "Radio" }),
            APP_ID_SETTINGS => Some(AppMetadata { name: "Settings" }),
            _ => proc_book_ref().dynamic_app.map(|d| d.metadata),
        }
    }

    pub fn required_caps(app_id: usize) -> CapabilitySet {
        match app_id {
            APP_ID_LAUNCHER | APP_ID_SETTINGS => CapabilitySet::NONE,
            APP_ID_VFO => CapabilitySet::NONE
                .with(::abi::Cap::RadioRx)
                .with(::abi::Cap::RadioTx),
            _ => proc_book_ref()
                .dynamic_app
                .map(|d| d.required_capabilities)
                .unwrap_or(CapabilitySet::NONE),
        }
    }

    pub fn register_dynamic(record: DynamicAppRecord) -> usize {
        proc_book_mut().dynamic_app = Some(record);
        RESERVED_APP_SLOTS
    }

    /// Forget the dynamic app record; its process is reaped by `orchestrator::reap_pid_resources`.
    pub fn unregister_dynamic() {
        proc_book_mut().dynamic_app = None;
    }

    #[inline(never)]
    pub fn on_start(app_id: usize, ctx: &mut SysContext<'_, '_>) {
        if app_id == APP_ID_LAUNCHER {
            unsafe { (*launcher_ptr()).on_start(ctx) };
        } else if Self::is_dynamic_loaded() {
            #[cfg(feature = "krx_native")]
            unsafe {
                crate::runtime::native_runner::native_on_start(ctx);
            }
        }
    }

    #[inline(never)]
    pub fn on_event(app_id: usize, ctx: &mut SysContext<'_, '_>, event: &Event) -> EventResult {
        if app_id == APP_ID_LAUNCHER {
            unsafe { (*launcher_ptr()).on_event(ctx, event) }
        } else if Self::is_dynamic_loaded() {
            #[cfg(feature = "krx_native")]
            return unsafe { crate::runtime::native_runner::native_on_event(ctx, event) };
            #[cfg(not(feature = "krx_native"))]
            EventResult::Ignored
        } else {
            EventResult::Ignored
        }
    }

    #[inline(never)]
    pub fn on_modal_result(
        app_id: usize,
        ctx: &mut SysContext<'_, '_>,
        result: &ModalResult<'_>,
    ) -> EventResult {
        if app_id == APP_ID_LAUNCHER {
            unsafe { (*launcher_ptr()).on_modal_result(ctx, result) }
        } else if Self::is_dynamic_loaded() {
            #[cfg(feature = "krx_native")]
            return unsafe { crate::runtime::native_runner::native_on_modal_result(ctx, result) };
            #[cfg(not(feature = "krx_native"))]
            EventResult::Ignored
        } else {
            EventResult::Ignored
        }
    }

    #[inline(never)]
    pub fn render<'a>(
        app_id: usize,
        canvas: &mut gfx::Canvas,
        ctx: &'a SysContext<'a, '_>,
        sb_cfg: &mut StatusBarConfig<'_>,
    ) {
        if app_id == APP_ID_LAUNCHER {
            unsafe { (*launcher_ptr()).render(canvas, ctx) };
            sb_cfg.visible = true;
            sb_cfg.title = Some("Home");
        } else {
            #[cfg(feature = "krx_native")]
            if Self::is_dynamic_loaded() {
                unsafe {
                    crate::runtime::native_runner::native_on_render(canvas, ctx, sb_cfg);
                }
            }
        }
    }

    #[inline(never)]
    pub fn on_exit(app_id: usize, ctx: &mut SysContext<'_, '_>) {
        if app_id == APP_ID_LAUNCHER {
            unsafe { (*launcher_ptr()).on_exit(ctx) };
        } else if Self::is_dynamic_loaded() {
            #[cfg(feature = "krx_native")]
            unsafe {
                crate::runtime::native_runner::native_on_exit(ctx);
            }
        }
    }
}
