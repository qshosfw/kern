//! Process bookkeeping owned by the orchestrator: the PCB table, resource leases, the
//! status tray, and atomic reaping of a process across all of them.

use kernel::proc::ProcessTable;

pub const MAX_PROCESSES: usize = 4;

#[unsafe(link_section = ".uninit")]
pub static mut PROC_TABLE: core::mem::MaybeUninit<ProcessTable<MAX_PROCESSES>> =
    core::mem::MaybeUninit::uninit();

#[inline(always)]
pub fn proc_table() -> &'static ProcessTable<MAX_PROCESSES> {
    proc_table_mut()
}

pub fn proc_table_mut() -> &'static mut ProcessTable<MAX_PROCESSES> {
    unsafe { (*core::ptr::addr_of_mut!(PROC_TABLE)).assume_init_mut() }
}

pub fn init_proc_table() {
    ProcessTable::init_in_place(unsafe { &mut *core::ptr::addr_of_mut!(PROC_TABLE) });
    #[cfg(feature = "krx_native")]
    crate::runtime::init_native_slot();
}

/// Consolidated zero-initialized process bookkeeping anchor (status tray, resource leases,
/// and active dynamic `.krx` app metadata share one base literal on Thumb-1).
#[repr(C)]
pub(crate) struct ProcBookkeeping {
    pub(crate) tray: kernel::proc::StatusTray<2>,
    pub(crate) leases: kernel::proc::ResourceLeaseManager<2>,
    pub(crate) dynamic_app: Option<crate::apps::registry::DynamicAppRecord>,
}

static mut PROC_BOOK: ProcBookkeeping = ProcBookkeeping {
    tray: kernel::proc::StatusTray::new(),
    leases: kernel::proc::ResourceLeaseManager::new(),
    dynamic_app: None,
};

#[inline(always)]
pub(crate) fn proc_book_ref() -> &'static ProcBookkeeping {
    proc_book_mut()
}

pub(crate) fn proc_book_mut() -> &'static mut ProcBookkeeping {
    unsafe { &mut *core::ptr::addr_of_mut!(PROC_BOOK) }
}

#[inline(always)]
pub fn lease_manager() -> &'static kernel::proc::ResourceLeaseManager<2> {
    &proc_book_ref().leases
}

#[inline(always)]
pub fn lease_manager_mut() -> &'static mut kernel::proc::ResourceLeaseManager<2> {
    &mut proc_book_mut().leases
}

#[inline(always)]
pub fn status_tray() -> &'static kernel::proc::StatusTray<2> {
    &proc_book_ref().tray
}

#[inline(always)]
pub fn status_tray_mut() -> &'static mut kernel::proc::StatusTray<2> {
    &mut proc_book_mut().tray
}

/// Release every kernel resource owned by `pid`; `Pid(0)` (no process) owns nothing.
pub(crate) fn reap_pid_only(pid: kernel::proc::Pid) {
    if pid.0 == 0 {
        return;
    }
    lease_manager_mut().revoke_all_for_owner(pid);
    status_tray_mut().clear_icon(pid);
    #[cfg(feature = "krx_native")]
    {
        crate::runtime::background::BgManager::stop_pid(pid);
        crate::runtime::syscall::service::service_registry_mut().unbind_provider(pid);
        crate::runtime::syscall::ipc_channel_mut().purge_pid(pid.0);
    }
    let _ = proc_table_mut().terminate(pid);
}

/// Atomically reap a terminated non-protected process across PCB, Leases, Tray, Services, and dynamic registry.
pub fn reap_pid_resources(target_pid: u8) -> bool {
    if target_pid == 0 {
        return false;
    }
    let Some(pcb) = proc_table().find(kernel::proc::Pid(target_pid)) else {
        return false;
    };
    if pcb.app_id as usize == crate::apps::APP_ID_LAUNCHER
        || pcb.exec_type == kernel::proc::ExecType::StaticFirmware
    {
        return false;
    }
    let app_id = pcb.app_id as usize;
    let pid = pcb.pid;

    reap_pid_only(pid);
    if app_id == crate::AppRegistry::count() {
        crate::AppRegistry::unregister_dynamic();
    }
    true
}

pub(super) fn record_proc_event_and_stack(app_id: usize) {
    if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
        let used_stk = kernel::sys::SystemDiagnostics::stack_used_now() as u16;
        pcb.health.record_stack_depth(used_stk);
    }
}
