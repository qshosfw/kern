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
    pub required_capabilities: CapabilitySet,
}

/// Dynamic application slot for installable application bundles (.app / .krx)
#[derive(Clone, Copy)]
pub struct DynamicAppRecord {
    pub metadata: AppMetadata,
    pub vtable: AppVTable<FirmwareContext<'static, 'static>>,
    pub instance: *mut (),
    pub required_capabilities: CapabilitySet,
}

static mut DYNAMIC_APP: Option<DynamicAppRecord> = None;

// Declarative OS Applications Registry
kernel::define_os_apps! {
    context: crate::services::FirmwareContext<'static, 'static>;
    APP_ID_LAUNCHER => crate::apps::LauncherApp [
        storage: APP_LAUNCHER,
        meta: AppMetadata { name: "Launcher", icon: &[0u8; 32] },
        caps: CapabilitySet::NONE
    ],
    APP_ID_VFO => crate::apps::VfoApp [
        storage: APP_VFO,
        meta: AppMetadata { name: "Radio", icon: &gfx::icons::ICON_16_RADIO },
        caps: CapabilitySet::NONE.with(kernel::caps::Cap::RadioRx).with(kernel::caps::Cap::RadioTx)
    ],
    APP_ID_SETTINGS => crate::apps::SettingsApp [
        storage: APP_SETTINGS,
        meta: AppMetadata { name: "Settings", icon: &gfx::icons::ICON_16_SETTINGS },
        caps: CapabilitySet::NONE
    ],
    #[cfg(feature = "filesystem")]
    APP_ID_EXPLORER => crate::apps::ExplorerApp [
        storage: APP_EXPLORER,
        meta: AppMetadata { name: "Explorer", icon: &gfx::icons::ICON_16_SCANNER },
        caps: CapabilitySet::NONE.with(kernel::caps::Cap::StorageRead).with(kernel::caps::Cap::StorageWrite)
    ],
}

pub struct AppRegistry;

impl AppRegistry {
    #[inline(always)]
    pub const fn count() -> usize {
        APP_TABLE.len()
    }

    #[inline(always)]
    pub const fn app_count() -> usize {
        APP_TABLE.len()
    }

    pub fn descriptor(app_id: usize) -> Option<AppMetadata> {
        if let Some(entry) = APP_TABLE.get(app_id) {
            Some(entry.metadata)
        } else {
            unsafe { (*core::ptr::addr_of!(DYNAMIC_APP)).map(|d| d.metadata) }
        }
    }

    pub fn register_dynamic(record: DynamicAppRecord) -> Result<usize, ()> {
        unsafe {
            if (*core::ptr::addr_of!(DYNAMIC_APP)).is_some() {
                return Err(());
            }
            *core::ptr::addr_of_mut!(DYNAMIC_APP) = Some(record);
            Ok(APP_TABLE.len())
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
        result: &ModalResult,
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
