//! Contained KRX Application Exception Capture, Unwind Landing, and Firmware Dialog.
//!
//! When a KRX application triggers a `HardFault` or a runtime contract violation
//! (`Denied`, `BadPtr`, `BadArg`, `UnleasedBus`, `NoOp`, `Stack`), the fault is
//! contained without rebooting the radio:
//! 1. RF TX is immediately quenched (`bk4829_fault_quench`).
//! 2. The stacked exception frame PC is rewritten to `krx_fault_landing`, which
//!    restores the saved host stack/registers and returns `KRX_FAULT_SENTINEL`.
//! 3. `finish_fault_cleanup` revokes all leases, VFO locks, services, IPC, and
//!    opens the firmware Exception Dialog (`[Retry] [Close]`).

use crate::services::SysContext;
#[cfg(all(target_arch = "arm", not(test)))]
use kernel::proc::fault::KRX_FAULT_SENTINEL;
use kernel::proc::fault::{AppFault, Exception};

/// Non-zero while executing inside a KRX guest invocation (`invoke_krx_trampoline`).
#[unsafe(no_mangle)]
pub static mut GUEST_IN_FLIGHT: u8 = 0;

/// Saved host MSP inside `invoke_krx_trampoline` so `krx_fault_landing` can unwind cleanly.
#[unsafe(no_mangle)]
pub static mut FAULT_HOST_SP: u32 = 0;

/// Guest GOT base (`r9`) passed to `invoke_krx_trampoline`.
#[unsafe(no_mangle)]
pub static mut GUEST_GOT_BASE: u32 = 0;

/// Last captured application exception (all-zero in `.bss` when idle).
static mut ACTIVE_FAULT: AppFault = AppFault::NONE;

/// Package path of the most recently faulted KRX app (used by the dialog `[Retry]` action).
static mut FAULT_PKG_PATH: [u8; 32] = [0u8; 32];
static mut FAULT_PKG_LEN: u8 = 0;

/// Record a synchronous runtime contract violation (e.g. capability denied, bad pointer).
#[inline(never)]
pub fn raise_contract_fault(kind: Exception, op: u8, addr: u32) {
    let slot = super::native_runner::slot_ref();
    unsafe {
        ACTIVE_FAULT = AppFault {
            pid: slot.pid,
            kind,
            op,
            _pad: 0,
            pc_off: 0,
            _pad2: 0,
            addr,
        };
    }
}

/// Returns true if an unconsumed app fault is pending.
#[inline(always)]
pub fn has_pending_fault() -> bool {
    unsafe { (*core::ptr::addr_of!(ACTIVE_FAULT)).is_active() }
}

/// Returns the saved package path for the last faulted app, if any.
pub fn fault_pkg_path() -> Option<&'static str> {
    unsafe {
        let len = FAULT_PKG_LEN as usize;
        let path = &*core::ptr::addr_of!(FAULT_PKG_PATH);
        if len == 0 || len > path.len() {
            None
        } else {
            // SAFETY: populated exclusively from `slot.pkg_path.as_str()`.
            Some(core::str::from_utf8_unchecked(&path[..len]))
        }
    }
}

/// Clears the saved fault package path after handling `[Retry]` or `[Close]`.
pub fn clear_fault_pkg() {
    unsafe {
        FAULT_PKG_LEN = 0;
    }
}

/// Called from `HardFault` (Handler mode, interrupts masked) when `GUEST_IN_FLIGHT != 0`.
///
/// Quenches RF TX, records `AppFault`, rewrites the stacked exception frame `PC` to
/// `krx_fault_landing`, and returns `1` so `HardFault` executes `bx lr` straight into
/// Thread mode recovery. Returns `0` if the exception frame pointer is outside SRAM.
#[cfg(all(target_arch = "arm", not(test)))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn try_contain_app_fault(frame: *mut u32) -> u32 {
    unsafe {
        GUEST_IN_FLIGHT = 0;
    }
    bsp::bk4829_fault_quench();

    let sp = frame as u32;
    if sp < 0x2000_0000 || sp > (0x2000_4000 - 32) || (sp & 3) != 0 {
        return 0;
    }

    let fault_pc = unsafe { *frame.add(6) };
    let arena_base = crate::data::arena::arena_base_ptr() as u32;
    let slot = super::native_runner::slot_ref();
    unsafe {
        ACTIVE_FAULT = AppFault {
            pid: slot.pid,
            kind: Exception::Fault,
            op: 0,
            _pad: 0,
            pc_off: fault_pc.wrapping_sub(arena_base) as u16,
            _pad2: 0,
            addr: fault_pc,
        };
        // Rewrite stacked PC (Thumb bit cleared per ARMv6-M exception return) & clean xPSR.
        *frame.add(6) = (krx_fault_landing as *const () as usize as u32) & !1;
        *frame.add(7) = 0x0100_0000;
    }
    1
}

/// ARMv6-M Thread-mode unwind landing pad entered via `HardFault` exception return.
/// Restores the host `sp`, callee-saved registers `r4..r11`, re-enables IRQs, and
/// returns `KRX_FAULT_SENTINEL` from `invoke_krx_trampoline`.
#[cfg(all(target_arch = "arm", not(test)))]
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn krx_fault_landing() -> ! {
    core::arch::naked_asm!(
        "ldr r5, ={host_sp}",
        "ldr r4, [r5]",
        "mov sp, r4",
        "ldr r0, ={sentinel}",
        "cpsie i",
        "pop {{r4-r7}}",
        "mov r8, r4",
        "mov r9, r5",
        "mov r10, r6",
        "mov r11, r7",
        "pop {{r4-r7, pc}}",
        host_sp = sym FAULT_HOST_SP,
        sentinel = const KRX_FAULT_SENTINEL,
    );
}

/// ARMv6-M fault-guarded trampoline for invoking a KRX native entry point.
/// Saves host `r4..r11`, `lr`, and `sp` so any `HardFault` inside guest code unwinds cleanly.
#[cfg(all(target_arch = "arm", not(test)))]
#[unsafe(naked)]
pub unsafe extern "C" fn invoke_krx_trampoline(
    op: u32,
    api_table: *const (),
    param: *const (),
    entry_point: usize,
) -> u32 {
    core::arch::naked_asm!(
        "push {{r4-r7, lr}}",
        "mov r4, r8",
        "mov r5, r9",
        "mov r6, r10",
        "mov r7, r11",
        "push {{r4-r7}}",
        "mov r4, sp",
        "ldr r5, ={host_sp}",
        "str r4, [r5]",
        "ldr r5, ={in_flight}",
        "movs r4, #1",
        "strb r4, [r5]",
        "ldr r5, ={guest_got}",
        "ldr r4, [r5]",
        "mov r9, r4",
        "blx r3",
        "ldr r5, ={in_flight}",
        "movs r4, #0",
        "strb r4, [r5]",
        "pop {{r4-r7}}",
        "mov r8, r4",
        "mov r9, r5",
        "mov r10, r6",
        "mov r11, r7",
        "pop {{r4-r7, pc}}",
        host_sp = sym FAULT_HOST_SP,
        in_flight = sym GUEST_IN_FLIGHT,
        guest_got = sym GUEST_GOT_BASE,
    );
}

/// Complete post-fault containment in normal Thread mode: quench RF, revoke all resources
/// held by the faulted process, unload the slot, and open the firmware Exception Dialog.
#[inline(never)]
pub fn finish_fault_cleanup(ctx: &mut SysContext<'_, '_>) {
    let fault = unsafe {
        let f = ACTIVE_FAULT;
        ACTIVE_FAULT = AppFault::NONE;
        f
    };
    if !fault.is_active() {
        return;
    }

    let slot = super::native_runner::slot_mut();
    let pid = if fault.pid.0 != 0 {
        fault.pid
    } else {
        slot.pid
    };

    // Snapshot package path & format dialog text into the transient staging buffer BEFORE
    // clearing the slot (`slot.app_name` lives in `NATIVE_SLOT`, disjoint from `TRANSIENT_ARENA`,
    // and `show_confirm` copies the text into `ACTIVE_MODAL` before returning).
    let msg_buf =
        unsafe { &mut *(crate::data::arena::staging_buf().as_mut_ptr() as *mut [u8; 96]) };
    let msg = fault.format_message(super::native_runner::active_native_app_name(), msg_buf);

    let pkg = slot.pkg_path_str().as_bytes();
    let pkg_len = pkg.len().min(32);
    unsafe {
        (&mut *core::ptr::addr_of_mut!(FAULT_PKG_PATH))[..pkg_len].copy_from_slice(&pkg[..pkg_len]);
        FAULT_PKG_LEN = pkg_len as u8;
    }

    // 1. Hardware safety & slot teardown (shared with normal exit) + PCB/lease reap.
    if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut(pid) {
        pcb.ledger.fault_count = pcb.ledger.fault_count.saturating_add(1);
    }
    let _ = crate::orchestrator::reap_pid_resources(pid.0);
    super::native_runner::teardown_slot_and_hw(ctx, pid, false);

    // 2. Present firmware Exception Dialog ([Retry] / [Close]) without rebooting.
    ctx.play_dialog_sound(crate::services::melody::DialogSoundType::Error);
    ctx.show_confirm(
        crate::orchestrator::TAG_FAULT_CONFIRM,
        "App Stopped",
        msg,
        "Retry",
        "Close",
    );
    crate::orchestrator::sink_modal_keys();
}
