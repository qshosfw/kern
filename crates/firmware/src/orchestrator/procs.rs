//! Process bookkeeping owned by the orchestrator: the PCB table, resource leases, the
//! status tray, and atomic reaping of a process across all of them.

use kernel::proc::{ProcState, ProcessTable};

pub const MAX_PROCESSES: usize = 8;

#[unsafe(link_section = ".uninit")]
pub static mut PROC_TABLE: core::mem::MaybeUninit<ProcessTable<MAX_PROCESSES>> =
    core::mem::MaybeUninit::uninit();

#[inline(always)]
pub fn proc_table() -> &'static ProcessTable<MAX_PROCESSES> {
    unsafe { (*core::ptr::addr_of!(PROC_TABLE)).assume_init_ref() }
}

#[inline(always)]
pub fn proc_table_mut() -> &'static mut ProcessTable<MAX_PROCESSES> {
    unsafe { (*core::ptr::addr_of_mut!(PROC_TABLE)).assume_init_mut() }
}

#[inline(never)]
pub fn init_proc_table() {
    ProcessTable::init_in_place(unsafe { &mut *core::ptr::addr_of_mut!(PROC_TABLE) });
    #[cfg(feature = "krx_native")]
    crate::runtime::init_native_slot();
}

static mut LEASE_MGR: kernel::proc::ResourceLeaseManager<8> =
    kernel::proc::ResourceLeaseManager::new();

#[inline(always)]
pub fn lease_manager() -> &'static kernel::proc::ResourceLeaseManager<8> {
    unsafe { &*core::ptr::addr_of!(LEASE_MGR) }
}

#[inline(always)]
pub fn lease_manager_mut() -> &'static mut kernel::proc::ResourceLeaseManager<8> {
    unsafe { &mut *core::ptr::addr_of_mut!(LEASE_MGR) }
}

static mut STATUS_TRAY: kernel::proc::StatusTray<4> = kernel::proc::StatusTray::new();

#[inline(always)]
pub fn status_tray() -> &'static kernel::proc::StatusTray<4> {
    unsafe { &*core::ptr::addr_of!(STATUS_TRAY) }
}

#[inline(always)]
pub fn status_tray_mut() -> &'static mut kernel::proc::StatusTray<4> {
    unsafe { &mut *core::ptr::addr_of_mut!(STATUS_TRAY) }
}

#[inline(never)]
pub fn record_proc_activation(app_id: usize) {
    if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
        pcb.ledger.record_activation();
    }
}

/// Returns true if process `app_id` is currently suspended, dormant, stopped, or dead.
#[inline(never)]
pub fn is_proc_paused(app_id: usize) -> bool {
    proc_table().find_by_app(app_id).is_some_and(|pcb| {
        matches!(
            pcb.state,
            ProcState::Suspended | ProcState::Dormant | ProcState::Stopped | ProcState::Dead
        )
    })
}

/// Release every kernel resource owned by `pid`; `Pid(0)` (no process) owns nothing.
pub(crate) fn reap_pid_only(pid: kernel::proc::Pid) {
    lease_manager_mut().revoke_all_for_owner(pid);
    status_tray_mut().clear_icon(pid);
    #[cfg(feature = "krx_native")]
    crate::runtime::syscall::service::service_registry_mut().unbind_provider(pid);
    let _ = proc_table_mut().terminate(pid);
}

/// Atomically reap a terminated non-protected process across PCB, Leases, Tray, Services, and dynamic registry.
pub fn reap_pid_resources(target_pid: u8) -> bool {
    if target_pid <= 4 {
        return false;
    }
    let Some(pcb) = proc_table().find(kernel::proc::Pid(target_pid)) else {
        return false;
    };
    let app_id = pcb.app_id;
    let pid = pcb.pid;

    reap_pid_only(pid);
    if app_id == crate::AppRegistry::count() {
        crate::AppRegistry::unregister_dynamic();
    }
    true
}

#[inline(never)]
pub(super) fn record_proc_event_and_stack(app_id: usize) {
    if let Some(pcb) = proc_table_mut().find_mut_by_app(app_id) {
        pcb.ledger.record_event();
        let used_stk = kernel::sys::SystemDiagnostics::stack_used_now() as u16;
        pcb.ledger.record_stack_depth(used_stk);
    }
}
