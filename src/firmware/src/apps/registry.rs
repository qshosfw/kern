//! Application Registry and OS Application Dispatch.

use crate::services::SysContext;

use kernel::app::{AppMetadata, AppVTable, EventResult, ModalResult, StatusBarConfig};
use kernel::caps::CapabilitySet;
use kernel::event::Event;

/// Application record stored in ROM describing an OS built-in application.
#[derive(Clone, Copy)]
pub struct OsAppDescriptor {
    pub metadata: AppMetadata,
    pub vtable: AppVTable<SysContext<'static, 'static>>,
    pub init: unsafe fn(*mut ()),
    pub required_capabilities: CapabilitySet,
    pub offset: u16,
    pub instance_size: u16,
}

/// Dynamic application slot for installable application bundles (.app)
#[derive(Clone, Copy)]
pub struct DynamicAppRecord {
    pub metadata: AppMetadata,
    pub vtable: AppVTable<SysContext<'static, 'static>>,
    pub instance: *mut (),
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
        meta: AppMetadata { name: "Radio", icon: &gfx::icons::ICON_16_RADIO, icon_8: &gfx::icons::ICON_APP_RADIO },
        caps: CapabilitySet::NONE.with(kernel::caps::Cap::RadioRx).with(kernel::caps::Cap::RadioTx)
    ],
    APP_ID_SETTINGS => crate::apps::SettingsApp [
        meta: AppMetadata { name: "Settings", icon: &gfx::icons::ICON_16_SETTINGS, icon_8: &gfx::icons::ICON_APP_SETTINGS },
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
        let pid = if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut_by_app(dyn_id) {
            pcb.set_state(kernel::proc::ProcState::Dead);
            pcb.pid
        } else {
            kernel::proc::Pid(dyn_id as u8)
        };
        crate::orchestrator::lease_manager_mut().revoke_all_for_owner(pid);
        crate::orchestrator::status_tray_mut().clear_icon(pid);
        unsafe {
            *core::ptr::addr_of_mut!(DYNAMIC_APP) = None;
        }
    }

    #[inline(never)]
    fn get_target(
        app_id: usize,
    ) -> Option<(*mut (), &'static AppVTable<SysContext<'static, 'static>>)> {
        if let Some(entry) = APP_TABLE.get(app_id) {
            let ptr = unsafe {
                (core::ptr::addr_of_mut!(OS_APP_STORAGE) as *mut u8).add(entry.offset as usize)
                    as *mut ()
            };
            Some((ptr, &entry.vtable))
        } else {
            unsafe {
                let dyn_ref = (*core::ptr::addr_of!(DYNAMIC_APP)).as_ref()?;
                Some((dyn_ref.instance, &dyn_ref.vtable))
            }
        }
    }

    pub fn on_start<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>) {
        if let Some((ptr, vtable)) = Self::get_target(app_id) {
            unsafe { (vtable.on_start)(ptr, core::mem::transmute(ctx)) }
        }
    }

    pub fn on_event<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>, event: &Event) -> EventResult {
        if let Some((ptr, vtable)) = Self::get_target(app_id) {
            unsafe { (vtable.on_event)(ptr, core::mem::transmute(ctx), event) }
        } else {
            EventResult::Ignored
        }
    }

    pub fn on_modal_result<'a>(
        app_id: usize,
        ctx: &mut SysContext<'a, '_>,
        result: &ModalResult<'_>,
    ) -> EventResult {
        if let Some((ptr, vtable)) = Self::get_target(app_id) {
            unsafe { (vtable.on_modal_result)(ptr, core::mem::transmute(ctx), result) }
        } else {
            EventResult::Ignored
        }
    }

    pub fn render<'a>(
        app_id: usize,
        canvas: &mut gfx::Canvas,
        ctx: &'a SysContext<'a, '_>,
        sb_cfg: &mut StatusBarConfig<'_>,
    ) {
        if let Some((ptr, vtable)) = Self::get_target(app_id) {
            unsafe { (vtable.render)(ptr, canvas, core::mem::transmute(ctx), sb_cfg) }
        }
    }

    pub fn on_exit<'a>(app_id: usize, ctx: &mut SysContext<'a, '_>) {
        if let Some((ptr, vtable)) = Self::get_target(app_id) {
            unsafe { (vtable.on_exit)(ptr, core::mem::transmute(ctx)) }
        }
    }
}
