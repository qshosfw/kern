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

#[cfg(all(target_arch = "arm", not(test)))]
use kernel::runtime::fault::KRX_FAULT_SENTINEL;
use kernel::runtime::fault::{AppFault, Exception};

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

/// Package path & PID of the currently loaded / most recently faulted KRX app.
/// Stored in `.bss` (far below `_stack_end`) so a stack overflow that clobbers `NATIVE_SLOT`
/// cannot lose recovery identity or trigger an out-of-bounds panic in `finish_fault_cleanup`.
static mut FAULT_PKG: utils::format::InlineStr<31> = utils::format::InlineStr::new();
static mut LOADED_PKG: utils::format::InlineStr<31> = utils::format::InlineStr::new();
static mut LOADED_PKG_PID: u8 = 0;

/// Snapshot package path and PID when a KRX package loads into `NATIVE_SLOT`.
#[inline(never)]
pub fn note_loaded_pkg(pid: kernel::proc::Pid, path: &str) {
    unsafe {
        (*core::ptr::addr_of_mut!(LOADED_PKG)).set(path);
        LOADED_PKG_PID = pid.0;
    }
}

/// Clear loaded package metadata on clean guest unload.
#[inline(always)]
pub fn clear_loaded_pkg() {
    unsafe {
        (*core::ptr::addr_of_mut!(LOADED_PKG)).clear();
        LOADED_PKG_PID = 0;
    }
}

/// Returns true when executing in Thread mode inside a fault-guarded KRX guest trampoline.
#[cfg(all(target_arch = "arm", not(test)))]
#[inline(always)]
pub fn can_contain_guest_in_thread() -> bool {
    let ipsr: u32;
    unsafe {
        core::arch::asm!("mrs {}, IPSR", out(reg) ipsr, options(nomem, nostack));
        (ipsr & 0x3F) == 0 && GUEST_IN_FLIGHT != 0 && FAULT_HOST_SP != 0
    }
}

/// Immediately contain a synchronous guest fault or stack exhaustion in Thread mode
/// by unwinding straight back to `krx_fault_landing` without disabling USBD or rebooting.
#[cfg(all(target_arch = "arm", not(test)))]
#[inline(never)]
pub unsafe fn unwind_guest_fault(kind: Exception, op: u8) -> ! {
    unsafe {
        cortex_m::interrupt::disable();
        GUEST_IN_FLIGHT = 0;
    }
    bsp::bk4829_fault_quench();
    let pid = unsafe {
        let p = LOADED_PKG_PID;
        if p != 0 {
            kernel::proc::Pid(p)
        } else {
            super::native_runner::slot_ref().pid
        }
    };
    unsafe {
        ACTIVE_FAULT = AppFault {
            pid,
            kind,
            op,
            _pad: 0,
            pc_off: 0,
            _pad2: 0,
            addr: 0,
        };
    }
    // Rewind MSP to `FAULT_HOST_SP` first so `paint_stack` paints safely below the host frame.
    unsafe {
        core::arch::asm!(
            "ldr r0, ={host_sp}",
            "ldr r0, [r0]",
            "mov sp, r0",
            "bl {paint}",
            "bl {landing}",
            host_sp = sym FAULT_HOST_SP,
            paint = sym kernel::sys::SystemDiagnostics::paint_stack,
            landing = sym krx_fault_landing,
            options(noreturn)
        );
    }
}

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
    let pkg = unsafe { &*core::ptr::addr_of!(FAULT_PKG) };
    if pkg.is_empty() {
        None
    } else {
        Some(pkg.as_str())
    }
}

/// Clears the saved fault package path after handling `[Retry]` or `[Close]`.
pub fn clear_fault_pkg() {
    unsafe {
        (*core::ptr::addr_of_mut!(FAULT_PKG)).clear();
    }
}

/// Called from `HardFault` (Handler mode, interrupts masked) when `GUEST_IN_FLIGHT != 0`.
///
/// Quenches RF TX, records `AppFault`, rewrites the stacked exception frame `PC` to
/// `krx_fault_landing`, and returns `1` so `HardFault` executes `bx lr` straight into
/// Thread mode recovery. Returns `0` if the exception frame pointer is outside SRAM.
#[cfg(all(target_arch = "arm", not(test)))]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn try_contain_app_fault(frame: *mut u32, is_stack_ovf: u32) -> u32 {
    unsafe {
        GUEST_IN_FLIGHT = 0;
    }
    bsp::bk4829_fault_quench();

    let sp = frame as u32;
    if sp < 0x2000_0000 || sp > (0x2000_4000 - 32) || (sp & 3) != 0 {
        return 0;
    }

    let (kind, fault_pc) = if is_stack_ovf != 0 {
        kernel::sys::SystemDiagnostics::paint_stack();
        (Exception::Stack, 0u32)
    } else {
        (Exception::Fault, unsafe { *frame.add(6) })
    };
    let arena_base = crate::data::arena::arena_base_ptr() as u32;
    let pid = unsafe {
        let p = LOADED_PKG_PID;
        if p != 0 {
            kernel::proc::Pid(p)
        } else {
            super::native_runner::slot_ref().pid
        }
    };
    unsafe {
        ACTIVE_FAULT = AppFault {
            pid,
            kind,
            op: 0,
            _pad: 0,
            pc_off: if fault_pc != 0 {
                fault_pc.wrapping_sub(arena_base) as u16
            } else {
                0
            },
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
/// Saves host `r4..r11`, `lr`, and outer `sp` so any `HardFault` inside guest code
/// (including nested P0 leaf overlays) unwinds cleanly to the outermost host frame.
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
        "ldr r5, ={in_flight}",
        "ldrb r4, [r5]",
        "cmp r4, #0",
        "bne 2f",
        "mov r6, sp",
        "ldr r7, ={host_sp}",
        "str r6, [r7]",
        "2:",
        "adds r4, #1",
        "strb r4, [r5]",
        "ldr r5, ={guest_got}",
        "ldr r4, [r5]",
        "mov r9, r4",
        "blx r3",
        "ldr r5, ={in_flight}",
        "ldrb r4, [r5]",
        "cmp r4, #0",
        "beq 3f",
        "subs r4, #1",
        "strb r4, [r5]",
        "3:",
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
pub fn finish_fault_cleanup() {
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
    } else if unsafe { LOADED_PKG_PID } != 0 {
        kernel::proc::Pid(unsafe { LOADED_PKG_PID })
    } else {
        slot.pid
    };

    // Format dialog text into a small stack buffer (MSP is already unwound to the host frame)
    // so an active RigWire file transfer in `staging_buf()` is never clobbered.
    let mut msg_buf = [0u8; 96];
    let msg = fault.format_message(super::native_runner::active_native_app_name(), &mut msg_buf);

    // Prefer the `.bss` load-time path snapshot (immune to stack overflow into `NATIVE_SLOT`),
    // falling back to `slot.pkg_path_str()` only when not snapshotted (e.g. unit tests).
    unsafe {
        let loaded = &*core::ptr::addr_of!(LOADED_PKG);
        if !loaded.is_empty() {
            FAULT_PKG = *loaded;
        } else {
            (*core::ptr::addr_of_mut!(FAULT_PKG)).set(slot.pkg_path_str());
        }
    }
    clear_loaded_pkg();

    // 1. Hardware safety & slot teardown (shared with normal exit) + PCB/lease reap.
    if let Some(pcb) = crate::orchestrator::proc_table_mut().find_mut(pid) {
        pcb.health.record_fault();
    }
    let _ = crate::orchestrator::reap_pid_resources(pid.0);
    super::native_runner::teardown_slot_and_hw(pid, false);
    #[cfg(not(test))]
    crate::system_canvas().clear();

    // 2. Present firmware Exception Dialog ([Retry] / [Close]) with clear category title.
    #[cfg(not(test))]
    crate::services::play_dialog_sound(crate::services::melody::DialogSoundType::Error);
    crate::orchestrator::compositor::active_modal_mut().show_confirm(
        crate::orchestrator::TAG_FAULT_CONFIRM,
        fault.kind.dialog_title(),
        msg,
        "Retry",
        "Close",
    );
    crate::orchestrator::sink_modal_keys();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupted_native_slot_pkg_path_never_panics_and_preserves_loaded_snapshot() {
        super::super::native_runner::init_native_slot();
        let slot = super::super::native_runner::slot_mut();
        slot.set_pkg_path("/apps/settings.krx");
        note_loaded_pkg(kernel::proc::Pid(7), "/apps/settings.krx");

        // Simulate a Cortex-M0+ stack overflow clobbering `NATIVE_SLOT` fields with stack garbage.
        slot.pkg_path.len = 0xFF;
        slot.pkg_path.buf[0] = 0xFE;
        assert_eq!(slot.pkg_path_str(), "");

        // `.bss` load-time snapshot stays intact far below `_stack_end`.
        unsafe {
            let pid = *core::ptr::addr_of!(LOADED_PKG_PID);
            let loaded = &*core::ptr::addr_of!(LOADED_PKG);
            assert_eq!(pid, 7);
            assert_eq!(loaded.as_str(), "/apps/settings.krx");
        }
        clear_loaded_pkg();
        super::super::native_runner::init_native_slot();
    }
}
