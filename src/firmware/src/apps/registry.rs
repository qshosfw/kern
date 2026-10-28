//! Application Registry and OS Application Dispatch.

use crate::services::FirmwareContext;

use kernel::app::{AppMetadata, AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::caps::CapabilitySet;
use kernel::event::Event;

/// Application record stored in ROM describing an OS built-in application.
#[derive(Clone, Copy)]
pub struct OsAppDescriptor {
    pub metadata: AppMetadata,
    pub vtable: AppVTable<FirmwareContext<'static, 'static>>,
    pub instance: fn() -> *mut (),
    pub init: fn(*mut ()),
    pub required_capabilities: CapabilitySet,
    pub instance_size: usize,
}

/// Dynamic application slot for installable application bundles (.app)
#[derive(Clone, Copy)]
pub struct DynamicAppRecord {
    pub metadata: AppMetadata,
    pub vtable: AppVTable<FirmwareContext<'static, 'static>>,
    pub instance: *mut (),
    pub required_capabilities: CapabilitySet,
}

static mut DYNAMIC_APP: Option<DynamicAppRecord> = None;

// Declarative OS Applications Registry (Core Triad)
kernel::define_os_apps! {
    context: crate::services::FirmwareContext<'static, 'static>;
    APP_ID_LAUNCHER => crate::apps::LauncherApp [
        meta: AppMetadata { name: "Launcher", icon: &[0u8; 32], icon_8: &[0u8; 8] },
        caps: CapabilitySet::NONE
    ],
    APP_ID_VFO => crate::apps::VfoApp [
        meta: AppMetadata { name: "Radio", icon: &crate::apps::vfo::assets::ICON_16, icon_8: &crate::apps::vfo::assets::ICON_8 },
        caps: CapabilitySet::NONE.with(kernel::caps::Cap::RadioRx).with(kernel::caps::Cap::RadioTx)
    ],
    APP_ID_SETTINGS => crate::apps::SettingsApp [
        meta: AppMetadata { name: "Settings", icon: &crate::apps::settings::assets::ICON_16, icon_8: &crate::apps::settings::assets::ICON_8 },
        caps: CapabilitySet::NONE
    ],
    #[cfg(feature = "app_tasks")]
    APP_ID_TASKS => crate::apps::TasksApp [
        meta: AppMetadata { name: "Tasks", icon: &crate::apps::launcher::assets::ICON_DIAG_16, icon_8: &crate::apps::launcher::assets::ICON_DIAG_8 },
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
        APP_TABLE.get(app_id).map(|entry| (entry.instance)())
    }

    #[inline(always)]
    pub fn app_size(app_id: usize) -> usize {
        APP_TABLE
            .get(app_id)
            .map(|entry| entry.instance_size)
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
        unsafe {
            *core::ptr::addr_of_mut!(DYNAMIC_APP) = None;
        }
    }

    pub fn on_start<'a>(app_id: usize, ctx: &mut FirmwareContext<'a, '_>) {
        if let Some(entry) = APP_TABLE.get(app_id) {
            let ptr = (entry.instance)();
            unsafe { (entry.vtable.on_start)(ptr, core::mem::transmute(ctx)) }
        } else {
            unsafe {
                if let Some(dyn_app) = *core::ptr::addr_of!(DYNAMIC_APP) {
                    (dyn_app.vtable.on_start)(dyn_app.instance, core::mem::transmute(ctx));
                }
            }
        }
    }

    pub fn on_event<'a>(
        app_id: usize,
        ctx: &mut FirmwareContext<'a, '_>,
        event: &Event,
    ) -> EventResult {
        if let Some(entry) = APP_TABLE.get(app_id) {
            let ptr = (entry.instance)();
            unsafe { (entry.vtable.on_event)(ptr, core::mem::transmute(ctx), event) }
        } else {
            unsafe {
                if let Some(dyn_app) = *core::ptr::addr_of!(DYNAMIC_APP) {
                    (dyn_app.vtable.on_event)(dyn_app.instance, core::mem::transmute(ctx), event)
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    pub fn on_modal_result<'a>(
        app_id: usize,
        ctx: &mut FirmwareContext<'a, '_>,
        result: &ModalResult<'_>,
    ) -> EventResult {
        if let Some(entry) = APP_TABLE.get(app_id) {
            let ptr = (entry.instance)();
            unsafe { (entry.vtable.on_modal_result)(ptr, core::mem::transmute(ctx), result) }
        } else {
            unsafe {
                if let Some(dyn_app) = *core::ptr::addr_of!(DYNAMIC_APP) {
                    (dyn_app.vtable.on_modal_result)(
                        dyn_app.instance,
                        core::mem::transmute(ctx),
                        result,
                    )
                } else {
                    EventResult::Ignored
                }
            }
        }
    }

    pub fn render<'a>(
        app_id: usize,
        canvas: &mut gfx::Canvas,
        ctx: &'a FirmwareContext<'a, '_>,
        sb_cfg: &mut StatusBarConfig<'_>,
    ) {
        if let Some(entry) = APP_TABLE.get(app_id) {
            let ptr = (entry.instance)();
            unsafe { (entry.vtable.render)(ptr, canvas, core::mem::transmute(ctx), sb_cfg) }
        } else {
            unsafe {
                if let Some(dyn_app) = *core::ptr::addr_of!(DYNAMIC_APP) {
                    (dyn_app.vtable.render)(
                        dyn_app.instance,
                        canvas,
                        core::mem::transmute(ctx),
                        sb_cfg,
                    );
                }
            }
        }
    }

    pub fn on_exit<'a>(app_id: usize, ctx: &mut FirmwareContext<'a, '_>) {
        if let Some(entry) = APP_TABLE.get(app_id) {
            let ptr = (entry.instance)();
            unsafe { (entry.vtable.on_exit)(ptr, core::mem::transmute(ctx)) }
        } else {
            unsafe {
                if let Some(dyn_app) = *core::ptr::addr_of!(DYNAMIC_APP) {
                    (dyn_app.vtable.on_exit)(dyn_app.instance, core::mem::transmute(ctx));
                }
            }
        }
    }
}
