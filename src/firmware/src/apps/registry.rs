//! Application Registry and OS Application Dispatch.

use crate::services::SysContext;

use kernel::app::{App, AppMetadata, EventResult, ModalResult, StatusBarConfig};
use kernel::caps::CapabilitySet;
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

// Declarative OS Applications Registry (Core Triad)
kernel::define_os_apps! {
    context: crate::services::SysContext<'static, 'static>;
    APP_ID_LAUNCHER => crate::apps::LauncherApp [
        meta: AppMetadata { name: "Home", icon: &[0u8; 32], icon_8: &[0u8; 8] },
        caps: CapabilitySet::NONE
    ],
    APP_ID_VFO => crate::apps::VfoApp [
        meta: AppMetadata { name: "Radio", icon: gfx::icons::ICON_16_RADIO, icon_8: gfx::icons::ICON_APP_RADIO },
        caps: CapabilitySet::NONE.with(kernel::caps::Cap::RadioRx).with(kernel::caps::Cap::RadioTx)
    ],
    APP_ID_SETTINGS => crate::apps::SettingsApp [
        meta: AppMetadata { name: "Settings", icon: gfx::icons::ICON_16_SETTINGS, icon_8: gfx::icons::ICON_APP_SETTINGS },
        caps: CapabilitySet::NONE
    ],
}

pub struct AppRegistry;

impl AppRegistry {
    #[inline(always)]
    pub const fn count() -> usize {
        APP_TABLE.len()
    }

    #[inline(always)]
    pub fn app_count() -> usize {
        APP_TABLE.len()
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
        if let Some(entry) = APP_TABLE.get(app_id) {
            Some(entry.metadata)
        } else {
            unsafe { (*core::ptr::addr_of!(DYNAMIC_APP)).map(|d| d.metadata) }
        }
    }

    pub fn required_caps(app_id: usize) -> CapabilitySet {
        if let Some(entry) = APP_TABLE.get(app_id) {
            entry.required_capabilities
        } else {
            unsafe {
                (*core::ptr::addr_of!(DYNAMIC_APP))
                    .map(|d| d.required_capabilities)
                    .unwrap_or(CapabilitySet::NONE)
            }
        }
    }

    pub fn register_dynamic(record: DynamicAppRecord) -> usize {
        unsafe {
            *core::ptr::addr_of_mut!(DYNAMIC_APP) = Some(record);
            APP_TABLE.len()
        }
    }

    pub fn unregister_dynamic() {
        let dyn_id = APP_TABLE.len();
        let dyn_pid = crate::orchestrator::proc_table()
            .find_by_app(dyn_id)
            .map(|p| p.pid);
        unsafe {
            *core::ptr::addr_of_mut!(DYNAMIC_APP) = None;
        }
        if let Some(pid) = dyn_pid {
            crate::orchestrator::reap_pid_only(pid);
        }
    }

    #[inline(never)]
    pub fn on_start<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>) {
        unsafe {
            let st = &mut *core::ptr::addr_of_mut!(OS_APP_STORAGE);
            match app_id {
                APP_ID_LAUNCHER => (*st.APP_ID_LAUNCHER.as_mut_ptr()).on_start(ctx),
                APP_ID_VFO => (*st.APP_ID_VFO.as_mut_ptr()).on_start(ctx),
                APP_ID_SETTINGS => (*st.APP_ID_SETTINGS.as_mut_ptr()).on_start(ctx),
                _ if Self::is_dynamic_loaded() => {
                    #[cfg(feature = "krx_native")]
                    crate::runtime::native_runner::native_on_start(core::mem::transmute(ctx));
                }
                _ => {}
            }
        }
    }

    #[inline(never)]
    pub fn on_event<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>, event: &Event) -> EventResult {
        unsafe {
            let st = &mut *core::ptr::addr_of_mut!(OS_APP_STORAGE);
            match app_id {
                APP_ID_LAUNCHER => (*st.APP_ID_LAUNCHER.as_mut_ptr()).on_event(ctx, event),
                APP_ID_VFO => (*st.APP_ID_VFO.as_mut_ptr()).on_event(ctx, event),
                APP_ID_SETTINGS => (*st.APP_ID_SETTINGS.as_mut_ptr()).on_event(ctx, event),
                _ if Self::is_dynamic_loaded() => {
                    #[cfg(feature = "krx_native")]
                    return crate::runtime::native_runner::native_on_event(
                        core::mem::transmute(ctx),
                        event,
                    );
                    #[cfg(not(feature = "krx_native"))]
                    EventResult::Ignored
                }
                _ => EventResult::Ignored,
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
            match app_id {
                APP_ID_LAUNCHER => (*st.APP_ID_LAUNCHER.as_mut_ptr()).on_modal_result(ctx, result),
                APP_ID_VFO => (*st.APP_ID_VFO.as_mut_ptr()).on_modal_result(ctx, result),
                APP_ID_SETTINGS => (*st.APP_ID_SETTINGS.as_mut_ptr()).on_modal_result(ctx, result),
                _ if Self::is_dynamic_loaded() => {
                    #[cfg(feature = "krx_native")]
                    return crate::runtime::native_runner::native_on_modal_result(
                        core::mem::transmute(ctx),
                        result,
                    );
                    #[cfg(not(feature = "krx_native"))]
                    EventResult::Ignored
                }
                _ => EventResult::Ignored,
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
            let st = &*core::ptr::addr_of!(OS_APP_STORAGE);
            let title = match app_id {
                APP_ID_LAUNCHER => {
                    (*st.APP_ID_LAUNCHER.as_ptr()).render(canvas, ctx);
                    "Home"
                }
                APP_ID_VFO => {
                    (*st.APP_ID_VFO.as_ptr()).render(canvas, ctx);
                    "Radio"
                }
                APP_ID_SETTINGS => {
                    let app = &*st.APP_ID_SETTINGS.as_ptr();
                    app.render(canvas, ctx);
                    app.title
                }
                _ => {
                    #[cfg(feature = "krx_native")]
                    if Self::is_dynamic_loaded() {
                        crate::runtime::native_runner::native_on_render(
                            canvas,
                            core::mem::transmute(ctx),
                            sb_cfg,
                        );
                    }
                    return;
                }
            };
            *sb_cfg = StatusBarConfig::new(title);
        }
    }

    #[inline(never)]
    pub fn on_exit<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>) {
        unsafe {
            let st = &mut *core::ptr::addr_of_mut!(OS_APP_STORAGE);
            match app_id {
                APP_ID_LAUNCHER => (*st.APP_ID_LAUNCHER.as_mut_ptr()).on_exit(ctx),
                APP_ID_VFO => (*st.APP_ID_VFO.as_mut_ptr()).on_exit(ctx),
                APP_ID_SETTINGS => (*st.APP_ID_SETTINGS.as_mut_ptr()).on_exit(ctx),
                _ if Self::is_dynamic_loaded() => {
                    #[cfg(feature = "krx_native")]
                    crate::runtime::native_runner::native_on_exit(core::mem::transmute(ctx));
                }
                _ => {}
            }
        }
    }
}
