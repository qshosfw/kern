//! Application Registry and OS Application Dispatch.

use crate::services::SysContext;

use ::abi::CapabilitySet;
use kernel::app::{App, AppMetadata, EventResult, ModalResult, StatusBarConfig};
use kernel::event::Event;

/// Application record stored in ROM describing an OS built-in application.
#[derive(Clone, Copy)]
pub struct OsAppDescriptor {
    pub metadata: AppMetadata,
    pub required_capabilities: CapabilitySet,
    pub offset: u16,
    pub instance_size: u16,
}

/// Dynamic application slot for installable application bundles (.krx)
#[derive(Clone, Copy)]
pub struct DynamicAppRecord {
    pub metadata: AppMetadata,
    pub required_capabilities: CapabilitySet,
}

static mut DYNAMIC_APP: Option<DynamicAppRecord> = None;

// Declarative OS Shell Registry (`Launcher` is the sole resident ROM app;
// `Radio` (`APP_ID_VFO`) and `Settings` (`APP_ID_SETTINGS`) are external `.krx` packages).
kernel::define_os_apps! {
    context: crate::services::SysContext<'static, 'static>;
    APP_ID_LAUNCHER => crate::apps::LauncherApp [
        meta: AppMetadata { name: "Home" },
        caps: CapabilitySet::NONE
    ],
}

pub const APP_ID_VFO: usize = 1;
pub const APP_ID_SETTINGS: usize = 2;
const RESERVED_APP_SLOTS: usize = 3;

pub struct AppRegistry;

impl AppRegistry {
    #[inline(always)]
    pub const fn count() -> usize {
        RESERVED_APP_SLOTS
    }

    #[inline(always)]
    pub fn app_count() -> usize {
        RESERVED_APP_SLOTS
            + unsafe {
                if (*core::ptr::addr_of!(DYNAMIC_APP)).is_some() {
                    1
                } else {
                    0
                }
            }
    }

    #[inline(always)]
    pub fn is_dynamic_loaded() -> bool {
        unsafe { (*core::ptr::addr_of!(DYNAMIC_APP)).is_some() }
    }

    #[inline(always)]
    pub fn init_app(app_id: usize) {
        init_app(app_id);
    }

    #[inline(always)]
    pub fn app_instance(app_id: usize) -> Option<*mut ()> {
        APP_TABLE.get(app_id).map(|entry| unsafe {
            (core::ptr::addr_of_mut!(OS_APP_STORAGE) as *mut u8).add(entry.offset as usize)
                as *mut ()
        })
    }

    #[inline(always)]
    pub fn app_size(app_id: usize) -> usize {
        APP_TABLE
            .get(app_id)
            .map(|entry| entry.instance_size as usize)
            .unwrap_or(0)
    }

    pub fn descriptor(app_id: usize) -> Option<AppMetadata> {
        match app_id {
            APP_ID_LAUNCHER => Some(AppMetadata { name: "Home" }),
            APP_ID_VFO => Some(AppMetadata { name: "Radio" }),
            APP_ID_SETTINGS => Some(AppMetadata { name: "Settings" }),
            _ => unsafe { (*core::ptr::addr_of!(DYNAMIC_APP)).map(|d| d.metadata) },
        }
    }

    pub fn required_caps(app_id: usize) -> CapabilitySet {
        match app_id {
            APP_ID_LAUNCHER | APP_ID_SETTINGS => CapabilitySet::NONE,
            APP_ID_VFO => CapabilitySet::NONE
                .with(::abi::Cap::RadioRx)
                .with(::abi::Cap::RadioTx),
            _ => unsafe {
                (*core::ptr::addr_of!(DYNAMIC_APP))
                    .map(|d| d.required_capabilities)
                    .unwrap_or(CapabilitySet::NONE)
            },
        }
    }

    pub fn register_dynamic(record: DynamicAppRecord) -> usize {
        unsafe {
            *core::ptr::addr_of_mut!(DYNAMIC_APP) = Some(record);
            RESERVED_APP_SLOTS
        }
    }

    /// Forget the dynamic app record; its process is reaped by `orchestrator::reap_pid_resources`.
    pub fn unregister_dynamic() {
        unsafe {
            *core::ptr::addr_of_mut!(DYNAMIC_APP) = None;
        }
    }

    #[inline(never)]
    pub fn on_start<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>) {
        unsafe {
            let st = &mut *core::ptr::addr_of_mut!(OS_APP_STORAGE);
            if app_id == APP_ID_LAUNCHER {
                (*st.APP_ID_LAUNCHER.as_mut_ptr()).on_start(ctx);
            } else if Self::is_dynamic_loaded() {
                #[cfg(feature = "krx_native")]
                crate::runtime::native_runner::native_on_start(core::mem::transmute(ctx));
            }
        }
    }

    #[inline(never)]
    pub fn on_event<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>, event: &Event) -> EventResult {
        unsafe {
            let st = &mut *core::ptr::addr_of_mut!(OS_APP_STORAGE);
            if app_id == APP_ID_LAUNCHER {
                (*st.APP_ID_LAUNCHER.as_mut_ptr()).on_event(ctx, event)
            } else if Self::is_dynamic_loaded() {
                #[cfg(feature = "krx_native")]
                return crate::runtime::native_runner::native_on_event(
                    core::mem::transmute(ctx),
                    event,
                );
                #[cfg(not(feature = "krx_native"))]
                EventResult::Ignored
            } else {
                EventResult::Ignored
            }
        }
    }

    #[inline(never)]
    pub fn on_modal_result<'a>(
        app_id: usize,
        ctx: &mut SysContext<'a, '_>,
        result: &ModalResult<'_>,
    ) -> EventResult {
        unsafe {
            let st = &mut *core::ptr::addr_of_mut!(OS_APP_STORAGE);
            if app_id == APP_ID_LAUNCHER {
                (*st.APP_ID_LAUNCHER.as_mut_ptr()).on_modal_result(ctx, result)
            } else if Self::is_dynamic_loaded() {
                #[cfg(feature = "krx_native")]
                return crate::runtime::native_runner::native_on_modal_result(
                    core::mem::transmute(ctx),
                    result,
                );
                #[cfg(not(feature = "krx_native"))]
                EventResult::Ignored
            } else {
                EventResult::Ignored
            }
        }
    }

    #[inline(never)]
    pub fn render<'a>(
        app_id: usize,
        canvas: &mut gfx::Canvas,
        ctx: &'a SysContext<'a, '_>,
        sb_cfg: &mut StatusBarConfig<'_>,
    ) {
        unsafe {
            if app_id == APP_ID_LAUNCHER {
                let st = &*core::ptr::addr_of!(OS_APP_STORAGE);
                (*st.APP_ID_LAUNCHER.as_ptr()).render(canvas, ctx);
                sb_cfg.visible = true;
                sb_cfg.title = Some("Home");
            } else {
                #[cfg(feature = "krx_native")]
                if Self::is_dynamic_loaded() {
                    crate::runtime::native_runner::native_on_render(
                        canvas,
                        core::mem::transmute(ctx),
                        sb_cfg,
                    );
                }
            }
        }
    }

    #[inline(never)]
    pub fn on_exit<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>) {
        unsafe {
            let st = &mut *core::ptr::addr_of_mut!(OS_APP_STORAGE);
            if app_id == APP_ID_LAUNCHER {
                (*st.APP_ID_LAUNCHER.as_mut_ptr()).on_exit(ctx);
            } else if Self::is_dynamic_loaded() {
                #[cfg(feature = "krx_native")]
                crate::runtime::native_runner::native_on_exit(core::mem::transmute(ctx));
            }
        }
    }
}
